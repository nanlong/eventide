//! 内存版事件总线（InMemoryEventBus）
//!
//! 基于 `tokio::sync::broadcast` 实现的轻量事件总线，满足 `EventBus` 协议：
//! - `publish`：克隆并广播事件；
//! - `subscribe`：返回 `'static` 生命周期事件流，便于在 `tokio::spawn` 中使用；
//! - 典型用途：测试环境、示例与本地开发。
//!
//! 注意：该实现具备“至少一次”投递语义，若无订阅者时发送将被忽略。

use async_trait::async_trait;
use futures_core::stream::BoxStream;
use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use crate::{
    error::{DomainError, DomainResult as Result},
    eventing::EventBus,
    persist::SerializedEvent,
};

/// 简单的内存事件总线实现
#[derive(Clone)]
pub struct InMemoryEventBus {
    tx: broadcast::Sender<SerializedEvent>,
}

impl InMemoryEventBus {
    /// 创建一个内存总线，`capacity` 为广播缓冲区容量
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish(&self, event: &SerializedEvent) -> Result<()> {
        // 若当前无订阅者，broadcast 的 send 会返回错误，这里视为非致命并忽略
        let _ = self.tx.send(event.clone());
        Ok(())
    }

    async fn subscribe(&self) -> BoxStream<'static, Result<SerializedEvent>> {
        let rx = self.tx.subscribe();
        let stream =
            BroadcastStream::new(rx).map(|r| r.map_err(|e| DomainError::event_bus(e.to_string())));
        Box::pin(stream)
    }
}
