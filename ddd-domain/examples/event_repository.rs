use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// EventRepository 示例
/// 演示如何实现事件仓储接口，用于持久化和查询领域事件
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use ddd_domain::{
    aggregate::Aggregate,
    aggregate_root::AggregateRoot,
    domain_event::{EventContext, EventEnvelope},
    entity::Entity,
    error::{DomainError, DomainResult},
    event_upcaster::EventUpcasterChain,
    persist::{
        AggregateRepository, EventRepository, SerializedEvent, deserialize_events, serialize_events,
    },
    value_object::Version,
};
use ddd_macros::{domain_event, entity};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

// ============================================================================
// 领域模型定义
// ============================================================================

#[entity]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BankAccount {
    balance: i64,
    is_locked: bool,
}

#[derive(Debug)]
enum BankAccountCommand {
    Deposit { amount: i64 },
    Withdraw { amount: i64 },
    Lock,
    Unlock,
}

#[domain_event(version = 1)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum BankAccountEvent {
    #[event(event_type = "bank_account.deposited")]
    Deposited { amount: i64 },
    #[event(event_type = "bank_account.withdrawn")]
    Withdrawn { amount: i64 },
    #[event(event_type = "bank_account.locked")]
    Locked { reason: String },
    #[event(event_type = "bank_account.unlocked")]
    Unlocked { reason: String },
}

impl Aggregate for BankAccount {
    const TYPE: &'static str = "bank_account";
    type Command = BankAccountCommand;
    type Event = BankAccountEvent;
    type Error = DomainError;

    fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            BankAccountCommand::Deposit { amount } => {
                if amount <= 0 {
                    return Err(DomainError::invalid_command("amount must be positive"));
                }
                if self.is_locked {
                    return Err(DomainError::invalid_state("account is locked"));
                }
                Ok(vec![BankAccountEvent::Deposited {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    amount,
                }])
            }
            BankAccountCommand::Withdraw { amount } => {
                if amount <= 0 {
                    return Err(DomainError::invalid_command("amount must be positive"));
                }
                if self.is_locked {
                    return Err(DomainError::invalid_state("account is locked"));
                }
                if self.balance < amount {
                    return Err(DomainError::invalid_state("insufficient balance"));
                }
                Ok(vec![BankAccountEvent::Withdrawn {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    amount,
                }])
            }
            BankAccountCommand::Lock => {
                if self.is_locked {
                    return Ok(vec![]);
                }
                Ok(vec![BankAccountEvent::Locked {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    reason: "Manual lock".to_string(),
                }])
            }
            BankAccountCommand::Unlock => {
                if !self.is_locked {
                    return Ok(vec![]);
                }
                Ok(vec![BankAccountEvent::Unlocked {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    reason: "Manual unlock".to_string(),
                }])
            }
        }
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            BankAccountEvent::Deposited {
                aggregate_version,
                amount,
                ..
            } => {
                self.balance += amount;
                self.version = *aggregate_version;
            }
            BankAccountEvent::Withdrawn {
                aggregate_version,
                amount,
                ..
            } => {
                self.balance -= amount;
                self.version = *aggregate_version;
            }
            BankAccountEvent::Locked {
                aggregate_version, ..
            } => {
                self.is_locked = true;
                self.version = *aggregate_version;
            }
            BankAccountEvent::Unlocked {
                aggregate_version, ..
            } => {
                self.is_locked = false;
                self.version = *aggregate_version;
            }
        }
    }
}

// ============================================================================
// 内存事件仓储实现
// ============================================================================

#[derive(Default, Clone)]
struct InMemoryEventRepository {
    // aggregate_id -> 事件列表
    events: Arc<Mutex<HashMap<String, Vec<SerializedEvent>>>>,
}

