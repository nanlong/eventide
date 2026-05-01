//! 事件引擎（EventEngine）
//!
//! 统一编排“投递 → 订阅 → 分发处理”的长驻任务：
//! - 周期从中继与回收器拉取事件并发布至总线；
//! - 订阅总线事件流，按处理器匹配分发并发执行；
//! - 失败标记与补偿重放；
//! - 提供关闭与等待的 `EngineHandle`。
use std::{collections::HashMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use bon::Builder;
use futures_util::{StreamExt, stream};
use tokio::{
    task::JoinHandle,
    time::{self, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;

// 导入由 bon::Builder 生成的 typestate 模块与状态转换别名
use self::event_engine_builder::{IsUnset, SetRegistry, State as BuilderState};
use super::{EventBus, EventDeliverer, EventHandler, EventReclaimer, handler::HandledEventType};
use crate::persist::SerializedEvent;

/// EventEngine：
/// - 周期性从 Deliverer/Reclaimer 拉取事件并发布到 Bus
/// - 订阅 Bus 的事件流，分发到匹配的 Handler，并发处理
#[derive(Builder)]
pub struct EventEngine {
    event_bus: Arc<dyn EventBus>,
    event_deliverer: Arc<dyn EventDeliverer>,
    event_reclaimer: Arc<dyn EventReclaimer>,
    #[builder(setters(vis = "pub(crate)"))]
    registry: HandlerRegistry,
    #[builder(default)]
    config: EventEngineConfig,
}

impl<S: BuilderState> EventEngineBuilder<S> {
    pub fn event_handlers(
        self,
        handlers: Vec<Arc<dyn EventHandler>>,
    ) -> EventEngineBuilder<SetRegistry<S>>
    where
        <S as BuilderState>::Registry: IsUnset,
    {
        self.registry(HandlerRegistry::new(handlers))
    }
}

impl EventEngine {
    /// 启动事件引擎，返回可用于关闭/等待的句柄
    ///
    /// 启动顺序：先启动 subscribe worker 并等待其完成订阅，
    /// 然后再启动 deliver/reclaim worker，避免事件丢失。
    pub fn start(self: Arc<Self>) -> EngineHandle {
        let token = CancellationToken::new();
        let mut tasks: Vec<JoinHandle<()>> = Vec::with_capacity(3);

        // 使用 oneshot channel 同步订阅完成
        let (subscribe_ready_tx, subscribe_ready_rx) = tokio::sync::oneshot::channel::<()>();

        // 1. 先启动 subscribe worker（长循环），等待订阅完成后再启动其他 worker
        tasks.push(tokio::spawn(Self::subscribe_loop_with_ready_signal(
            self.clone(),
            token.clone(),
            subscribe_ready_tx,
        )));

        // 2. 启动 deliver worker（周期任务），等待订阅完成
        {
            let bus = self.event_bus.clone();
            let deliverer = self.event_deliverer.clone();
            let marker = DelivererMarker::new(deliverer.clone());
            let interval = self.config.deliver_interval;

            tasks.push(Self::spawn_periodic_after_ready(
                token.clone(),
                interval,
                subscribe_ready_rx,
                move || {
                    let bus = bus.clone();
                    let deliverer = deliverer.clone();
                    let marker = marker.clone();
                    async move {
                        match deliverer.fetch_events().await {
                            Ok(events) => {
                                Self::publish_and_mark(&bus, &marker, events).await;
                            }
                            Err(_) => {
                                // 拉取事件失败，稍后重试
                            }
                        }
                    }
                },
            ));
        }

        // 3. 启动 reclaim worker（周期任务）
        {
            let bus = self.event_bus.clone();
            let reclaimer = self.event_reclaimer.clone();
            let marker = ReclaimerMarker::new(reclaimer.clone());
            let interval = self.config.reclaim_interval;

            tasks.push(Self::spawn_periodic(token.clone(), interval, move || {
                let bus = bus.clone();
                let reclaimer = reclaimer.clone();
                let marker = marker.clone();
                async move {
                    if let Ok(events) = reclaimer.fetch_events().await {
                        Self::publish_and_mark(&bus, &marker, events).await;
                    }
                }
            }));
        }

        EngineHandle { token, tasks }
    }

    fn spawn_periodic<F, Fut>(
        token: CancellationToken,
        interval: Duration,
        mut f: F,
    ) -> JoinHandle<()>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            let mut ticker = time::interval(interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = ticker.tick() => f().await,
                }
            }
        })
    }

    /// 等待 ready 信号后再开始周期任务
    fn spawn_periodic_after_ready<F, Fut>(
        token: CancellationToken,
        interval: Duration,
        ready_rx: tokio::sync::oneshot::Receiver<()>,
        mut f: F,
    ) -> JoinHandle<()>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(async move {
            // 等待订阅完成信号（或被取消）
            tokio::select! {
                _ = token.cancelled() => return,
                result = ready_rx => {
                    if result.is_err() {
                        // sender 已 drop，说明订阅 worker 异常退出
                        return;
                    }
                }
            }

            let mut ticker = time::interval(interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = ticker.tick() => f().await,
                }
            }
        })
    }

    async fn publish_and_mark(
        bus: &Arc<dyn EventBus>,
        marker: &impl EventBatchMarker,
        events: Vec<SerializedEvent>,
    ) {
        if events.is_empty() {
            return;
        }

        match bus.publish_batch(&events).await {
            Ok(()) => {
                let refs: Vec<&SerializedEvent> = events.iter().collect();
                marker.mark_success(&refs).await;
            }
            Err(_batch_err) => {
                for ev in &events {
                    match bus.publish(ev).await {
                        Ok(()) => {
                            marker.mark_success(&[ev]).await;
                        }
                        Err(e) => {
                            let reason = e.to_string();
                            marker.mark_failure(&[ev], &reason).await;
                        }
                    }
                }
            }
        }
    }

    /// 带 ready 信号的订阅循环
    ///
    /// 在完成订阅后发送 ready 信号，通知 deliver worker 可以开始投递事件
    async fn subscribe_loop_with_ready_signal(
        self: Arc<Self>,
        token: CancellationToken,
        ready_tx: tokio::sync::oneshot::Sender<()>,
    ) {
        let mut stream = self.event_bus.subscribe().await;
        let registry = self.registry.clone();
        let concurrency = self.config.handler_concurrency;
        let reclaimer = self.event_reclaimer.clone();

        // 订阅完成，发送 ready 信号
        let _ = ready_tx.send(());

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    break;
                }
                maybe_event = stream.next() => {
                    match maybe_event {
                        Some(Ok(event)) => {
                            let merged = registry.matching(event.event_type());
                            if merged.is_empty() { continue; }
                            let tasks = merged.into_iter();
                            let reclaimer_for_stream = reclaimer.clone();

                            stream::iter(tasks)
                                .for_each_concurrent(Some(concurrency), move |h| {
                                    let ev = event.clone();
                                    let reclaimer = reclaimer_for_stream.clone();
                                    async move {
                                        match h.handle(&ev).await {
                                            Ok(()) => {
                                                // 标记处理成功（仅更新已存在的失败/过期记录）
                                                let _ = reclaimer
                                                    .mark_handler_success(h.handler_name(), &[&ev])
                                                    .await;
                                            }
                                            Err(err) => {
                                                let _ = reclaimer
                                                    .mark_handler_failed(h.handler_name(), &[&ev], &err.to_string())
                                                    .await;
                                            }
                                        }
                                    }
                                })
                                .await;
                        }
                        None => {
                            break;
                        }
                        Some(Err(_)) => {
                            // 事件流错误，继续处理下一个
                        }
                    }
                }
            }
        }
    }
}

