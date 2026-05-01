use std::{
    any::{TypeId, type_name, type_name_of_val},
    sync::Arc,
};

use async_trait::async_trait;
use dashmap::DashMap;

use crate::{
    bus_types::{BoxAnySend, HandlerFn},
    command_bus::CommandBus,
    command_handler::CommandHandler,
    context::AppContext,
    error::AppError,
};

/// 基于内存的 CommandBus 实现
/// - 通过 (CommandTypeId, ResultTypeId) 注册不同 Command 对应的 Handler
/// - 运行时以类型擦除（Any）方式进行调度，并在调用端进行结果还原
pub struct InMemoryCommandBus {
    handlers: DashMap<(TypeId, TypeId), (&'static str, HandlerFn)>,
}

impl Default for InMemoryCommandBus {
    fn default() -> Self {
        Self {
            handlers: DashMap::new(),
        }
    }
}

impl InMemoryCommandBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册命令处理器
    pub fn register<C, R, H>(&self, handler: Arc<H>) -> Result<(), AppError>
    where
        C: Send + 'static,
        R: Send + 'static,
        H: CommandHandler<C, R> + Send + Sync + 'static,
    {
        let key = (TypeId::of::<C>(), TypeId::of::<R>());

        let f: HandlerFn = {
            let handler = handler.clone();

            Arc::new(move |boxed_cmd, ctx| {
                let handler = handler.clone();

                Box::pin(async move {
                    match boxed_cmd.downcast::<C>() {
                        Ok(cmd) => {
                            let result = handler.handle(ctx, *cmd).await?;
                            Ok(Box::new(result) as BoxAnySend)
                        }
                        Err(e) => {
                            let found = type_name_of_val(&e);
                            Err(AppError::type_mismatch(type_name::<C>(), found))
                        }
                    }
                })
            })
        };

        if self.handlers.contains_key(&key) {
            return Err(AppError::handler_already_registered(&format!(
                "{}->{}",
                type_name::<C>(),
                type_name::<R>()
            )));
        }

        self.handlers.insert(key, (type_name::<C>(), f));

        Ok(())
    }
}

#[async_trait]
impl CommandBus for InMemoryCommandBus {
    async fn dispatch<C, R>(&self, ctx: &AppContext, cmd: C) -> Result<R, AppError>
    where
        C: Send + 'static,
        R: Send + 'static,
    {
        self.dispatch_impl::<C, R>(ctx, cmd).await
    }
}

impl InMemoryCommandBus {
    async fn dispatch_impl<C, R>(&self, ctx: &AppContext, cmd: C) -> Result<R, AppError>
    where
        C: Send + 'static,
        R: Send + 'static,
    {
        let key = (TypeId::of::<C>(), TypeId::of::<R>());
        let Some((_name, f)) = self.handlers.get(&key).map(|h| h.clone()) else {
            return Err(AppError::handler_not_found(type_name::<C>()));
        };

        let out = (f)(Box::new(cmd), ctx).await?;

        match out.downcast::<R>() {
            Ok(result) => Ok(*result),
            Err(e) => Err(AppError::type_mismatch(
                type_name::<R>(),
                type_name_of_val(&e),
            )),
        }
    }
}

impl InMemoryCommandBus {
    /// 获取已注册的命令类型名列表（只读视图）
    pub fn registered_commands(&self) -> Vec<&'static str> {
        self.handlers.iter().map(|e| e.value().0).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use eventide_domain::error::ErrorCode;
    use tokio::task::JoinSet;

    use super::*;
    use crate::{command_handler::CommandHandler, error::AppError};

    #[derive(Debug)]
    struct Add;

    #[derive(Debug, PartialEq, Eq)]
    struct AddResult(pub usize);

    struct AddHandler {
        counter: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl CommandHandler<Add, AddResult> for AddHandler {
        async fn handle(&self, _ctx: &AppContext, _cmd: Add) -> Result<AddResult, AppError> {
            let v = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(AddResult(v))
        }
    }

    // 测试注册和分发命令正常工作
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn register_and_dispatch_works() {
        let bus = InMemoryCommandBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        bus.register::<Add, AddResult, _>(Arc::new(AddHandler {
            counter: counter.clone(),
        }))
        .unwrap();

        let ctx = AppContext::default();
        let AddResult(n) = bus.dispatch::<Add, AddResult>(&ctx, Add).await.unwrap();
        assert_eq!(n, 1);
    }

    // 测试未注册处理器时返回 HANDLER_NOT_FOUND 错误
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn not_found_error_when_unregistered() {
        let bus = InMemoryCommandBus::new();
        let ctx = AppContext::default();
        let err = bus.dispatch::<Add, AddResult>(&ctx, Add).await.unwrap_err();
        assert_eq!(err.code(), "HANDLER_NOT_FOUND");
        assert!(err.to_string().contains("Add"));
    }

    #[derive(Debug)]
    struct WrongResult;

    // 测试结果类型不匹配时返回 TYPE_MISMATCH 错误
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn type_mismatch_error_when_result_downcast_fails() {
        let bus = InMemoryCommandBus::new();
        // 手动插入一个错误的条目：键是 (Add, AddResult)，但闭包返回 WrongResult
        let f: HandlerFn = Arc::new(|_boxed_cmd, _ctx| {
            Box::pin(async move { Ok(Box::new(WrongResult) as BoxAnySend) })
        });
        bus.handlers.insert(
            (TypeId::of::<Add>(), TypeId::of::<AddResult>()),
            (type_name::<Add>(), f),
        );

        let ctx = AppContext::default();
        let err = bus.dispatch::<Add, AddResult>(&ctx, Add).await.unwrap_err();
        assert_eq!(err.code(), "TYPE_MISMATCH");
        assert!(err.to_string().contains("AddResult"));
    }

    // 测试并发分发命令是安全的
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_dispatch_is_safe() {
        let bus = Arc::new(InMemoryCommandBus::new());
        let counter = Arc::new(AtomicUsize::new(0));
        bus.register::<Add, AddResult, _>(Arc::new(AddHandler {
            counter: counter.clone(),
        }))
        .unwrap();

        let mut set = JoinSet::new();
        let ctx = AppContext::default();
        for _ in 0..100 {
            let bus = bus.clone();
            let ctx = ctx.clone();
            set.spawn(async move { bus.dispatch::<Add, AddResult>(&ctx, Add).await.unwrap() });
        }

        let mut results = Vec::new();
        while let Some(res) = set.join_next().await {
            results.push(res.unwrap().0);
        }
        results.sort_unstable();
        assert_eq!(results.len(), 100);
        assert_eq!(results[0], 1);
        assert_eq!(results[99], 100);
    }

    // 返回 () 的命令处理器
    #[derive(Debug)]
    struct VoidCmd;

    struct VoidHandler;

    #[async_trait]
    impl CommandHandler<VoidCmd, ()> for VoidHandler {
        async fn handle(&self, _ctx: &AppContext, _cmd: VoidCmd) -> Result<(), AppError> {
            Ok(())
        }
    }

    // 测试返回 () 的命令处理器正常工作
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn void_result_works() {
        let bus = InMemoryCommandBus::new();
        bus.register::<VoidCmd, (), _>(Arc::new(VoidHandler))
            .unwrap();

        let ctx = AppContext::default();
        bus.dispatch::<VoidCmd, ()>(&ctx, VoidCmd).await.unwrap();
    }
}