#[async_trait]
impl EventRepository for InMemoryEventRepository {
    /// 获取聚合的所有事件
    async fn get_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
    ) -> DomainResult<Vec<SerializedEvent>> {
        let events = self.events.lock().unwrap();
        Ok(events
            .get(&aggregate_id.to_string())
            .cloned()
            .unwrap_or_default())
    }

    /// 获取聚合从指定版本之后的事件
    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        last_version: usize,
    ) -> DomainResult<Vec<SerializedEvent>> {
        let events = self.events.lock().unwrap();
        Ok(events
            .get(&aggregate_id.to_string())
            .map(|evts| {
                evts.iter()
                    .filter(|e| e.aggregate_version() > last_version)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }

    /// 保存事件到仓储
    async fn save(&self, events: Vec<SerializedEvent>) -> DomainResult<()> {
        if events.is_empty() {
            return Ok(());
        }

        let mut store = self.events.lock().unwrap();
        let aggregate_id = events[0].aggregate_id().to_string();

        let entry = store.entry(aggregate_id.clone()).or_default();
        entry.extend_from_slice(&events);

        Ok(())
    }
}

// ============================================================================
// AggregateRepository 实现（整合 EventRepository）
// ============================================================================

struct BankAccountRepository<A, E>
where
    A: Aggregate,
    E: EventRepository,
{
    event_repo: E,
    upcaster_chain: EventUpcasterChain,
    _phantom: std::marker::PhantomData<A>,
}

impl<A, E> BankAccountRepository<A, E>
where
    A: Aggregate,
    E: EventRepository,
{
    fn new(event_repo: E) -> Self {
        Self {
            event_repo,
            upcaster_chain: EventUpcasterChain::default(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<E> AggregateRepository<BankAccount> for BankAccountRepository<BankAccount, E>
where
    E: EventRepository,
{
    async fn load(
        &self,
        aggregate_id: &<BankAccount as Entity>::Id,
    ) -> Result<Option<BankAccount>, DomainError> {
        let serialized = self
            .event_repo
            .get_events::<BankAccount>(aggregate_id)
            .await?;

        if serialized.is_empty() {
            return Ok(None);
        }

        let envelopes = deserialize_events::<BankAccount>(&self.upcaster_chain, serialized)?;
        let mut account = <BankAccount as Entity>::new(aggregate_id.clone(), Version::new());
        for envelope in envelopes.iter() {
            account.apply(&envelope.payload);
        }
        Ok(Some(account))
    }

    async fn save(
        &self,
        aggregate: &BankAccount,
        events: Vec<BankAccountEvent>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<BankAccount>>, DomainError> {
        let envelopes: Vec<EventEnvelope<BankAccount>> = events
            .into_iter()
            .map(|e| EventEnvelope::new(aggregate.id(), e, context.clone()))
            .collect();

        let serialized = serialize_events(&envelopes)?;
        self.event_repo.save(serialized).await?;

        Ok(envelopes)
    }
}

// ============================================================================
// 主函数：演示使用
// ============================================================================

#[tokio::main(flavor = "current_thread")]
async fn main() -> AnyResult<()> {
    let event_repo = Arc::new(InMemoryEventRepository::default());
    let repo = Arc::new(BankAccountRepository::new(event_repo.clone()));
    let root = AggregateRoot::<BankAccount, _>::new(repo.clone());
    let account_id = "account-001".to_string();

    println!("=== EventRepository 示例（使用 AggregateRoot）===\n");

    // 使用 AggregateRoot 执行命令
    println!("--- 使用 AggregateRoot 执行命令 ---");

    // 存款
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Deposit { amount: 1000 }],
            EventContext::default(),
        )
        .await?;
    println!("✅ 存款 +1000, 产生 {} 个事件", events.len());

    // 取款
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Withdraw { amount: 300 }],
            EventContext::default(),
        )
        .await?;
    println!("✅ 取款 -300, 产生 {} 个事件", events.len());

    // 锁定账户
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Lock],
            EventContext::default(),
        )
        .await?;
    println!("✅ 锁定账户, 产生 {} 个事件", events.len());

    // 解锁账户
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Unlock],
            EventContext::default(),
        )
        .await?;
    println!("✅ 解锁账户, 产生 {} 个事件\n", events.len());

    // 直接使用 EventRepository 查询
    println!("--- 使用 EventRepository 查询事件 ---");
    let all_events = event_repo.get_events::<BankAccount>(&account_id).await?;
    println!("共 {} 个事件:", all_events.len());
    for event in &all_events {
        println!(
            "  类型: {}, 版本: {}",
            event.event_type(),
            event.event_version()
        );
    }

    // 查询增量事件（从版本1之后）
    println!("\n--- 查询增量事件（version > 1）---");
    let incremental = event_repo
        .get_last_events::<BankAccount>(&account_id, 1)
        .await?;
    println!("共 {} 个增量事件:", incremental.len());
    for event in &incremental {
        println!(
            "  类型: {}, 版本: {}",
            event.event_type(),
            event.event_version()
        );
    }

    // 使用 AggregateRepository 重新加载聚合
    println!("\n--- 使用 AggregateRepository 重新加载聚合 ---");
    let loaded_account = repo.load(&account_id).await?.unwrap();
    println!(
        "账户ID: {}, 余额: {}, 版本: {}, 锁定: {}",
        loaded_account.id(),
        loaded_account.balance,
        loaded_account.version(),
        loaded_account.is_locked
    );

    Ok(())
}
