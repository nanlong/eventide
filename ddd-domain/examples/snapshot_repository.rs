use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// SnapshotRepository 示例
/// 演示如何实现快照仓储接口，用于优化事件溯源性能
/// 快照机制可以避免重放大量历史事件，直接从快照恢复聚合状态
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
        AggregateRepository, EventRepository, SerializedEvent, SerializedSnapshot, SnapshotPolicy,
        SnapshotRepository, SnapshotRepositoryWithPolicy, deserialize_events, serialize_events,
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
// 使用库提供的 SerializedSnapshot
// ============================================================================
// SerializedSnapshot 现在由 ddd_domain::persist 模块提供

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
            .unwrap_or_else(Vec::new))
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
// 内存快照仓储实现
// ============================================================================

type SnapshotsMap = HashMap<(String, String), Vec<SerializedSnapshot>>;

struct InMemorySnapshotRepository {
    // (aggregate_type, aggregate_id) -> 快照列表（按版本排序），策略由装饰器控制
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
    /// 获取快照，如果指定版本则获取该版本或之前的最新快照
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
                    // 找到版本 <= v 的最新快照
                    Ok(snaps
                        .iter()
                        .filter(|s| s.aggregate_version() <= v)
                        .max_by_key(|s| s.aggregate_version())
                        .cloned())
                }
                None => {
                    // 返回最新快照
                    Ok(snaps.last().cloned())
                }
            }
        } else {
            Ok(None)
        }
    }

    /// 保存快照
    async fn save<A: Aggregate>(&self, aggregate: &A) -> DomainResult<()> {
        let snapshot = SerializedSnapshot::from_aggregate(aggregate)?;
        let mut snapshots = self.snapshots.lock().unwrap();

        let key = (A::TYPE.to_string(), aggregate.id().to_string());
        let entry = snapshots.entry(key).or_default();

        // 保持版本排序
        entry.push(snapshot);
        entry.sort_by_key(|s| s.aggregate_version());

        Ok(())
    }
}

// ============================================================================
// AggregateRepository 实现（整合 SnapshotRepository）
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
        // 1. 尝试从快照加载
        if let Some(snapshot) = self
            .snapshot_repo
            .get_snapshot::<OrderAggregate>(aggregate_id, None)
            .await?
        {
            let mut order: OrderAggregate = snapshot.to_aggregate()?;
            let snapshot_version = snapshot.aggregate_version();

            // 2. 加载快照之后的增量事件
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

        // 3. 没有快照，从事件重建
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
    // 通过装饰器统一评估快照策略，避免上层自行判断
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

    println!("=== SnapshotRepository 示例（使用 AggregateRoot）===\n");

    // 使用 AggregateRoot 执行命令
    println!("--- 使用 AggregateRoot 创建订单 ---");

    // 添加商品
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
            "✅ 添加商品: {} x{} = {}",
            product_id,
            quantity,
            price * (quantity as i64)
        );
    }

    // 移除一个商品
    root.execute(
        &order_id,
        vec![OrderCommand::RemoveItem {
            product_id: "product-C".to_string(),
        }],
        EventContext::default(),
    )
    .await?;
    println!("✅ 移除商品: product-C");

    // 加载当前状态并保存快照
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n📸 保存快照 v{}", order.version());

    // 继续订单流程
    println!("\n--- 订单状态流转 ---");
    root.execute(
        &order_id,
        vec![OrderCommand::Confirm],
        EventContext::default(),
    )
    .await?;
    println!("✅ 确认订单");

    root.execute(&order_id, vec![OrderCommand::Pay], EventContext::default())
        .await?;
    println!("✅ 支付订单");

    // 保存第二个快照
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n📸 保存快照 v{}", order.version());

    root.execute(&order_id, vec![OrderCommand::Ship], EventContext::default())
        .await?;
    println!("✅ 发货订单");

    // 保存第三个快照
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n📸 保存快照 v{}", order.version());

    root.execute(
        &order_id,
        vec![OrderCommand::Deliver],
        EventContext::default(),
    )
    .await?;
    println!("✅ 签收订单");

    // 保存第四个快照
    let order = repo.load(&order_id).await?.unwrap();
    snapshot_repo.save(&order).await?;
    println!("\n📸 保存快照 v{}", order.version());

    // 演示快照查询
    println!("\n--- 使用 SnapshotRepository 查询快照 ---");

    // 获取最新快照
    if let Some(snapshot) = snapshot_repo
        .get_snapshot::<OrderAggregate>(&order_id, None)
        .await?
    {
        println!("最新快照: 版本={}", snapshot.aggregate_version());
        let restored: OrderAggregate = snapshot.to_aggregate()?;
        println!(
            "  状态: {:?}, 总金额: {}, 商品数: {}",
            restored.status,
            restored.total_amount,
            restored.items.len()
        );
    }

    // 获取指定版本的快照
    if let Some(snapshot) = snapshot_repo
        .get_snapshot::<OrderAggregate>(&order_id, Some(4))
        .await?
    {
        println!(
            "\n查询版本4的快照: 实际返回版本={}",
            snapshot.aggregate_version()
        );
        let restored: OrderAggregate = snapshot.to_aggregate()?;
        println!(
            "  状态: {:?}, 总金额: {}, 商品数: {}",
            restored.status,
            restored.total_amount,
            restored.items.len()
        );
    }

    // 使用 AggregateRepository 重新加载（利用快照优化）
    println!("\n--- 使用 AggregateRepository 加载聚合（自动使用快照）---");
    let loaded = repo.load(&order_id).await?.unwrap();
    println!(
        "订单ID: {}, 状态: {:?}, 总金额: {}, 版本: {}",
        loaded.id(),
        loaded.status,
        loaded.total_amount,
        loaded.version()
    );

    // 演示取消订单命令（创建新订单）
    println!("\n--- 演示取消订单 ---");
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
    println!("✅ 创建订单 order-002 并添加商品");

    root.execute(
        &order_id_2,
        vec![OrderCommand::Cancel],
        EventContext::default(),
    )
    .await?;
    println!("✅ 取消订单 order-002");

    let cancelled_order = repo.load(&order_id_2).await?.unwrap();
    println!(
        "订单ID: {}, 状态: {:?}",
        cancelled_order.id(),
        cancelled_order.status
    );

    println!("\n--- SnapshotRepository 的作用 ---");
    println!("✅ SnapshotRepository: 快照存储接口");
    println!("   - 提供聚合快照的持久化和查询能力");
    println!("   - 支持按版本查询快照");
    println!("   - 优化事件溯源性能，避免重放大量事件");
    println!("\n✅ AggregateRepository 整合快照:");
    println!("   - load_aggregate 时优先使用快照");
    println!("   - 从快照恢复 + 重放增量事件");
    println!("   - 对上层透明，自动优化性能");
    println!("\n✅ 快照策略:");
    println!("   • 每隔N个事件创建快照（如每10个事件）");
    println!("   • 版本6的订单: 快照v6直接恢复（0个事件重放）");
    println!("   • 版本4的订单: 快照v3恢复 + 重放1个事件");
    println!("   • 没有快照: 需要重放所有6个事件");
    println!("\n✅ 性能优势:");
    println!("   • 事件数100: 快照节省90%+重放时间");
    println!("   • 事件数1000: 快照节省99%+重放时间");
    println!("   • 高频聚合: 快照是性能的关键");

    Ok(())
}
