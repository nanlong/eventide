//! # Event Upcasting Example
//!
//! Demonstrates how to evolve event schemas over time without rewriting
//! historical data. Old serialized events are transparently transformed into
//! the current shape on read using [`EventUpcaster`] and [`EventUpcasterChain`].
//!
//! ## Scenario
//!
//! A bank account's deposit and withdrawal events have gone through four
//! schema generations. The aggregate code only ever needs to understand the
//! latest version (v4); older events are upcast on the fly during load.
//!
//! ### Deposit event evolution
//!
//! - v1: `account.credited { amount: i64 }`
//! - v2: `account.credited { amount: i64, currency: String }` — added currency field
//! - v3: `account.credited { minor_units: i64, currency: String }` — switched amount to minor units (cents)
//! - v4: `account.deposited { minor_units: i64, currency: String }` — renamed to `deposited`
//!
//! ### Withdrawal event evolution
//!
//! - v1: `account.debited { amount: i64 }`
//! - v2: `account.debited { amount: i64, currency: String }` — added currency field
//! - v3: `account.debited { minor_units: i64, currency: String }` — switched amount to minor units (cents)
//! - v4: `account.withdrew { minor_units: i64, currency: String }` — renamed to `withdrew`
//!
//! ## What this example demonstrates
//!
//! - Implementing [`EventUpcaster`] for each step (v1->v2, v2->v3, v3->v4)
//! - Composing them into an [`EventUpcasterChain`] so loads cascade through every step
//! - Using [`EventSourcedRepo`] for plain event-sourced loads
//! - Using [`SnapshotPolicyRepo`] (with [`SnapshotRepositoryWithPolicy`] as a decorator)
//!   for snapshot-aware loads that combine snapshot + post-snapshot increments
//!
//! ## Prerequisites
//!
//! No external services. Storage is fully in-memory.
//!
//! ## Running
//!
//! ```bash
//! cargo run -p eventide-domain --example event_upcasting
//! ```
//!
//! ## Expected output
//!
//! Logs each upcaster firing as it transforms historical events, prints the
//! rebuilt aggregate balance and version after upcasting, demonstrates a
//! snapshot save and post-snapshot incremental load, and ends with a summary
//! of the application scenario.
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use eventide_domain::aggregate::Aggregate;
use eventide_domain::domain_event::{EventContext, EventEnvelope};
use eventide_domain::entity::Entity;
use eventide_domain::error::{DomainError, DomainResult};
use eventide_domain::event_upcaster::{EventUpcaster, EventUpcasterChain, EventUpcasterResult};
use eventide_domain::persist::{
    AggregateRepository, EventRepository, EventSourcedRepo, SerializedEvent, SerializedSnapshot,
    SnapshotPolicy, SnapshotPolicyRepo, SnapshotRepository, SnapshotRepositoryWithPolicy,
    serialize_events,
};
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
    balance_minor_units: i64, // Balance expressed in minor units (cents)
    currency: String,
}

#[derive(Debug)]
#[allow(dead_code)]
enum BankAccountCommand {
    Credit { minor_units: i64, currency: String },
}

// Current event schema version (v4)
#[domain_event]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum BankAccountEvent {
    #[event(event_type = "account.deposited", event_version = 4)]
    Deposited { minor_units: i64, currency: String },

    #[event(event_type = "account.withdrew", event_version = 4)]
    Withdrew { minor_units: i64, currency: String },
}

impl Aggregate for BankAccount {
    const TYPE: &'static str = "bank_account";
    type Command = BankAccountCommand;
    type Event = BankAccountEvent;
    type Error = DomainError;

    fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            BankAccountCommand::Credit {
                minor_units,
                currency,
            } => {
                if minor_units <= 0 {
                    return Err(DomainError::invalid_command("amount must be positive"));
                }
                Ok(vec![BankAccountEvent::Deposited {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    minor_units,
                    currency,
                }])
            }
        }
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            BankAccountEvent::Deposited {
                aggregate_version,
                minor_units,
                currency,
                ..
            } => {
                // Single-currency account: pin the currency on the first event, then assume it stays consistent.
                if self.currency.is_empty() {
                    self.currency = currency.clone();
                }
                self.balance_minor_units += minor_units;
                // If the event carries the placeholder version 0, auto-increment; otherwise honor the event's value.
                self.version = if aggregate_version.is_new() {
                    self.version.next()
                } else {
                    *aggregate_version
                };
            }
            BankAccountEvent::Withdrew {
                aggregate_version,
                minor_units,
                ..
            } => {
                // Withdrawal: decrement the balance by the event's amount.
                self.balance_minor_units -= minor_units;
                // If the event carries the placeholder version 0, auto-increment; otherwise honor the event's value.
                self.version = if aggregate_version.is_new() {
                    self.version.next()
                } else {
                    *aggregate_version
                };
            }
        }
    }
}

// ============================================================================
// Event Upcasters
// ============================================================================

/// V1 -> V2: add the default `currency` field to `account.credited` payloads.
struct AccountCreditedV1ToV2;

impl EventUpcaster for AccountCreditedV1ToV2 {
    fn applies(&self, event_type: &str, event_version: usize) -> bool {
        event_type == "account.credited" && event_version == 1
    }

    fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
        let mut payload = event.payload().clone();
        let amount = payload
            .get("amount")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountCreditedV1ToV2"),
                    "v1 missing amount",
                )
            })?;

        println!(
            "  [Upcaster V1->V2] amount={} -> amount={}, currency=CNY",
            amount, amount
        );

        // Inject the default currency field into the payload.
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("currency".to_string(), serde_json::json!("CNY"));
        }

        // Rebuild EventContext so the original business context (correlation/causation/actor) is preserved
        let business_context = EventContext::builder()
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .build();

        let upgraded = SerializedEvent::builder()
            .event_id(event.event_id().to_string())
            .event_type(event.event_type().to_string())
            .event_version(2) // bump to v2
            .maybe_sequence_number(None)
            .aggregate_id(event.aggregate_id().to_string())
            .aggregate_type(event.aggregate_type().to_string())
            .aggregate_version(event.aggregate_version())
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .occurred_at(event.occurred_at())
            .payload(payload)
            .context(serde_json::to_value(&business_context)?)
            .build();

        Ok(EventUpcasterResult::One(upgraded))
    }
}

/// V2 -> V3: convert the amount from yuan to minor units (cents).
struct AccountCreditedV2ToV3;

impl EventUpcaster for AccountCreditedV2ToV3 {
    fn applies(&self, event_type: &str, event_version: usize) -> bool {
        event_type == "account.credited" && event_version == 2
    }

    fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
        let mut payload = event.payload().clone();
        let amount = payload
            .get("amount")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountCreditedV2ToV3"),
                    "v2 missing amount",
                )
            })?;
        let currency = payload
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("CNY");

        let minor_units = amount * 100;

        println!(
            "  [Upcaster V2->V3] amount={} {} -> minor_units={} {}",
            amount, currency, minor_units, currency
        );

        // Replace `amount` with `minor_units` in the payload.
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("amount");
            obj.insert("minor_units".to_string(), serde_json::json!(minor_units));
        }

        // Rebuild EventContext so the original business context (correlation/causation/actor) is preserved
        let business_context = EventContext::builder()
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .build();

        let upgraded = SerializedEvent::builder()
            .event_id(event.event_id().to_string())
            .event_type(event.event_type().to_string())
            .event_version(3) // bump to v3
            .maybe_sequence_number(None)
            .aggregate_id(event.aggregate_id().to_string())
            .aggregate_type(event.aggregate_type().to_string())
            .aggregate_version(event.aggregate_version())
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .occurred_at(event.occurred_at())
            .payload(payload)
            .context(serde_json::to_value(&business_context)?)
            .build();

        Ok(EventUpcasterResult::One(upgraded))
    }
}

