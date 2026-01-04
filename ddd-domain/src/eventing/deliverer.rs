//! 事件中继（EventDeliverer）
//!
//! 负责从本地存储（如 Outbox 表）批量取出待投递事件，并在发布后
//! 标记成功或失败，便于进行重试与审计。
use async_trait::async_trait;

use crate::{error::DomainResult as Result, persist::SerializedEvent};

/// 事件中继：从本地存储/Outbox 拉取待发送的事件
#[async_trait]
pub trait EventDeliverer: Send + Sync {
    /// 拉取待投递的事件（Outbox）
    async fn fetch_events(&self) -> Result<Vec<SerializedEvent>>;

    /// 将事件标记为已成功投递
    async fn mark_delivered(&self, events: &[&SerializedEvent]) -> Result<()>;

    /// 将事件标记为投递失败（可用于增加 attempts、设置 next_retry_at 等）
    async fn mark_failed(&self, events: &[&SerializedEvent], reason: &str) -> Result<()>;
}
