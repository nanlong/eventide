//! 命令总线和查询总线的公共类型定义

use std::{any::Any, future::Future, pin::Pin, sync::Arc};

use crate::{context::AppContext, error::AppError};

/// 类型擦除的 Send 类型
pub type BoxAnySend = Box<dyn Any + Send>;

/// 处理器 Future 类型
pub type HandlerFuture<'a> =
    Pin<Box<dyn Future<Output = Result<BoxAnySend, AppError>> + Send + 'a>>;

/// 处理器函数类型
pub type HandlerFn =
    Arc<dyn for<'a> Fn(BoxAnySend, &'a AppContext) -> HandlerFuture<'a> + Send + Sync>;
