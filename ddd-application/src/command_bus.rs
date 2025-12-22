use crate::{context::AppContext, error::AppError};
use async_trait::async_trait;

/// 命令总线（Command Bus）
///
/// - 负责根据命令的具体类型路由到对应的处理器；
/// - 框架可提供不同实现（如进程内、消息队列等）；
/// - 该 trait 带有泛型方法，通常以具体实现类型注入使用。
#[async_trait]
pub trait CommandBus: Send + Sync {
    /// 分发命令到对应处理器
    ///
    /// - `ctx`：应用上下文（链路追踪、幂等键等）
    /// - `cmd`：具体命令实例
    /// - 返回：命令处理结果
    async fn dispatch<C, R>(&self, ctx: &AppContext, cmd: C) -> Result<R, AppError>
    where
        C: Send + 'static,
        R: Send + 'static;
}
