/// Eventing 引擎（内存版）示例
/// 展示 Outbox -> Bus -> Handlers -> Reclaimer 的闭环，以及 handler 失败后的补偿重投
use anyhow::Result as AnyResult;
use chrono::Utc;
use ddd_domain::domain_event::EventContext;
use ddd_domain::error::DomainResult;
use ddd_domain::eventing::{
    EventDeliverer, EventEngine, EventEngineConfig, EventHandler, EventReclaimer, HandledEventType,
    InMemoryEventBus,
};
use ddd_domain::persist::SerializedEvent;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

// ============================================================================
// 内存 Outbox（EventDeliverer）
// ============================================================================

#[derive(Clone, Default)]
struct InMemoryOutbox {
    inner: Arc<Mutex<Vec<SerializedEvent>>>,
}

impl InMemoryOutbox {
    fn push(&self, ev: SerializedEvent) {
        self.inner.lock().unwrap().push(ev);
    }

    fn drain(&self) -> Vec<SerializedEvent> {
        let mut g = self.inner.lock().unwrap();
        std::mem::take(&mut *g)
    }
}

#[derive(Clone)]
struct InMemoryDeliverer {
    outbox: InMemoryOutbox,
}

#[async_trait::async_trait]
impl EventDeliverer for InMemoryDeliverer {
    async fn fetch_events(&self) -> DomainResult<Vec<SerializedEvent>> {
        Ok(self.outbox.drain())
    }

    async fn mark_delivered(&self, _events: &[&SerializedEvent]) -> DomainResult<()> {
        Ok(())
    }

    async fn mark_failed(&self, _events: &[&SerializedEvent], _reason: &str) -> DomainResult<()> {
        // In-memory 简化：失败事件暂不重试
        Ok(())
    }
}

// ============================================================================
// 内存失败存储（EventReclaimer）
// ============================================================================

#[derive(Clone, Default)]
struct InMemoryFailures {
    inner: Arc<Mutex<Vec<SerializedEvent>>>,
}

impl InMemoryFailures {
    fn push(&self, ev: SerializedEvent) {
        self.inner.lock().unwrap().push(ev);
    }
    fn drain(&self) -> Vec<SerializedEvent> {
        let mut g = self.inner.lock().unwrap();
        std::mem::take(&mut *g)
    }
}

#[derive(Clone)]
struct InMemoryReclaimer {
    failures: InMemoryFailures,
}

#[async_trait::async_trait]
impl EventReclaimer for InMemoryReclaimer {
    async fn fetch_events(&self) -> DomainResult<Vec<SerializedEvent>> {
        Ok(self.failures.drain())
    }

    async fn mark_reclaimed(&self, _events: &[&SerializedEvent]) -> DomainResult<()> {
        Ok(())
    }

    async fn mark_failed(&self, events: &[&SerializedEvent], _reason: &str) -> DomainResult<()> {
        for ev in events {
            self.failures.push((*ev).clone());
        }
        Ok(())
    }

    async fn mark_handler_failed(
        &self,
        _handler_name: &str,
        events: &[&SerializedEvent],
        _reason: &str,
    ) -> DomainResult<()> {
        for ev in events {
            self.failures.push((*ev).clone());
        }
        Ok(())
    }

    async fn mark_handler_success(
        &self,
        _handler_name: &str,
        _events: &[&SerializedEvent],
    ) -> DomainResult<()> {
        // In-memory 简化：成功时无需记录
        Ok(())
    }
}

// ============================================================================
// 示例处理器（EventHandler）
// ============================================================================

#[derive(Clone)]
struct PrintHandler {
    name: &'static str,
    types: HandledEventType,
    fail_on: Option<&'static str>,
}

#[async_trait::async_trait]
impl EventHandler for PrintHandler {
    async fn handle(&self, event: &SerializedEvent) -> anyhow::Result<()> {
        if let Some(bad) = self.fail_on
            && event.event_type() == bad
        {
            anyhow::bail!("{} failed on {}", self.name, bad);
        }
        println!(
            "handler={} type={} aggregate={} payload={}",
            self.name,
            event.event_type(),
            event.aggregate_id(),
            event.payload()
        );
        Ok(())
    }

    fn handled_event_type(&self) -> HandledEventType {
        self.types.clone()
    }
    fn handler_name(&self) -> &str {
        self.name
    }
}

// ============================================================================
// 工具函数
// ============================================================================

fn mk_event(id: &str, ty: &str) -> SerializedEvent {
    // 使用 EventContext 作为上下文格式
    let event_context = EventContext::builder()
        .maybe_correlation_id(Some(format!("cor-{id}")))
        .maybe_causation_id(Some(format!("cau-{id}")))
        .maybe_actor_type(Some("user".to_string()))
        .maybe_actor_id(Some("u-1".to_string()))
        .build();

    SerializedEvent::builder()
        .event_id(id.to_string())
        .event_type(ty.to_string())
        .event_version(1)
        .aggregate_id(format!("agg-{}", id))
        .aggregate_type("DemoAggregate".to_string())
        .aggregate_version(1)
        // 顶层冗余字段与 EventContext 保持一致，便于查询
        .correlation_id(format!("cor-{id}"))
        .causation_id(format!("cau-{id}"))
        .actor_type("user".to_string())
        .actor_id("u-1".to_string())
        .occurred_at(Utc::now())
        .payload(serde_json::json!({"id": id, "version": 1, "value": 42}))
        .context(serde_json::to_value(&event_context).expect("serialize EventContext"))
        .build()
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> AnyResult<()> {
    println!("=== Eventing 引擎（内存版）示例 ===\n");
    // Bus
    let bus = Arc::new(InMemoryEventBus::new(1024));

    // Outbox & Deliverer
    let outbox = InMemoryOutbox::default();
    outbox.push(mk_event("e1", "UserCreated"));
    outbox.push(mk_event("e2", "UserDeleted"));
    let deliverer = Arc::new(InMemoryDeliverer {
        outbox: outbox.clone(),
    });

    // Reclaimer
    let reclaimer = Arc::new(InMemoryReclaimer {
        failures: InMemoryFailures::default(),
    });

    // Handlers
    let handlers: Vec<Arc<dyn EventHandler>> = vec![
        Arc::new(PrintHandler {
            name: "printer",
            types: HandledEventType::All,
            fail_on: None,
        }),
        Arc::new(PrintHandler {
            name: "sometimes_fail",
            types: HandledEventType::One("UserDeleted".to_string()),
            fail_on: Some("UserDeleted"),
        }),
    ];

    // Engine
    let engine = Arc::new(
        EventEngine::builder()
            .event_bus(bus)
            .event_handlers(handlers)
            .event_deliverer(deliverer)
            .event_reclaimer(reclaimer)
            .config(EventEngineConfig {
                deliver_interval: Duration::from_millis(200),
                reclaim_interval: Duration::from_millis(400),
                handler_concurrency: 8,
            })
            .build(),
    );

    let handle = engine.start();
    println!("✅ 引擎已启动");

    // 演示在运行中继续塞入事件
    tokio::time::sleep(Duration::from_millis(300)).await;
    outbox.push(mk_event("e3", "UserCreated"));
    outbox.push(mk_event("e4", "UserDeleted"));
    println!("✅ 追加事件: e3(UserCreated), e4(UserDeleted)");

    tokio::time::sleep(Duration::from_secs(2)).await;
    handle.shutdown();
    handle.join().await;
    println!("\n✅ 优雅关闭完成");
    Ok(())
}
