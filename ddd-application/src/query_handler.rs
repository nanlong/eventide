use async_trait::async_trait;

use crate::{context::AppContext, error::AppError};

/// 查询处理器（Query Handler）
///
/// - 处理具体类型的查询，返回结果对象/类型；
/// - 建议只读，不修改领域状态，可直接访问读模型或投影。
#[async_trait]
pub trait QueryHandler<Q, R>: Send + Sync {
    /// 处理查询并返回结果对象/类型
    async fn handle(&self, ctx: &AppContext, q: Q) -> Result<R, AppError>;
}
