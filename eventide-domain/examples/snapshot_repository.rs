//! # SnapshotRepository Example
//!
//! Demonstrates how to implement a [`SnapshotRepository`] to accelerate
//! event-sourced loads. Without snapshots, rehydrating an aggregate requires
//! replaying every historical event; with snapshots, we restore the latest
//! snapshot and only replay the events that occurred after it.
//!
//! ## Scenario
//!
//! An `OrderAggregate` walks through a typical lifecycle (add items, remove an
//! item, confirm, pay, ship, deliver). At several points the example saves a
//! snapshot so the difference between "snapshot + tail" loads and "full
//! replay" loads becomes visible.
//!
//! ## What this example demonstrates
//!
//! - Defining an aggregate with multi-state transitions and a long event history
//! - Implementing both [`EventRepository`] and [`SnapshotRepository`] in memory
//! - Building a generic `OrderRepository<A, E, S>` that:
//!   - Tries to load from the latest snapshot first
//!   - Falls back to a full event replay if no snapshot exists
//!   - Always applies the post-snapshot incremental events on top
//! - Wiring [`SnapshotRepositoryWithPolicy`] (a decorator) so the snapshot
//!   policy is enforced uniformly without leaking into call sites
//! - Driving the aggregate via [`AggregateRoot`] and querying both the latest
//!   snapshot and a snapshot at a specific historical version
//!
//! ## Prerequisites
//!
//! No external services. Storage is fully in-memory.
//!
//! ## Running
//!
//! ```bash
//! cargo run -p eventide-domain --example snapshot_repository
//! ```
//!
//! ## Expected output
//!
//! Logs each command applied to the order, prints snapshots as they are
//! captured, queries snapshots both for the newest version and for a specific
//! historical version, demonstrates an order cancellation, and finishes with a
//! summary of when snapshots are valuable.
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use eventide_domain::aggregate::Aggregate;
use eventide_domain::aggregate_root::AggregateRoot;
use eventide_domain::domain_event::{EventContext, EventEnvelope};
use eventide_domain::entity::Entity;
use eventide_domain::error::{DomainError, DomainResult};
use eventide_domain::event_upcaster::EventUpcasterChain;
use eventide_domain::persist::{
    AggregateRepository, EventRepository, SerializedEvent, SerializedSnapshot, SnapshotPolicy,
    SnapshotRepository, SnapshotRepositoryWithPolicy, deserialize_events, serialize_events,
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
struct OrderAggregate {
    status: OrderStatus,
    total_amount: i64,
    items: Vec<OrderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
enum OrderStatus {
    #[default]
    Draft,
    Confirmed,
    Paid,
    Shipped,
    Delivered,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OrderItem {
    product_id: String,
    quantity: u32,
    price: i64,
}

#[derive(Debug)]
enum OrderCommand {
    AddItem {
        product_id: String,
        quantity: u32,
        price: i64,
    },
    RemoveItem {
        product_id: String,
    },
    Confirm,
    Pay,
    Ship,
    Deliver,
    Cancel,
}

#[domain_event(version = 1)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum OrderEvent {
    #[event(event_type = "order.item_added")]
    ItemAdded {
        product_id: String,
        quantity: u32,
        price: i64,
    },
    #[event(event_type = "order.item_removed")]
    ItemRemoved { product_id: String },
    #[event(event_type = "order.confirmed")]
    Confirmed { confirmed_at: i64 },
    #[event(event_type = "order.paid")]
    Paid { paid_at: i64 },
    #[event(event_type = "order.shipped")]
    Shipped { shipped_at: i64 },
    #[event(event_type = "order.delivered")]
    Delivered { delivered_at: i64 },
    #[event(event_type = "order.cancelled")]
    Cancelled { cancelled_at: i64 },
}

impl Aggregate for OrderAggregate {
    const TYPE: &'static str = "order";
    type Command = OrderCommand;
    type Event = OrderEvent;
    type Error = DomainError;

    fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        match command {
            OrderCommand::AddItem {
                product_id,
                quantity,
                price,
            } => {
                if quantity == 0 {
                    return Err(DomainError::invalid_command("quantity must be positive"));
                }
                if self.status != OrderStatus::Draft {
                    return Err(DomainError::invalid_state("invalid order status"));
                }
                Ok(vec![OrderEvent::ItemAdded {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    product_id,
                    quantity,
                    price,
                }])
            }
            OrderCommand::RemoveItem { product_id } => {
                if self.status != OrderStatus::Draft {
                    return Err(DomainError::invalid_state("invalid order status"));
                }
                if !self.items.iter().any(|item| item.product_id == product_id) {
                    return Err(DomainError::not_found("item not found"));
                }
                Ok(vec![OrderEvent::ItemRemoved {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    product_id,
                }])
            }
            OrderCommand::Confirm => {
                if self.status != OrderStatus::Draft {
                    return Err(DomainError::invalid_state("invalid order status"));
                }
                Ok(vec![OrderEvent::Confirmed {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    confirmed_at: chrono::Utc::now().timestamp(),
                }])
            }
            OrderCommand::Pay => {
                if self.status != OrderStatus::Confirmed {
                    return Err(DomainError::invalid_state("invalid order status"));
                }
                Ok(vec![OrderEvent::Paid {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    paid_at: chrono::Utc::now().timestamp(),
                }])
            }
            OrderCommand::Ship => {
                if self.status != OrderStatus::Paid {
                    return Err(DomainError::invalid_state("invalid order status"));
                }
                Ok(vec![OrderEvent::Shipped {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    shipped_at: chrono::Utc::now().timestamp(),
                }])
            }
            OrderCommand::Deliver => {
                if self.status != OrderStatus::Shipped {
                    return Err(DomainError::invalid_state("invalid order status"));
                }
                Ok(vec![OrderEvent::Delivered {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    delivered_at: chrono::Utc::now().timestamp(),
                }])
            }
            OrderCommand::Cancel => {
                if matches!(self.status, OrderStatus::Delivered | OrderStatus::Cancelled) {
                    return Err(DomainError::invalid_state("invalid order status"));
                }
                Ok(vec![OrderEvent::Cancelled {
                    id: Ulid::new().to_string(),
                    aggregate_version: self.version().next(),
                    cancelled_at: chrono::Utc::now().timestamp(),
                }])
            }
        }
    }

    fn apply(&mut self, event: &Self::Event) {
        match event {
            OrderEvent::ItemAdded {
                aggregate_version,
                product_id,
                quantity,
                price,
                ..
            } => {
                self.items.push(OrderItem {
                    product_id: product_id.clone(),
                    quantity: *quantity,
                    price: *price,
                });
                self.total_amount += price * (*quantity as i64);
                self.version = *aggregate_version;
            }
            OrderEvent::ItemRemoved {
                aggregate_version,
                product_id,
                ..
            } => {
                if let Some(pos) = self.items.iter().position(|i| &i.product_id == product_id) {
                    let item = self.items.remove(pos);
                    self.total_amount -= item.price * (item.quantity as i64);
                }
                self.version = *aggregate_version;
            }
            OrderEvent::Confirmed {
                aggregate_version, ..
            } => {
                self.status = OrderStatus::Confirmed;
                self.version = *aggregate_version;
            }
            OrderEvent::Paid {
                aggregate_version, ..
            } => {
                self.status = OrderStatus::Paid;
                self.version = *aggregate_version;
            }
            OrderEvent::Shipped {
                aggregate_version, ..
            } => {
                self.status = OrderStatus::Shipped;
                self.version = *aggregate_version;
            }
            OrderEvent::Delivered {
                aggregate_version, ..
            } => {
                self.status = OrderStatus::Delivered;
                self.version = *aggregate_version;
            }
            OrderEvent::Cancelled {
                aggregate_version, ..
            } => {
                self.status = OrderStatus::Cancelled;
                self.version = *aggregate_version;
            }
        }
    }
}

// ============================================================================
// Using the SerializedSnapshot type provided by the library
// ============================================================================
// SerializedSnapshot now lives in the eventide_domain::persist module.

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
            .unwrap_or_else(Vec::new))
    }

    /// Returns events whose aggregate_version is strictly greater than `last_version`.
    /// Used after a snapshot load to apply only the incremental tail.
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
// In-memory SnapshotRepository implementation
// ============================================================================

