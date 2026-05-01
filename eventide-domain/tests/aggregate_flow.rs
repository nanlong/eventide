#![cfg(feature = "eventing")]
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use eventide_domain::aggregate::Aggregate;
use eventide_domain::aggregate_root::AggregateRoot;
use eventide_domain::domain_event::{EventContext, EventEnvelope};
use eventide_domain::entity::Entity;
use eventide_domain::error::{DomainError, DomainResult};
use eventide_domain::persist::{
    AggregateRepository, EventRepository, EventSourcedRepo, SerializedEvent, serialize_events,
};
use eventide_domain::value_object::Version;
use eventide_macros::{domain_event, entity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[entity]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BankAccount {
    balance: i64,
    is_locked: bool,
}

#[derive(Debug)]
enum Cmd {
    Deposit { amount: i64 },
    Withdraw { amount: i64 },
}

#[domain_event(version = 1)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Evt {
    Deposited { amount: i64 },
    Withdrawn { amount: i64 },
    Locked { reason: String },
    Unlocked { reason: String },
}

impl Aggregate for BankAccount {
    const TYPE: &'static str = "bank_account";
    type Command = Cmd;
    type Event = Evt;
    type Error = DomainError;
    fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            Cmd::Deposit { amount } => {
                if amount <= 0 || self.is_locked {
                    return Err(DomainError::invalid_command("bad"));
                }
                Ok(vec![Evt::Deposited {
                    id: ulid::Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    amount,
                }])
            }
            Cmd::Withdraw { amount } => {
                if amount <= 0 || self.is_locked || self.balance < amount {
                    return Err(DomainError::invalid_state("bad"));
                }
                Ok(vec![Evt::Withdrawn {
                    id: ulid::Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    amount,
                }])
            }
        }
    }
    fn apply(&mut self, e: &Self::Event) {
        match e {
            Evt::Deposited {
                aggregate_version,
                amount,
                ..
            } => {
                self.balance += amount;
                self.version = *aggregate_version;
            }
            Evt::Withdrawn {
                aggregate_version,
                amount,
                ..
            } => {
                self.balance -= amount;
                self.version = *aggregate_version;
            }
            Evt::Locked {
                aggregate_version, ..
            } => {
                self.is_locked = true;
                self.version = *aggregate_version;
            }
            Evt::Unlocked {
                aggregate_version, ..
            } => {
                self.is_locked = false;
                self.version = *aggregate_version;
            }
        }
    }
}

#[derive(Default, Clone)]
struct InMemoryEventRepository {
    inner: Arc<Mutex<HashMap<String, Vec<SerializedEvent>>>>,
}

#[async_trait]
impl EventRepository for InMemoryEventRepository {
    async fn get_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
    ) -> DomainResult<Vec<SerializedEvent>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .get(&aggregate_id.to_string())
            .cloned()
            .unwrap_or_default())
    }
    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        last_version: usize,
    ) -> DomainResult<Vec<SerializedEvent>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .get(&aggregate_id.to_string())
            .map(|v| {
                v.iter()
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
        let mut m = self.inner.lock().unwrap();
        let key = events[0].aggregate_id().to_string();
        m.entry(key).or_default().extend_from_slice(&events);
        Ok(())
    }
}

#[tokio::test]
async fn aggregate_persist_and_load_flow() -> AnyResult<()> {
    let event_repo = Arc::new(InMemoryEventRepository::default());
    let upcasters = Arc::new(eventide_domain::event_upcaster::EventUpcasterChain::default());
    let repo = Arc::new(EventSourcedRepo::new(event_repo.clone(), upcasters));
    let root = AggregateRoot::<BankAccount, _>::new(repo.clone());
    let id = "acc-1".to_string();

    // act: execute command pipeline -> events -> persisted via repo
    root.execute(
        &id,
        vec![Cmd::Deposit { amount: 1000 }, Cmd::Withdraw { amount: 300 }],
        EventContext::default(),
    )
    .await?;

    // assert: inspect the underlying event store directly
    let stored = event_repo.get_events::<BankAccount>(&id).await?;
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].aggregate_version(), 1);
    assert_eq!(stored[1].aggregate_version(), 2);

    // assert: rehydrate the aggregate from events via the repository
    let loaded: BankAccount = repo.load(&id).await?.unwrap();
    assert_eq!(loaded.balance, 700);
    assert_eq!(loaded.version(), Version::from_value(2));

    // act: append a Locked event and verify state survives a round trip through the store
    let evs = vec![Evt::Locked {
        id: ulid::Ulid::new().to_string(),
        aggregate_version: Version::from_value(3),
        reason: "m".into(),
    }];
    let envs: Vec<EventEnvelope<BankAccount>> = evs
        .into_iter()
        .map(|e| EventEnvelope::new(&id, e, EventContext::default()))
        .collect();
    let ser = serialize_events(&envs).unwrap();
    event_repo.save(ser).await?;
    let loaded2: BankAccount = repo.load(&id).await?.unwrap();
    assert!(loaded2.is_locked);
    assert_eq!(loaded2.version(), Version::from_value(3));
    Ok(())
}