/// V3 -> V4: rename `account.credited` to `account.deposited` and reshape the payload.
struct AccountCreditedV3ToV4;

impl EventUpcaster for AccountCreditedV3ToV4 {
    fn applies(&self, event_type: &str, event_version: usize) -> bool {
        event_type == "account.credited" && event_version == 3
    }

    fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
        let payload = event.payload();
        let minor_units = payload
            .get("minor_units")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountCreditedV3ToV4"),
                    "v3 missing minor_units",
                )
            })?;
        let currency = payload
            .get("currency")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountCreditedV3ToV4"),
                    "v3 missing currency",
                )
            })?;

        println!(
            "  [Upcaster V3->V4] Renaming account.credited to account.deposited ({} {})",
            minor_units, currency
        );

        let deposited_payload = serde_json::json!({
            "Deposited": {
                "id": event.event_id(),
                "aggregate_version": event.aggregate_version(),
                "minor_units": minor_units,
                "currency": currency,
            }
        });

        // Rebuild EventContext so the original business context (correlation/causation/actor) is preserved
        let business_context = EventContext::builder()
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .build();

        let deposited_event = SerializedEvent::builder()
            .event_id(event.event_id().to_string())
            .event_type("account.deposited".to_string())
            .event_version(4)
            .maybe_sequence_number(None)
            .aggregate_id(event.aggregate_id().to_string())
            .aggregate_type(event.aggregate_type().to_string())
            .aggregate_version(event.aggregate_version())
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .occurred_at(event.occurred_at())
            .payload(deposited_payload)
            .context(serde_json::to_value(&business_context)?)
            .build();

        Ok(EventUpcasterResult::One(deposited_event))
    }
}

// ============================================================================
// Upcasters for withdrawal events
// ============================================================================

/// V1 -> V2: add the default `currency` field to `account.debited` payloads.
struct AccountDebitedV1ToV2;

impl EventUpcaster for AccountDebitedV1ToV2 {
    fn applies(&self, event_type: &str, event_version: usize) -> bool {
        event_type == "account.debited" && event_version == 1
    }

    fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
        let mut payload = event.payload().clone();
        let amount = payload
            .get("amount")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountDebitedV1ToV2"),
                    "v1 missing amount",
                )
            })?;

        println!(
            "  [Upcaster V1->V2] amount={} -> amount={}, currency=CNY (debited)",
            amount, amount
        );

        if let Some(obj) = payload.as_object_mut() {
            obj.insert("currency".to_string(), serde_json::json!("CNY"));
        }

        // Rebuild EventContext so the original business context (correlation/causation/actor) is preserved
        let business_context = EventContext::builder()
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .build();

        let upgraded = SerializedEvent::builder()
            .event_id(event.event_id().to_string())
            .event_type(event.event_type().to_string())
            .event_version(2)
            .maybe_sequence_number(None)
            .aggregate_id(event.aggregate_id().to_string())
            .aggregate_type(event.aggregate_type().to_string())
            .aggregate_version(event.aggregate_version())
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .occurred_at(event.occurred_at())
            .payload(payload)
            .context(serde_json::to_value(&business_context)?)
            .build();

        Ok(EventUpcasterResult::One(upgraded))
    }
}

/// V2 -> V3: convert the amount from yuan to minor units (cents) for debited events.
struct AccountDebitedV2ToV3;

impl EventUpcaster for AccountDebitedV2ToV3 {
    fn applies(&self, event_type: &str, event_version: usize) -> bool {
        event_type == "account.debited" && event_version == 2
    }

    fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
        let mut payload = event.payload().clone();
        let amount = payload
            .get("amount")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountDebitedV2ToV3"),
                    "v2 missing amount",
                )
            })?;
        let currency = payload
            .get("currency")
            .and_then(|v| v.as_str())
            .unwrap_or("CNY");

        let minor_units = amount * 100;

        println!(
            "  [Upcaster V2->V3] amount={} {} -> minor_units={} {} (debited)",
            amount, currency, minor_units, currency
        );

        if let Some(obj) = payload.as_object_mut() {
            obj.remove("amount");
            obj.insert("minor_units".to_string(), serde_json::json!(minor_units));
        }

        // Rebuild EventContext so the original business context (correlation/causation/actor) is preserved
        let business_context = EventContext::builder()
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .build();

        let upgraded = SerializedEvent::builder()
            .event_id(event.event_id().to_string())
            .event_type(event.event_type().to_string())
            .event_version(3)
            .maybe_sequence_number(None)
            .aggregate_id(event.aggregate_id().to_string())
            .aggregate_type(event.aggregate_type().to_string())
            .aggregate_version(event.aggregate_version())
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .occurred_at(event.occurred_at())
            .payload(payload)
            .context(serde_json::to_value(&business_context)?)
            .build();

        Ok(EventUpcasterResult::One(upgraded))
    }
}

/// V3 -> V4: rename `account.debited` to `account.withdrew` and reshape the payload.
struct AccountDebitedV3ToV4;

impl EventUpcaster for AccountDebitedV3ToV4 {
    fn applies(&self, event_type: &str, event_version: usize) -> bool {
        event_type == "account.debited" && event_version == 3
    }

    fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
        let payload = event.payload();
        let minor_units = payload
            .get("minor_units")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountDebitedV3ToV4"),
                    "v3 missing minor_units",
                )
            })?;
        let currency = payload
            .get("currency")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DomainError::upcast_failed(
                    event.event_type(),
                    event.event_version(),
                    Some("AccountDebitedV3ToV4"),
                    "v3 missing currency",
                )
            })?;

        println!(
            "  [Upcaster V3->V4] Renaming account.debited to account.withdrew ({} {})",
            minor_units, currency
        );

        let withdrew_payload = serde_json::json!({
            "Withdrew": {
                "id": event.event_id(),
                "aggregate_version": event.aggregate_version(),
                "minor_units": minor_units,
                "currency": currency,
            }
        });

        // Rebuild EventContext so the original business context (correlation/causation/actor) is preserved
        let business_context = EventContext::builder()
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .build();

        let withdrew_event = SerializedEvent::builder()
            .event_id(event.event_id().to_string())
            .event_type("account.withdrew".to_string())
            .event_version(4)
            .maybe_sequence_number(None)
            .aggregate_id(event.aggregate_id().to_string())
            .aggregate_type(event.aggregate_type().to_string())
            .aggregate_version(event.aggregate_version())
            .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
            .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
            .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
            .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
            .occurred_at(event.occurred_at())
            .payload(withdrew_payload)
            .context(serde_json::to_value(&business_context)?)
            .build();

        Ok(EventUpcasterResult::One(withdrew_event))
    }
}

// ============================================================================
// In-memory repository implementations (example only)
// ============================================================================

#[derive(Default, Clone)]
struct InMemoryEventRepository {
    events: Arc<Mutex<HashMap<String, Vec<SerializedEvent>>>>,
}

