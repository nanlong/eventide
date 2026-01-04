//! 事件总线（EventBus）协议
//!
//! 定义事件发布与订阅的统一抽象，支持批量发布与 'static 生命周期事件流，
//! 以便在异步运行时（如 tokio::spawn）中消费。
use async_trait::async_trait;
use futures_core::stream::BoxStream;

use crate::{error::DomainResult as Result, persist::SerializedEvent};

/// 事件总线：负责分发事件与订阅事件流
#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, event: &SerializedEvent) -> Result<()>;

    async fn publish_batch(&self, events: &[SerializedEvent]) -> Result<()> {
        for event in events {
            self.publish(event).await?;
        }
        Ok(())
    }

    /// 返回一个 'static 生命周期的事件流，便于在 tokio::spawn 中使用
    async fn subscribe(&self) -> BoxStream<'static, Result<SerializedEvent>>;
}
