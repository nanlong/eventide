//! 事件回收器（EventReclaimer）
//!
//! 负责拉取失败/超时/漏投递事件进行补偿，并细化到处理器粒度的失败标记，
//! 以便区分具体 handler 的异常。
use async_trait::async_trait;

use crate::{error::DomainResult as Result, persist::SerializedEvent};

/// 事件回收器：拉取失败/超时/漏投递事件进行补偿
#[async_trait]
pub trait EventReclaimer: Send + Sync {
    /// 拉取需要补偿的事件
    async fn fetch_events(&self) -> Result<Vec<SerializedEvent>>;

    /// 标记事件已补偿投递成功
    async fn mark_reclaimed(&self, events: &[&SerializedEvent]) -> Result<()>;

    /// 标记事件补偿投递失败
    async fn mark_failed(&self, events: &[&SerializedEvent], reason: &str) -> Result<()>;

    /// 指定处理器粒度的失败标记（区分具体 handler 出错）
    async fn mark_handler_failed(
        &self,
        handler_name: &str,
        events: &[&SerializedEvent],
        reason: &str,
    ) -> Result<()>;

    /// 指定处理器粒度的成功标记（若存在失败/过期记录则更新为成功）
    async fn mark_handler_success(
        &self,
        handler_name: &str,
        events: &[&SerializedEvent],
    ) -> Result<()>;
}