type SnapshotsMap = HashMap<(String, String), Vec<SerializedSnapshot>>;

struct InMemorySnapshotRepository {
    // Map of (aggregate_type, aggregate_id) -> snapshots sorted by version.
    // The snapshot policy itself is enforced by the SnapshotRepositoryWithPolicy decorator.
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
    /// Fetch a snapshot. When `version` is provided, return the latest snapshot whose version
    /// is less than or equal to it. When `version` is `None`, return the most recent snapshot.
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> DomainResult<Option<SerializedSnapshot>> {
        let snapshots = self.snapshots.lock().unwrap();
        let key = (A::TYPE.to_string(), aggregate_id.to_string());

        if let Some(snaps) = snapshots.get(&key) {
            match version {
                Some(v) => {
                    // Pick the latest snapshot whose version is <= v
                    Ok(snaps
                        .iter()
                        .filter(|s| s.aggregate_version() <= v)
                        .max_by_key(|s| s.aggregate_version())
                        .cloned())
                }
                None => {
                    // Return the most recent snapshot
                    Ok(snaps.last().cloned())
                }
            }
        } else {
            Ok(None)
        }
    }

    /// Persist a snapshot of the aggregate's current state.
    async fn save<A: Aggregate>(&self, aggregate: &A) -> DomainResult<()> {
        let snapshot = SerializedSnapshot::from_aggregate(aggregate)?;
        let mut snapshots = self.snapshots.lock().unwrap();

        let key = (A::TYPE.to_string(), aggregate.id().to_string());
        let entry = snapshots.entry(key).or_default();

        // Keep the per-aggregate snapshot list sorted by version for deterministic lookups
        entry.push(snapshot);
        entry.sort_by_key(|s| s.aggregate_version());

        Ok(())
    }
}

