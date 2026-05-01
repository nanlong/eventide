//! # EventRepository Example
//!
//! Demonstrates how to implement the low-level `EventRepository` trait for
//! persisting and querying domain events, then layer a higher-level
//! `AggregateRepository` on top of it. The aggregate modeled here is a
//! `BankAccount` that supports deposits, withdrawals, locking, and unlocking.
//!
//! ## What this example demonstrates
//!
//! - Implementing `EventRepository` with three required operations:
//!   `get_events`, `get_last_events` (incremental load), and `save`
//! - Building a generic `BankAccountRepository<A, E>` that wraps any
//!   `EventRepository` and provides `AggregateRepository` semantics
//! - Driving an aggregate end-to-end via `AggregateRoot`
//! - Querying the raw event log directly (`get_events`) and incrementally
//!   (`get_last_events` for tailing after a known version)
//! - Rehydrating the aggregate via `AggregateRepository::load`
//!
//! ## Prerequisites
//!
//! No external services. Storage is purely in-memory.
//!
//! ## Running
//!
//! ```bash
//! cargo run -p eventide-domain --example event_repository
//! ```
//!
//! ## Expected output
//!
//! Logs each command executed against the aggregate (deposit, withdraw, lock,
//! unlock), prints the full and incremental event lists from the event store,
//! and finally prints the reloaded aggregate's balance, version, and lock state.
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use eventide_domain::aggregate::Aggregate;
use eventide_domain::aggregate_root::AggregateRoot;
use eventide_domain::domain_event::{EventContext, EventEnvelope};
use eventide_domain::entity::Entity;
use eventide_domain::error::{DomainError, DomainResult};
use eventide_domain::event_upcaster::EventUpcasterChain;
use eventide_domain::persist::{
    AggregateRepository, EventRepository, SerializedEvent, deserialize_events, serialize_events,
};
use eventide_domain::value_object::Version;
use eventide_macros::{domain_event, entity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use ulid::Ulid;

// ============================================================================
// Domain model definitions
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
// In-memory EventRepository implementation
// ============================================================================

#[derive(Default, Clone)]
struct InMemoryEventRepository {
    // Map of aggregate_id -> ordered list of serialized events
    events: Arc<Mutex<HashMap<String, Vec<SerializedEvent>>>>,
}

#[async_trait]
impl EventRepository for InMemoryEventRepository {
    /// Returns every event ever recorded for the given aggregate.
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

    /// Returns events whose aggregate_version is strictly greater than `last_version`.
    /// Used for incremental loads (e.g. tailing events after a known snapshot).
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

    /// Appends the given batch of events to the store under their aggregate id.
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
// AggregateRepository implementation (built on top of EventRepository)
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
// Main: end-to-end walkthrough
// ============================================================================

#[tokio::main(flavor = "current_thread")]
async fn main() -> AnyResult<()> {
    let event_repo = Arc::new(InMemoryEventRepository::default());
    let repo = Arc::new(BankAccountRepository::new(event_repo.clone()));
    let root = AggregateRoot::<BankAccount, _>::new(repo.clone());
    let account_id = "account-001".to_string();

    println!("=== EventRepository example (driven through AggregateRoot) ===\n");

    // Drive the aggregate through AggregateRoot for each command.
    println!("--- Executing commands via AggregateRoot ---");

    // Deposit
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Deposit { amount: 1000 }],
            EventContext::default(),
        )
        .await?;
    println!("[ok] Deposit +1000, {} event(s) produced", events.len());

    // Withdraw
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Withdraw { amount: 300 }],
            EventContext::default(),
        )
        .await?;
    println!("[ok] Withdraw -300, {} event(s) produced", events.len());

    // Lock the account
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Lock],
            EventContext::default(),
        )
        .await?;
    println!("[ok] Lock account, {} event(s) produced", events.len());

    // Unlock the account
    let events = root
        .execute(
            &account_id,
            vec![BankAccountCommand::Unlock],
            EventContext::default(),
        )
        .await?;
    println!("[ok] Unlock account, {} event(s) produced\n", events.len());

    // Query the raw event log directly through the EventRepository.
    println!("--- Querying events via EventRepository ---");
    let all_events = event_repo.get_events::<BankAccount>(&account_id).await?;
    println!("Total {} event(s):", all_events.len());
    for event in &all_events {
        println!(
            "  type: {}, version: {}",
            event.event_type(),
            event.event_version()
        );
    }

    // Query only the events newer than version 1 (incremental tailing).
    println!("\n--- Incremental query (version > 1) ---");
    let incremental = event_repo
        .get_last_events::<BankAccount>(&account_id, 1)
        .await?;
    println!("Total {} incremental event(s):", incremental.len());
    for event in &incremental {
        println!(
            "  type: {}, version: {}",
            event.event_type(),
            event.event_version()
        );
    }

    // Rehydrate the aggregate through the higher-level AggregateRepository.
    println!("\n--- Reloading aggregate via AggregateRepository ---");
    let loaded_account = repo.load(&account_id).await?.unwrap();
    println!(
        "account_id: {}, balance: {}, version: {}, locked: {}",
        loaded_account.id(),
        loaded_account.balance,
        loaded_account.version(),
        loaded_account.is_locked
    );

    Ok(())
}
