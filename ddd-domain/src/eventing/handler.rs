//! 事件处理器（EventHandler）
//!
//! 定义消费某类/多类/全部事件的处理逻辑与元信息（名称、订阅类型）。
use async_trait::async_trait;

use crate::persist::SerializedEvent;

#[derive(Clone, Debug)]
pub enum HandledEventType {
    One(String),
    Many(Vec<String>),
    All,
}

/// 事件处理器：处理某一类型的事件
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// 处理器名称（用于失败标记与审计）
    fn handler_name(&self) -> &str;
    /// 返回该处理器支持的事件类型
    fn handled_event_type(&self) -> HandledEventType;
    /// 处理事件
    async fn handle(&self, event: &SerializedEvent) -> anyhow::Result<()>;
}