// 自定义 Builder 方法：接收 handlers，内部转换为 HandlerRegistry 并设置到 builder 的 registry
// 字段。 注意：受 typestate 限制，仅当 `registry` 尚未设置时可调用。
// 若已设置 `registry`，编译器会报错提示重复设置。
// 正确的做法是：链式调用一次 `event_handlers(...)` 即可。

#[derive(Clone, Default)]
pub(crate) struct HandlerRegistry {
    by_type: HashMap<String, Vec<Arc<dyn EventHandler>>>,
    all: Vec<Arc<dyn EventHandler>>,
}

impl HandlerRegistry {
    fn new(handlers: Vec<Arc<dyn EventHandler>>) -> Self {
        let mut by_type: HashMap<String, Vec<Arc<dyn EventHandler>>> = HashMap::new();
        let mut all: Vec<Arc<dyn EventHandler>> = Vec::new();

        for h in handlers {
            match h.handled_event_type() {
                HandledEventType::All => all.push(h),
                HandledEventType::One(t) => {
                    by_type.entry(t).or_default().push(h);
                }
                HandledEventType::Many(ts) => {
                    for t in ts {
                        by_type.entry(t).or_default().push(h.clone());
                    }
                }
            }
        }

        Self { by_type, all }
    }

    fn matching(&self, event_type: &str) -> Vec<Arc<dyn EventHandler>> {
        let mut merged: Vec<Arc<dyn EventHandler>> = Vec::new();
        if let Some(list) = self.by_type.get(event_type) {
            merged.extend(list.iter().cloned());
        }
        merged.extend(self.all.iter().cloned());
        merged
    }
}