#[async_trait]
impl EventRepository for InMemoryEventRepository {
    async fn get_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
    ) -> DomainResult<Vec<SerializedEvent>> {
        let store = self.events.lock().unwrap();
        Ok(store
            .get(&aggregate_id.to_string())
            .cloned()
            .unwrap_or_default())
    }

    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        last_version: usize,
    ) -> DomainResult<Vec<SerializedEvent>> {
        let store = self.events.lock().unwrap();
        Ok(store
            .get(&aggregate_id.to_string())
            .map(|events| {
                events
                    .iter()
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

type SnapshotsMap = HashMap<(String, String), Vec<SerializedSnapshot>>;

#[derive(Clone)]
struct InMemorySnapshotRepository {
    // Snapshots stored in memory; the snapshot policy is enforced by the outer decorator.
    snapshots: Arc<Mutex<SnapshotsMap>>,
}

impl Default for InMemorySnapshotRepository {
    fn default() -> Self {
        Self {
            snapshots: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl SnapshotRepository for InMemorySnapshotRepository {
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> DomainResult<Option<SerializedSnapshot>> {
        let store = self.snapshots.lock().unwrap();
        let key = (A::TYPE.to_string(), aggregate_id.to_string());

        if let Some(snaps) = store.get(&key) {
            match version {
                Some(target) => Ok(snaps
                    .iter()
                    .filter(|s| s.aggregate_version() <= target)
                    .max_by_key(|s| s.aggregate_version())
                    .cloned()),
                None => Ok(snaps.last().cloned()),
            }
        } else {
            Ok(None)
        }
    }

    async fn save<A: Aggregate>(&self, aggregate: &A) -> DomainResult<()> {
        let snapshot = SerializedSnapshot::from_aggregate(aggregate)?;
        let mut store = self.snapshots.lock().unwrap();
        let key = (A::TYPE.to_string(), aggregate.id().to_string());
        let entry = store.entry(key).or_default();
        entry.push(snapshot);
        entry.sort_by_key(|s| s.aggregate_version());

        Ok(())
    }
}

// ============================================================================
// Generic event factories
// ============================================================================

/// Create a deposit event in any of the supported schema versions (v1-v4).
fn create_deposit(
    id: &str,
    ver: usize,
    yuan: Option<i64>,
    cents: Option<i64>,
    currency: Option<&str>,
) -> SerializedEvent {
    let eid = Ulid::new().to_string();
    let aver: usize = 0;
    let (event_type, payload) = match ver {
        1 => (
            "account.credited",
            serde_json::json!({
                "id": eid,
                "aggregate_version": aver,
                "amount": yuan.unwrap()
            }),
        ),
        2 => (
            "account.credited",
            serde_json::json!({
                "id": eid,
                "aggregate_version": aver,
                "amount": yuan.unwrap(),
                "currency": currency.unwrap()
            }),
        ),
        3 => (
            "account.credited",
            serde_json::json!({
                "id": eid,
                "aggregate_version": aver,
                "minor_units": cents.unwrap(),
                "currency": currency.unwrap()
            }),
        ),
        4 => (
            "account.deposited",
            serde_json::json!({
                "Deposited": {
                    "id": eid,
                    "aggregate_version": aver,
                    "minor_units": cents.unwrap(),
                    "currency": currency.unwrap()
                }
            }),
        ),
        _ => panic!("Unsupported version"),
    };
    let event_context = EventContext::builder()
        .maybe_correlation_id(Some(format!("cor-{id}")))
        .maybe_causation_id(Some(format!("cau-{id}")))
        .maybe_actor_type(Some("user".into()))
        .maybe_actor_id(Some("u-1".into()))
        .build();

    SerializedEvent::builder()
        .event_id(eid)
        .event_type(event_type.to_string())
        .event_version(ver)
        .maybe_sequence_number(None)
        .aggregate_id(id.to_string())
        .aggregate_type("bank_account".to_string())
        .aggregate_version(aver)
        .correlation_id(format!("cor-{id}"))
        .causation_id(format!("cau-{id}"))
        .actor_type("user".into())
        .actor_id("u-1".into())
        .occurred_at(chrono::Utc::now())
        .payload(payload)
        .context(serde_json::to_value(&event_context).expect("serialize EventContext"))
        .build()
}

/// Create a withdrawal event in any of the supported schema versions (v1-v4).
fn create_withdraw(
    id: &str,
    ver: usize,
    yuan: Option<i64>,
    cents: Option<i64>,
    currency: Option<&str>,
) -> SerializedEvent {
    let eid = Ulid::new().to_string();
    let aver: usize = 0;
    let (event_type, payload) = match ver {
        1 => (
            "account.debited",
            serde_json::json!({
                "id": eid,
                "aggregate_version": aver,
                "amount": yuan.unwrap()
            }),
        ),
        2 => (
            "account.debited",
            serde_json::json!({
                "id": eid,
                "aggregate_version": aver,
                "amount": yuan.unwrap(),
                "currency": currency.unwrap()
            }),
        ),
        3 => (
            "account.debited",
            serde_json::json!({
                "id": eid,
                "aggregate_version": aver,
                "minor_units": cents.unwrap(),
                "currency": currency.unwrap()
            }),
        ),
        4 => (
            "account.withdrew",
            serde_json::json!({
                "Withdrew": {
                    "id": eid,
                    "aggregate_version": aver,
                    "minor_units": cents.unwrap(),
                    "currency": currency.unwrap()
                }
            }),
        ),
        _ => panic!("Unsupported version"),
    };
    let event_context = EventContext::builder()
        .maybe_correlation_id(Some(format!("cor-{id}")))
        .maybe_causation_id(Some(format!("cau-{id}")))
        .maybe_actor_type(Some("user".into()))
        .maybe_actor_id(Some("u-1".into()))
        .build();

    SerializedEvent::builder()
        .event_id(eid)
        .event_type(event_type.to_string())
        .event_version(ver)
        .maybe_sequence_number(None)
        .aggregate_id(id.to_string())
        .aggregate_type("bank_account".to_string())
        .aggregate_version(aver)
        .correlation_id(format!("cor-{id}"))
        .causation_id(format!("cau-{id}"))
        .actor_type("user".into())
        .actor_id("u-1".into())
        .occurred_at(chrono::Utc::now())
        .payload(payload)
        .context(serde_json::to_value(&event_context).expect("serialize EventContext"))
        .build()
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main(flavor = "current_thread")]
async fn main() -> AnyResult<()> {
    println!("=== Event Upcasting Example ===\n");

    let account_id = "acc-001".to_string();

    // Build the upcaster chain and wrap it in an Arc so it can be shared across repositories.
    let upcaster_chain: Arc<EventUpcasterChain> = Arc::new(
        vec![
            Arc::new(AccountCreditedV1ToV2) as Arc<dyn EventUpcaster>,
            Arc::new(AccountCreditedV2ToV3) as Arc<dyn EventUpcaster>,
            Arc::new(AccountCreditedV3ToV4) as Arc<dyn EventUpcaster>,
            Arc::new(AccountDebitedV1ToV2) as Arc<dyn EventUpcaster>,
            Arc::new(AccountDebitedV2ToV3) as Arc<dyn EventUpcaster>,
            Arc::new(AccountDebitedV3ToV4) as Arc<dyn EventUpcaster>,
        ]
        .into_iter()
        .collect(),
    );

    let event_repo = Arc::new(InMemoryEventRepository::default());
    // Decorate the underlying snapshot repo with a policy so save() automatically evaluates whether to snapshot.
    let snapshot_repo = Arc::new(SnapshotRepositoryWithPolicy::new(
        Arc::new(InMemorySnapshotRepository::default()),
        SnapshotPolicy::Every(1),
    ));

    // Build a stream of historical events spanning multiple schema versions and persist it.
    println!("Historical events (mixed versions):");
    let historical_events = vec![
        create_deposit(&account_id, 1, Some(100), None, None), // v1: deposit 100 yuan
        create_withdraw(&account_id, 1, Some(30), None, None), // v1: withdraw 30 yuan
        create_deposit(&account_id, 2, Some(50), None, Some("CNY")), // v2: deposit 50 yuan
        create_withdraw(&account_id, 2, Some(20), None, Some("CNY")), // v2: withdraw 20 yuan
        create_withdraw(&account_id, 2, Some(5), None, Some("CNY")), // v2: withdraw 5 yuan
        create_deposit(&account_id, 3, None, Some(8000), Some("CNY")), // v3: deposit 80 yuan (8000 minor units)
        create_withdraw(&account_id, 3, None, Some(1000), Some("CNY")), // v3: withdraw 10 yuan (1000 minor units)
        create_deposit(&account_id, 3, None, Some(2000), Some("CNY")), // v3: deposit 20 yuan (2000 minor units)
        create_deposit(&account_id, 4, None, Some(5000), Some("CNY")), // v4: deposit 50 yuan (5000 minor units)
        create_withdraw(&account_id, 4, None, Some(3000), Some("CNY")), // v4: withdraw 30 yuan (3000 minor units)
    ];

    for (i, se) in historical_events.iter().enumerate() {
        println!("  {}. {} v{}", i + 1, se.event_type(), se.event_version());
    }
    println!();

    event_repo.save(historical_events).await?;

    // Use EventSourcedRepo to load the aggregate, automatically upcasting historical events.
    println!("Rebuilding aggregate via EventSourcedRepo:");
    let account: BankAccount =
        match EventSourcedRepo::new(event_repo.clone(), upcaster_chain.clone())
            .load(&account_id)
            .await?
        {
            Some(aggregate) => aggregate,
            None => {
                println!("  [warn] no events found in the repository");
                return Ok(());
            }
        };

    println!(
        "  [ok] upcasting complete: balance {} minor units ({:.2} yuan), version {}",
        account.balance_minor_units,
        account.balance_minor_units as f64 / 100.0,
        account.version()
    );

    // Save a snapshot, then append further events to simulate post-snapshot evolution.
    snapshot_repo.save(&account).await?;
    println!("  [save] snapshot saved (version {})", account.version());

    let incremental_events = vec![
        create_withdraw(&account_id, 2, Some(10), None, Some("CNY")), // v2: extra withdrawal of 10 yuan
        create_deposit(&account_id, 3, None, Some(1500), Some("CNY")), // v3: extra deposit of 15 yuan (1500 minor units)
    ];
    println!(
        "  [+] appended {} incremental event(s) after the snapshot",
        incremental_events.len()
    );
    event_repo.save(incremental_events).await?;

    // Use SnapshotPolicyRepo: load the latest snapshot first, then upcast and apply post-snapshot events.
    let account_after_snapshot: BankAccount = match SnapshotPolicyRepo::new(
        event_repo.clone(),
        snapshot_repo.clone(),
        upcaster_chain.clone(),
    )
    .load(&account_id)
    .await?
    {
        Some(aggregate) => aggregate,
        None => {
            println!("  [warn] aggregate not present in the snapshot repository");
            return Ok(());
        }
    };

    println!(
        "  [reload] loaded via SnapshotPolicyRepo: balance {} minor units ({:.2} yuan), version {}\n",
        account_after_snapshot.balance_minor_units,
        account_after_snapshot.balance_minor_units as f64 / 100.0,
        account_after_snapshot.version()
    );

    // Demonstrate serialization of a freshly emitted event (always in the latest schema).
    println!("New event serialization demo:");
    let new_event = BankAccountEvent::Deposited {
        id: Ulid::new().to_string(),
        aggregate_version: account_after_snapshot.version().next(),
        minor_units: 2000,
        currency: "CNY".to_string(),
    };

    let new_envelope: EventEnvelope<BankAccount> =
        EventEnvelope::new(&account_id, new_event, EventContext::default());

    let serialized = serialize_events(&[new_envelope])?;
    println!(
        "  {} v{} → SerializedEvent\n",
        serialized[0].event_type(),
        serialized[0].event_version()
    );

    // Summary
    println!("=== Application scenario summary ===");
    println!("- Repository: EventSourcedRepo::load() -> historical events upcast automatically");
    println!(
        "- Snapshot: SnapshotPolicyRepo::load() -> integrated snapshot + incremental event recovery"
    );
    println!(
        "- Storage: EventRepository::save() / serialize_events() -> new events persisted as-is"
    );
    println!(
        "\nBenefit: historical events are upcast on the fly so business code only handles the latest version."
    );

    Ok(())
}
