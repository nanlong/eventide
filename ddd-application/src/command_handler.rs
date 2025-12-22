use crate::{context::AppContext, error::AppError};
use async_trait::async_trait;

/// 命令处理器（Command Handler）
///
/// - 处理具体类型的命令，执行业务用例并产生领域变化；
/// - 建议仅做应用编排：参数校验、调用领域服务/仓储、发布事件等。
#[async_trait]
pub trait CommandHandler<C, R>: Send + Sync {
    /// 处理命令，返回是否执行成功
    async fn handle(&self, ctx: &AppContext, cmd: C) -> Result<R, AppError>;
}
