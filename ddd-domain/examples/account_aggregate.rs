use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Account 聚合示例
/// 演示基于命令驱动的事件溯源：打开账户、存取款等
use async_trait::async_trait;
use ddd_domain::{
    aggregate::Aggregate,
    aggregate_root::AggregateRoot,
    domain_event::{EventContext, EventEnvelope},
    entity::Entity,
    error::{DomainError, DomainResult},
    persist::{AggregateRepository, EventRepository, SerializedEvent, serialize_events},
    value_object::Version,
};
use ddd_macros::{domain_event, entity, entity_id};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

// ============================================================================
// 领域模型定义
// ============================================================================

#[entity_id]
pub struct AccountId(Ulid);

#[entity(id = AccountId)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Account {
    balance: usize,
}

#[derive(Debug)]
enum AccountCommand {
    Open { new_balance: usize },
    Deposit { amount: usize },
    Withdraw { amount: usize },
}

#[domain_event(version = 1)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum AccountEvent {
    #[event(event_type = "account.opened")]
    Opened { new_balance: usize },
    #[event(event_type = "account.deposited")]
    Deposited { amount: usize },
    #[event(event_type = "account.withdrawn")]
    Withdrawn { amount: usize },
}

impl Aggregate for Account {
    const TYPE: &'static str = "account";
    type Command = AccountCommand;
    type Event = AccountEvent;
    type Error = DomainError;

    fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            AccountCommand::Open { new_balance } => {
                if self.version().is_created() {
                    return Err(DomainError::invalid_state("account already opened"));
                }
                let evt = AccountEvent::Opened {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    new_balance,
                };
                Ok(vec![evt])
            }
            AccountCommand::Deposit { amount } => {
                if self.version().is_new() {
                    return Err(DomainError::invalid_state("account not opened"));
                }
                let evt = AccountEvent::Deposited {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    amount,
                };
                Ok(vec![evt])
            }
            AccountCommand::Withdraw { amount } => {
                if self.version().is_new() {
                    return Err(DomainError::invalid_state("account not opened"));
                }
                if self.balance < amount {
                    return Err(DomainError::invalid_state("insufficient funds"));
                }
                let evt = AccountEvent::Withdrawn {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    amount,
                };
                Ok(vec![evt])
            }
        }
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            AccountEvent::Opened {
                aggregate_version,
                new_balance,
                ..
            } => {
                self.balance = *new_balance;
                self.version = *aggregate_version;
            }
            AccountEvent::Deposited {
                aggregate_version,
                amount,
                ..
            } => {
                self.balance += *amount;
                self.version = *aggregate_version;
            }
            AccountEvent::Withdrawn {
                aggregate_version,
                amount,
                ..
            } => {
                self.balance -= *amount;
                self.version = *aggregate_version;
            }
        }
    }
}

// 事件仓储的内存实现（仅用于示例）
#[derive(Default, Clone)]
struct InMemoryEventRepository {
    // aggregate_id -> 事件列表
    events: Arc<Mutex<HashMap<String, Vec<SerializedEvent>>>>,
}

#[async_trait]
impl EventRepository for InMemoryEventRepository {
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

    async fn save(&self, events: Vec<SerializedEvent>) -> DomainResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut store = self.events.lock().unwrap();
        let aggregate_id = events[0].aggregate_id().to_string();
        let entry = store.entry(aggregate_id).or_default();
        entry.extend_from_slice(&events);
        Ok(())
    }
}

#[derive(Clone)]
struct InMemoryAccountRepo {
    // 最新聚合状态（避免通过事件重放恢复）
    states: Arc<Mutex<HashMap<String, Account>>>,
    // 事件存储依赖（示例注入内存实现）
    event_repo: Arc<InMemoryEventRepository>,
}

impl InMemoryAccountRepo {
    fn new(event_repo: Arc<InMemoryEventRepository>) -> Self {
        Self {
            states: Arc::new(Mutex::new(HashMap::new())),
            event_repo,
        }
    }
}

#[async_trait]
impl AggregateRepository<Account> for InMemoryAccountRepo {
    async fn load(&self, aggregate_id: &AccountId) -> Result<Option<Account>, DomainError> {
        // 不通过事件重放恢复，直接读取最新聚合快照
        let states = self.states.lock().unwrap();
        Ok(states.get(&aggregate_id.to_string()).cloned())
    }

    async fn save(
        &self,
        aggregate: &Account,
        events: Vec<AccountEvent>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<Account>>, DomainError> {
        // 先封装事件
        let envelopes: Vec<EventEnvelope<Account>> = events
            .into_iter()
            .map(|e| EventEnvelope::new(aggregate.id(), e, context.clone()))
            .collect();

        // 乐观锁校验：预期版本 = 当前聚合版本 - 新事件数量
        let expected_version =
            Version::from_value(aggregate.version().value().saturating_sub(envelopes.len()));

        let actual_version = {
            let states = self.states.lock().unwrap();
            states
                .get(&aggregate.id().to_string())
                .map(|a| a.version())
                .unwrap_or_default()
        };

        if actual_version != expected_version {
            return Err(DomainError::conflict(
                expected_version.value(),
                actual_version.value(),
            ));
        }

        // 事件持久化（依赖事件仓储）
        let serialized = serialize_events(&envelopes)?;
        self.event_repo.save(serialized).await?;

        // 更新内存中的最新聚合状态
        let mut states = self.states.lock().unwrap();
        states.insert(aggregate.id().to_string(), aggregate.clone());

        Ok(envelopes)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("=== Account 聚合示例 ===\n");
    let event_repo = Arc::new(InMemoryEventRepository::default());
    let repo = InMemoryAccountRepo::new(event_repo);
    let root = AggregateRoot::<Account, _>::new(repo.clone());
    // 生成有效的 ULID 作为聚合 ID，避免 FromStr 无效长度错误
    let id = AccountId(Ulid::new());

    // 开户
    let events = root
        .execute(
            &id,
            vec![AccountCommand::Open { new_balance: 1000 }],
            EventContext::default(),
        )
        .await
        .unwrap();
    println!("✅ 开户，产生 {} 个事件", events.len());
    println!("events: {:?}", events);

    // 存款
    let events = root
        .execute(
            &id,
            vec![AccountCommand::Deposit { amount: 500 }],
            EventContext::default(),
        )
        .await
        .unwrap();
    println!("✅ 存款 +500，产生 {} 个事件", events.len());
    println!("events: {:?}", events);

    // 取款
    let events = root
        .execute(
            &id,
            vec![AccountCommand::Withdraw { amount: 200 }],
            EventContext::default(),
        )
        .await
        .unwrap();
    println!("✅ 取款 -200，产生 {} 个事件", events.len());
    println!("events: {:?}", events);

    // 重新加载并打印状态
    let loaded = repo.load(&id).await.unwrap().unwrap();
    println!("\n--- 重新加载聚合 ---");
    println!(
        "聚合: id={}, 版本={}, 余额={}",
        loaded.id(),
        loaded.version(),
        loaded.balance
    );
}