#[async_trait]
trait EventBatchMarker: Send + Sync {
    async fn mark_success(&self, events: &[&SerializedEvent]);
    async fn mark_failure(&self, events: &[&SerializedEvent], reason: &str);
}

#[derive(Clone)]
struct DelivererMarker {
    inner: Arc<dyn EventDeliverer>,
}

impl DelivererMarker {
    fn new(inner: Arc<dyn EventDeliverer>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl EventBatchMarker for DelivererMarker {
    async fn mark_success(&self, events: &[&SerializedEvent]) {
        let _ = self.inner.mark_delivered(events).await;
    }

    async fn mark_failure(&self, events: &[&SerializedEvent], reason: &str) {
        let _ = self.inner.mark_failed(events, reason).await;
    }
}

#[derive(Clone)]
struct ReclaimerMarker {
    inner: Arc<dyn EventReclaimer>,
}

impl ReclaimerMarker {
    fn new(inner: Arc<dyn EventReclaimer>) -> Self {
        Self { inner }
    }
}

#[async_trait]
impl EventBatchMarker for ReclaimerMarker {
    async fn mark_success(&self, events: &[&SerializedEvent]) {
        let _ = self.inner.mark_reclaimed(events).await;
    }

    async fn mark_failure(&self, events: &[&SerializedEvent], reason: &str) {
        let _ = self.inner.mark_failed(events, reason).await;
    }
}

/// 事件引擎配置
#[derive(Clone, Copy, Debug)]
pub struct EventEngineConfig {
    /// Outbox -> Bus 的推送间隔
    pub deliver_interval: Duration,
    /// 补偿投递的间隔
    pub reclaim_interval: Duration,
    /// 单事件的处理并发（同一事件广播给多个 handler）
    pub handler_concurrency: usize,
}

impl Default for EventEngineConfig {
    fn default() -> Self {
        Self {
            deliver_interval: Duration::from_secs(10),
            reclaim_interval: Duration::from_secs(60),
            handler_concurrency: 8,
        }
    }
}

/// 引擎运行句柄：用于优雅关闭与等待任务结束
pub struct EngineHandle {
    token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}

impl EngineHandle {
    pub fn shutdown(&self) {
        self.token.cancel();
    }

    pub async fn join(mut self) {
        let tasks = std::mem::take(&mut self.tasks);

        for t in tasks {
            let _ = t.await;
        }
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use chrono::Utc;
    use futures_core::stream::BoxStream;
    use futures_util::StreamExt;
    use tokio::sync::broadcast;
    use tokio_stream::wrappers::BroadcastStream;

    use super::*;
    use crate::{
        domain_event::EventContext,
        error::{DomainError, DomainResult},
    };

    #[derive(Clone)]
    struct InMemoryBus {
        tx: broadcast::Sender<SerializedEvent>,
    }
    impl InMemoryBus {
        fn new(cap: usize) -> Self {
            let (tx, _rx) = broadcast::channel(cap);
            Self { tx }
        }
    }
    #[async_trait]
    impl EventBus for InMemoryBus {
        async fn publish(&self, event: &SerializedEvent) -> DomainResult<()> {
            let _ = self.tx.send(event.clone());
            Ok(())
        }
        async fn subscribe(&self) -> BoxStream<'static, DomainResult<SerializedEvent>> {
            let rx = self.tx.subscribe();
            Box::pin(
                BroadcastStream::new(rx)
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
    struct SpyDeliverer {
        outbox: Outbox,
        delivered: Arc<AtomicUsize>,
        failed: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl EventDeliverer for SpyDeliverer {
        async fn fetch_events(&self) -> DomainResult<Vec<SerializedEvent>> {
            Ok(self.outbox.drain())
        }
        async fn mark_delivered(&self, events: &[&SerializedEvent]) -> DomainResult<()> {
            self.delivered.fetch_add(events.len(), Ordering::Relaxed);
            Ok(())
        }
        async fn mark_failed(
            &self,
            events: &[&SerializedEvent],
            _reason: &str,
        ) -> DomainResult<()> {
            self.failed.fetch_add(events.len(), Ordering::Relaxed);
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct SpyReclaimer {
        handler_failed: Arc<AtomicUsize>,
        reclaimed: Arc<AtomicUsize>,
        stored: Arc<Mutex<Vec<SerializedEvent>>>,
    }
    #[async_trait]
    impl EventReclaimer for SpyReclaimer {
        async fn fetch_events(&self) -> DomainResult<Vec<SerializedEvent>> {
            Ok(std::mem::take(&mut *self.stored.lock().unwrap()))
        }
        async fn mark_reclaimed(&self, events: &[&SerializedEvent]) -> DomainResult<()> {
            self.reclaimed.fetch_add(events.len(), Ordering::Relaxed);
            Ok(())
        }
        async fn mark_failed(
            &self,
            _events: &[&SerializedEvent],
            _reason: &str,
        ) -> DomainResult<()> {
            Ok(())
        }
        async fn mark_handler_failed(
            &self,
            _handler_name: &str,
            events: &[&SerializedEvent],
            _reason: &str,
        ) -> DomainResult<()> {
            self.handler_failed
                .fetch_add(events.len(), Ordering::Relaxed);
            for e in events {
                self.stored.lock().unwrap().push((*e).clone());
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
    struct SpyHandler {
        name: &'static str,
        types: HandledEventType,
        fail_on: Option<&'static str>,
        handled: Arc<Mutex<usize>>,
    }
    #[async_trait]
    impl EventHandler for SpyHandler {
        async fn handle(&self, event: &SerializedEvent) -> anyhow::Result<()> {
            if let Some(bad) = self.fail_on
                && event.event_type() == bad
            {
                anyhow::bail!("simulated handler failure on event type {}", bad);
            }
            *self.handled.lock().unwrap() += 1;
            Ok(())
        }
        fn handled_event_type(&self) -> HandledEventType {
            self.types.clone()
        }
        fn handler_name(&self) -> &str {
            self.name
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
            .aggregate_id("agg-1".to_string())
            .aggregate_type("Demo".to_string())
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
    async fn engine_end_to_end_delivery_subscribe_handle_failure() {
        // 组件
        let bus = Arc::new(InMemoryBus::new(256));
        let outbox = Outbox::default();
        let deliverer = Arc::new(SpyDeliverer {
            outbox: outbox.clone(),
            ..Default::default()
        });
        let reclaimer = Arc::new(SpyReclaimer::default());
        let ok = Arc::new(SpyHandler {
            name: "ok",
            types: HandledEventType::All,
            fail_on: None,
            handled: Arc::new(Mutex::new(0)),
        });
        let fail = Arc::new(SpyHandler {
            name: "fail",
            types: HandledEventType::One("FailMe".into()),
            fail_on: Some("FailMe"),
            handled: Arc::new(Mutex::new(0)),
        });

        let engine = Arc::new(
            EventEngine::builder()
                .event_bus(bus.clone())
                .event_deliverer(deliverer.clone())
                .event_reclaimer(reclaimer.clone())
                .event_handlers(vec![ok.clone(), fail.clone()])
                .config(EventEngineConfig {
                    deliver_interval: Duration::from_millis(100),
                    reclaim_interval: Duration::from_millis(200),
                    handler_concurrency: 8,
                })
                .build(),
        );

        // 投入待投递事件
        outbox.push(mk_event("e1", "Ok"));
        outbox.push(mk_event("e2", "FailMe"));
        outbox.push(mk_event("e3", "Ok"));

        let handle = engine.start();
        // 使用 timeout + 条件轮询，减少固定 sleep 的脆弱性
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if deliverer.delivered.load(Ordering::Relaxed) == 3
                    && reclaimer.handler_failed.load(Ordering::Relaxed) >= 1
                    && *ok.handled.lock().unwrap() >= 2
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await;
        handle.shutdown();
        handle.join().await;

        // 断言：全部 3 条已标记 delivered；失败处理器至少记录 1 次失败（可能被补偿多次重投导致>1）
        assert_eq!(deliverer.delivered.load(Ordering::Relaxed), 3);
        assert!(reclaimer.handler_failed.load(Ordering::Relaxed) >= 1);
        // 至少一个处理器成功消费
        assert!(*ok.handled.lock().unwrap() >= 2);
    }
}
