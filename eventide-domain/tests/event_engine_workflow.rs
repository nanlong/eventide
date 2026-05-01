#![cfg(feature = "eventing")]
use std::{
    collections::HashSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use anyhow::Result as AnyResult;
use chrono::Utc;
use eventide_domain::{
    domain_event::EventContext,
    error::{DomainError, DomainResult},
    eventing::{
        EventBus, EventDeliverer, EventEngine, EventEngineConfig, EventHandler, EventReclaimer,
        HandledEventType,
    },
    persist::SerializedEvent,
};
use futures_core::stream::BoxStream;
use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

#[derive(Clone)]
struct Bus {
    tx: broadcast::Sender<SerializedEvent>,
}
impl Bus {
    fn new(cap: usize) -> Self {
        let (tx, _rx) = broadcast::channel(cap);
        Self { tx }
    }
}
#[async_trait::async_trait]
impl EventBus for Bus {
    async fn publish(&self, event: &SerializedEvent) -> DomainResult<()> {
        let _ = self.tx.send(event.clone());
        Ok(())
    }
    async fn subscribe(&self) -> BoxStream<'static, DomainResult<SerializedEvent>> {
        Box::pin(
            BroadcastStream::new(self.tx.subscribe())
                .map(|r| r.map_err(|e| DomainError::event_bus(e.to_string()))),
        )
    }
}

#[derive(Clone, Default)]
struct Outbox {
    inner: Arc<Mutex<Vec<SerializedEvent>>>,
}
impl Outbox {
    fn push(&self, ev: SerializedEvent) {
        self.inner.lock().unwrap().push(ev);
    }
    fn drain(&self) -> Vec<SerializedEvent> {
        std::mem::take(&mut *self.inner.lock().unwrap())
    }
}

#[derive(Clone, Default)]
struct Deliverer {
    outbox: Outbox,
    delivered: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl EventDeliverer for Deliverer {
    async fn fetch_events(&self) -> DomainResult<Vec<SerializedEvent>> {
        Ok(self.outbox.drain())
    }
    async fn mark_delivered(&self, events: &[&SerializedEvent]) -> DomainResult<()> {
        self.delivered.fetch_add(events.len(), Ordering::Relaxed);
        Ok(())
    }
    async fn mark_failed(&self, _events: &[&SerializedEvent], _reason: &str) -> DomainResult<()> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct Reclaimer {
    failures: Arc<Mutex<Vec<SerializedEvent>>>,
    reclaimed: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl EventReclaimer for Reclaimer {
    async fn fetch_events(&self) -> DomainResult<Vec<SerializedEvent>> {
        Ok(std::mem::take(&mut *self.failures.lock().unwrap()))
    }
    async fn mark_reclaimed(&self, events: &[&SerializedEvent]) -> DomainResult<()> {
        self.reclaimed.fetch_add(events.len(), Ordering::Relaxed);
        Ok(())
    }
    async fn mark_failed(&self, _events: &[&SerializedEvent], _reason: &str) -> DomainResult<()> {
        Ok(())
    }
    async fn mark_handler_failed(
        &self,
        _handler_name: &str,
        events: &[&SerializedEvent],
        _reason: &str,
    ) -> DomainResult<()> {
        for e in events {
            self.failures.lock().unwrap().push((*e).clone());
        }
        Ok(())
    }
    async fn mark_handler_success(
        &self,
        _handler_name: &str,
        _events: &[&SerializedEvent],
    ) -> DomainResult<()> {
        Ok(())
    }
}

#[derive(Clone)]
struct FlakyHandler {
    seen: Arc<Mutex<HashSet<String>>>,
}
#[async_trait::async_trait]
impl EventHandler for FlakyHandler {
    async fn handle(&self, event: &SerializedEvent) -> anyhow::Result<()> {
        if event.event_type() == "Bad" {
            let mut g = self.seen.lock().unwrap();
            if !g.contains(event.event_id()) {
                g.insert(event.event_id().to_string());
                anyhow::bail!("first time fails");
            }
        }
        Ok(())
    }
    fn handled_event_type(&self) -> HandledEventType {
        HandledEventType::All
    }
    fn handler_name(&self) -> &str {
        "flaky"
    }
}

fn mk_event(id: &str, ty: &str) -> SerializedEvent {
    let event_context = EventContext::builder()
        .maybe_correlation_id(Some(format!("cor-{id}")))
        .maybe_causation_id(Some(format!("cau-{id}")))
        .maybe_actor_type(Some("user".into()))
        .maybe_actor_id(Some("u-1".into()))
        .build();

    SerializedEvent::builder()
        .event_id(id.to_string())
        .event_type(ty.to_string())
        .event_version(1)
        .aggregate_id("agg".into())
        .aggregate_type("T".into())
        .aggregate_version(1)
        .correlation_id(format!("cor-{id}"))
        .causation_id(format!("cau-{id}"))
        .actor_type("user".into())
        .actor_id("u-1".into())
        .occurred_at(Utc::now())
        .payload(serde_json::json!({"id": id}))
        .context(serde_json::to_value(&event_context).expect("serialize EventContext"))
        .build()
}

#[tokio::test(flavor = "multi_thread")]
async fn event_engine_full_workflow() -> AnyResult<()> {
    let bus = Arc::new(Bus::new(1024));
    let outbox = Outbox::default();
    let deliverer = Arc::new(Deliverer {
        outbox: outbox.clone(),
        ..Default::default()
    });
    let reclaimer = Arc::new(Reclaimer::default());
    let handler = Arc::new(FlakyHandler {
        seen: Arc::new(Mutex::new(HashSet::new())),
    });

    let engine = Arc::new(
        EventEngine::builder()
            .event_bus(bus)
            .event_deliverer(deliverer.clone())
            .event_reclaimer(reclaimer.clone())
            .event_handlers(vec![handler])
            .config(EventEngineConfig {
                deliver_interval: Duration::from_millis(100),
                reclaim_interval: Duration::from_millis(150),
                handler_concurrency: 4,
            })
            .build(),
    );

    // 初始 outbox +1 普通 +1 失败一次的事件
    outbox.push(mk_event("e-ok", "Ok"));
    outbox.push(mk_event("e-bad", "Bad"));

    let handle = engine.start();
    // 使用 timeout + 轮询条件，减少固定 sleep 带来的不确定性
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if deliverer.delivered.load(Ordering::Relaxed) >= 2
                && reclaimer.reclaimed.load(Ordering::Relaxed) >= 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    handle.shutdown();
    handle.join().await;

    // 两个 outbox 事件均已 delivered；Bad 在第一次失败后进入 reclaimer，经 reclaim 再次投递后被
    // handler 成功处理并标记 reclaimed
    assert!(deliverer.delivered.load(Ordering::Relaxed) >= 2);
    assert!(reclaimer.reclaimed.load(Ordering::Relaxed) >= 1);
    Ok(())
}
