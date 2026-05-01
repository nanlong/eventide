//! # Account Aggregate Example
//!
//! A guided introduction to building a command-driven, event-sourced aggregate
//! with `eventide`. The `Account` aggregate models a simple bank account that
//! can be opened, deposited into, and withdrawn from.
//!
//! ## What this example demonstrates
//!
//! - Defining a strongly typed aggregate ID with `#[entity_id]`
//! - Declaring the aggregate state via `#[entity]` (auto-injects `id` / `version`)
//! - Declaring a versioned domain event enum with `#[domain_event]`
//! - Implementing the `Aggregate` trait: `execute` (decision) and `apply` (projection)
//! - Implementing both an `EventRepository` (raw event storage) and a higher-level
//!   `AggregateRepository` that performs optimistic-locking on the in-memory state
//! - Using `AggregateRoot` to orchestrate load -> execute -> apply -> persist
//!
//! ## Prerequisites
//!
//! No external services. Storage is fully in-memory and the example runs end-to-end.
//!
//! ## Running
//!
//! ```bash
//! cargo run -p eventide-domain --example account_aggregate
//! ```
//!
//! ## Expected output
//!
//! Three "open / deposit / withdraw" operations, each printing the events that
//! were produced, followed by the rehydrated aggregate state showing the final
//! balance and version number.
use async_trait::async_trait;
use eventide_domain::aggregate::Aggregate;
use eventide_domain::aggregate_root::AggregateRoot;
use eventide_domain::domain_event::{EventContext, EventEnvelope};
use eventide_domain::entity::Entity;
use eventide_domain::error::{DomainError, DomainResult};
use eventide_domain::persist::{
    AggregateRepository, EventRepository, SerializedEvent, serialize_events,
};
use eventide_domain::value_object::Version;
use eventide_macros::{domain_event, entity, entity_id};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ulid::Ulid;

// ============================================================================
// Domain model definitions
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

// In-memory implementation of the EventRepository (example use only).
#[derive(Default, Clone)]
struct InMemoryEventRepository {
    // Map of aggregate_id -> ordered list of serialized events
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
    // Latest snapshot of each aggregate, kept in memory so we can avoid replaying events on load.
    states: Arc<Mutex<HashMap<String, Account>>>,
    // Underlying event store dependency; here we wire in the in-memory implementation defined above.
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
        // Read the latest snapshot directly instead of replaying events from the event store.
        let states = self.states.lock().unwrap();
        Ok(states.get(&aggregate_id.to_string()).cloned())
    }

    async fn save(
        &self,
        aggregate: &Account,
        events: Vec<AccountEvent>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<Account>>, DomainError> {
        // Wrap each domain event with shared metadata (aggregate id + EventContext).
        let envelopes: Vec<EventEnvelope<Account>> = events
            .into_iter()
            .map(|e| EventEnvelope::new(aggregate.id(), e, context.clone()))
            .collect();

        // Optimistic-locking check:
        // expected_version = the aggregate's current version BEFORE the new events were applied.
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

        // Persist the serialized events through the underlying event repository.
        let serialized = serialize_events(&envelopes)?;
        self.event_repo.save(serialized).await?;

        // Update the in-memory snapshot to reflect the new state.
        let mut states = self.states.lock().unwrap();
        states.insert(aggregate.id().to_string(), aggregate.clone());

        Ok(envelopes)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    println!("=== Account Aggregate Example ===\n");
    let event_repo = Arc::new(InMemoryEventRepository::default());
    let repo = InMemoryAccountRepo::new(event_repo);
    let root = AggregateRoot::<Account, _>::new(repo.clone());
    // Use a freshly generated ULID as the aggregate ID to avoid FromStr length errors.
    let id = AccountId(Ulid::new());

    // Step 1: open the account.
    let events = root
        .execute(
            &id,
            vec![AccountCommand::Open { new_balance: 1000 }],
            EventContext::default(),
        )
        .await
        .unwrap();
    println!("[ok] Account opened, {} event(s) produced", events.len());
    println!("events: {:?}", events);

    // Step 2: deposit funds.
    let events = root
        .execute(
            &id,
            vec![AccountCommand::Deposit { amount: 500 }],
            EventContext::default(),
        )
        .await
        .unwrap();
    println!("[ok] Deposit +500, {} event(s) produced", events.len());
    println!("events: {:?}", events);

    // Step 3: withdraw funds.
    let events = root
        .execute(
            &id,
            vec![AccountCommand::Withdraw { amount: 200 }],
            EventContext::default(),
        )
        .await
        .unwrap();
    println!("[ok] Withdraw -200, {} event(s) produced", events.len());
    println!("events: {:?}", events);

    // Reload the aggregate from the repository and print the resulting state.
    let loaded = repo.load(&id).await.unwrap().unwrap();
    println!("\n--- Reloaded aggregate ---");
    println!(
        "aggregate: id={}, version={}, balance={}",
        loaded.id(),
        loaded.version(),
        loaded.balance
    );
}