// ============================================================================
// AggregateRepository implementation (built on top of SnapshotRepository)
// ============================================================================

struct OrderRepository<A, E, S>
where
    A: Aggregate,
    E: EventRepository,
    S: SnapshotRepository,
{
    event_repo: E,
    snapshot_repo: S,
    upcaster_chain: EventUpcasterChain,
    _phantom: std::marker::PhantomData<A>,
}

impl<A, E, S> OrderRepository<A, E, S>
where
    A: Aggregate,
    E: EventRepository,
    S: SnapshotRepository,
{
    fn new(event_repo: E, snapshot_repo: S) -> Self {
        Self {
            event_repo,
            snapshot_repo,
            upcaster_chain: EventUpcasterChain::default(),
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<E, S> AggregateRepository<OrderAggregate> for OrderRepository<OrderAggregate, E, S>
where
    E: EventRepository,
    S: SnapshotRepository,
{
    async fn load(
        &self,
        aggregate_id: &<OrderAggregate as Entity>::Id,
    ) -> Result<Option<OrderAggregate>, DomainError> {
        // Step 1: try to restore from the latest snapshot.
        if let Some(snapshot) = self
            .snapshot_repo
            .get_snapshot::<OrderAggregate>(aggregate_id, None)
            .await?
        {
            let mut order: OrderAggregate = snapshot.to_aggregate()?;
            let snapshot_version = snapshot.aggregate_version();

            // Step 2: replay only the events that happened after the snapshot.
            let incremental = self
                .event_repo
                .get_last_events::<OrderAggregate>(aggregate_id, snapshot_version)
                .await?;

            let envelopes =
                deserialize_events::<OrderAggregate>(&self.upcaster_chain, incremental)?;
            for envelope in envelopes.iter() {
                order.apply(&envelope.payload);
            }

            return Ok(Some(order));
        }

        // Step 3: no snapshot exists, rebuild from the full event history.
        let serialized = self
            .event_repo
            .get_events::<OrderAggregate>(aggregate_id)
            .await?;

        if serialized.is_empty() {
            return Ok(None);
        }

        let envelopes = deserialize_events::<OrderAggregate>(&self.upcaster_chain, serialized)?;
        let mut order = <OrderAggregate as Entity>::new(aggregate_id.clone(), Version::new());
        for envelope in envelopes.iter() {
            order.apply(&envelope.payload);
        }

        Ok(Some(order))
    }

    async fn save(
        &self,
        aggregate: &OrderAggregate,
        events: Vec<OrderEvent>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<OrderAggregate>>, DomainError> {
        let envelopes: Vec<EventEnvelope<OrderAggregate>> = events
            .into_iter()
            .map(|e| EventEnvelope::new(aggregate.id(), e, context.clone()))
            .collect();

        let serialized = serialize_events(&envelopes)?;
        self.event_repo.save(serialized).await?;

        Ok(envelopes)
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> AnyResult<()> {
    let event_repo = Arc::new(InMemoryEventRepository::default());
    // Wrap the underlying snapshot repo with a policy decorator so the policy is enforced uniformly.
    let snapshot_repo = Arc::new(SnapshotRepositoryWithPolicy::new(
        Arc::new(InMemorySnapshotRepository::default()),
        SnapshotPolicy::Every(2),
    ));
    let repo = Arc::new(OrderRepository::new(
        event_repo.clone(),
        snapshot_repo.clone(),
    ));
    let root = AggregateRoot::<OrderAggregate, _>::new(repo.clone());
    let order_id = "order-001".to_string();

    println!("=== SnapshotRepository example (driven through AggregateRoot) ===\n");

    // Drive the aggregate through AggregateRoot for each command.
    println!("--- Creating an order via AggregateRoot ---");

    // Add line items.
    let items = vec![
        ("product-A", 2, 100),
        ("product-B", 1, 200),
        ("product-C", 3, 50),
    ];

    for (product_id, quantity, price) in items {
        root.execute(
            &order_id,
            vec![OrderCommand::AddItem {
                product_id: product_id.to_string(),
                quantity,
                price,
            }],
            EventContext::default(),
        )
        .await?;
        println!(
            "[ok] added item: {} x{} = {}",
            product_id,
            quantity,
            price * (quantity as i64)
        );
    }

    // Remove one of the line items.
    root.execute(
        &order_id,
        vec![OrderCommand::RemoveItem {
            product_id: "product-C".to_string(),
        }],
        EventContext::default(),
    )
    .await?;
    println!("[ok] removed item: product-C");

    // Reload the aggregate's current state and snapshot it.
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n[snapshot] saved snapshot v{}", order.version());

    // Continue advancing the order through its lifecycle.
    println!("\n--- Order state transitions ---");
    root.execute(
        &order_id,
        vec![OrderCommand::Confirm],
        EventContext::default(),
    )
    .await?;
    println!("[ok] order confirmed");

    root.execute(&order_id, vec![OrderCommand::Pay], EventContext::default())
        .await?;
    println!("[ok] order paid");

    // Capture the second snapshot.
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n[snapshot] saved snapshot v{}", order.version());

    root.execute(&order_id, vec![OrderCommand::Ship], EventContext::default())
        .await?;
    println!("[ok] order shipped");

    // Capture the third snapshot.
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n[snapshot] saved snapshot v{}", order.version());

    root.execute(
        &order_id,
        vec![OrderCommand::Deliver],
        EventContext::default(),
    )
    .await?;
    println!("[ok] order delivered");

    // Capture the fourth snapshot.
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n[snapshot] saved snapshot v{}", order.version());

    // Demonstrate snapshot queries.
    println!("\n--- Querying snapshots via SnapshotRepository ---");

    // Look up the most recent snapshot.
    if let Some(snapshot) = snapshot_repo
        .get_snapshot::<OrderAggregate>(&order_id, None)
        .await?
    {
        println!("latest snapshot: version={}", snapshot.aggregate_version());
        let restored: OrderAggregate = snapshot.to_aggregate()?;
        println!(
            "  status: {:?}, total_amount: {}, item_count: {}",
            restored.status,
            restored.total_amount,
            restored.items.len()
        );
    }

    // Look up a snapshot at a specific version (we asked for v4; the repo returns the closest <= 4).
    if let Some(snapshot) = snapshot_repo
        .get_snapshot::<OrderAggregate>(&order_id, Some(4))
        .await?
    {
        println!(
            "\nQuery snapshot at v4: actual returned version={}",
            snapshot.aggregate_version()
        );
        let restored: OrderAggregate = snapshot.to_aggregate()?;
        println!(
            "  status: {:?}, total_amount: {}, item_count: {}",
            restored.status,
            restored.total_amount,
            restored.items.len()
        );
    }

    // Reload the aggregate via the AggregateRepository (snapshot fast-path is applied transparently).
    println!("\n--- Loading aggregate via AggregateRepository (uses snapshot automatically) ---");
    let loaded = repo.load(&order_id).await?.unwrap();
    println!(
        "order_id: {}, status: {:?}, total_amount: {}, version: {}",
        loaded.id(),
        loaded.status,
        loaded.total_amount,
        loaded.version()
    );

    // Demonstrate the cancel command on a separate, freshly created order.
    println!("\n--- Cancel order demo ---");
    let order_id_2 = "order-002".to_string();
    root.execute(
        &order_id_2,
        vec![OrderCommand::AddItem {
            product_id: "product-D".to_string(),
            quantity: 1,
            price: 100,
        }],
        EventContext::default(),
    )
    .await?;
    println!("[ok] created order-002 and added an item");

    root.execute(
        &order_id_2,
        vec![OrderCommand::Cancel],
        EventContext::default(),
    )
    .await?;
    println!("[ok] cancelled order-002");

    let cancelled_order = repo.load(&order_id_2).await?.unwrap();
    println!(
        "order_id: {}, status: {:?}",
        cancelled_order.id(),
        cancelled_order.status
    );

    println!("\n--- What SnapshotRepository gives you ---");
    println!("- SnapshotRepository: snapshot storage interface");
    println!("   - Persists and retrieves aggregate snapshots");
    println!("   - Supports querying snapshots by version");
    println!("   - Optimizes event-sourced loads by avoiding full event replay");
    println!("\n- AggregateRepository integrating snapshots:");
    println!("   - load() prefers snapshots when available");
    println!("   - Restore from snapshot, then replay only post-snapshot events");
    println!("   - Transparent to callers; performance optimization is automatic");
    println!("\n- Snapshot policy:");
    println!("   - Capture a snapshot every N events (e.g. every 10 events)");
    println!("   - Order at version 6: snapshot v6 restores directly (0 events to replay)");
    println!("   - Order at version 4: restore snapshot v3 + replay 1 event");
    println!("   - No snapshot: replay all 6 events from scratch");
    println!("\n- Performance benefit:");
    println!("   - 100 events: snapshots save 90%+ replay time");
    println!("   - 1000 events: snapshots save 99%+ replay time");
    println!("   - For high-throughput aggregates, snapshots are critical");

    Ok(())
}
