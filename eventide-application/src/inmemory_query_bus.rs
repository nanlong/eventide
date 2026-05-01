use std::{
    any::{TypeId, type_name, type_name_of_val},
    sync::Arc,
};

use async_trait::async_trait;
use dashmap::DashMap;

use crate::{
    bus_types::{BoxAnySend, HandlerFn},
    context::AppContext,
    error::AppError,
    query_bus::QueryBus,
    query_handler::QueryHandler,
};

/// 基于内存的 QueryBus 实现
/// - 通过 TypeId 注册不同 Query 对应的 Handler
/// - 以类型擦除方式调度，并在调用端进行结果还原
pub struct InMemoryQueryBus {
    // 使用 (QueryTypeId, ResultTypeId) 作为键，避免相同 Query 不同返回类型的冲突
    handlers: DashMap<(TypeId, TypeId), (&'static str, HandlerFn)>,
}

impl Default for InMemoryQueryBus {
    fn default() -> Self {
        Self {
            handlers: DashMap::new(),
        }
    }
}

impl InMemoryQueryBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注册查询处理器
    pub fn register<Q, R, H>(&self, handler: Arc<H>) -> Result<(), AppError>
    where
        Q: Send + 'static,
        R: Send + 'static,
        H: QueryHandler<Q, R> + Send + Sync + 'static,
    {
        let key = (TypeId::of::<Q>(), TypeId::of::<R>());

        let f: HandlerFn = {
            let handler = handler.clone();

            Arc::new(move |boxed_q, ctx| {
                let handler = handler.clone();

                Box::pin(async move {
                    match boxed_q.downcast::<Q>() {
                        Ok(q) => {
                            let dto_opt = handler.handle(ctx, *q).await?;
                            Ok(Box::new(dto_opt) as BoxAnySend)
                        }
                        Err(e) => Err(AppError::type_mismatch(
                            type_name::<Q>(),
                            type_name_of_val(&e),
                        )),
                    }
                })
            })
        };

        if self.handlers.contains_key(&key) {
            return Err(AppError::handler_already_registered(&format!(
                "{}->{}",
                type_name::<Q>(),
                type_name::<R>()
            )));
        }

        self.handlers.insert(key, (type_name::<Q>(), f));

        Ok(())
    }
}

#[async_trait]
impl QueryBus for InMemoryQueryBus {
    async fn dispatch<Q, R>(&self, ctx: &AppContext, q: Q) -> Result<R, AppError>
    where
        Q: Send + 'static,
        R: Send + 'static,
    {
        self.dispatch_impl::<Q, R>(ctx, q).await
    }
}

impl InMemoryQueryBus {
    async fn dispatch_impl<Q, R>(&self, ctx: &AppContext, q: Q) -> Result<R, AppError>
    where
        Q: Send + 'static,
        R: Send + 'static,
    {
        let key = (TypeId::of::<Q>(), TypeId::of::<R>());
        let Some((_name, f)) = self.handlers.get(&key).map(|h| h.clone()) else {
            return Err(AppError::handler_not_found(type_name::<Q>()));
        };

        let out = (f)(Box::new(q), ctx).await?;

        match out.downcast::<R>() {
            Ok(dto_opt) => Ok(*dto_opt),
            Err(e) => Err(AppError::type_mismatch(
                type_name::<R>(),
                type_name_of_val(&e),
            )),
        }
    }
}

impl InMemoryQueryBus {
    /// 获取已注册的查询类型名列表（只读视图）
    pub fn registered_queries(&self) -> Vec<&'static str> {
        self.handlers.iter().map(|e| e.value().0).collect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use eventide_domain::error::ErrorCode;
    use serde::Serialize;
    use tokio::task::JoinSet;

    use super::*;
    use crate::{error::AppError, query_handler::QueryHandler};

    #[derive(Debug)]
    struct Get;

    #[derive(Debug, Serialize)]
    struct NumDto(pub usize);

    struct GetHandler {
        counter: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl QueryHandler<Get, NumDto> for GetHandler {
        async fn handle(&self, _ctx: &AppContext, _q: Get) -> Result<NumDto, AppError> {
            let v = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
            Ok(NumDto(v))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn register_and_dispatch_works() {
        let bus = InMemoryQueryBus::new();
        let counter = Arc::new(AtomicUsize::new(0));
        bus.register::<Get, NumDto, _>(Arc::new(GetHandler {
            counter: counter.clone(),
        }))
        .unwrap();

        let ctx = AppContext::default();
        let NumDto(n) = bus.dispatch::<Get, NumDto>(&ctx, Get).await.unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn not_found_error_when_unregistered() {
        let bus = InMemoryQueryBus::new();
        let ctx = AppContext::default();
        let err = bus.dispatch::<Get, NumDto>(&ctx, Get).await.unwrap_err();
        assert_eq!(err.code(), "HANDLER_NOT_FOUND");
        assert!(err.to_string().contains("Get"));
    }

    #[derive(Debug, Serialize)]
    struct WrongDto;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn type_mismatch_error_when_result_downcast_fails() {
        let bus = InMemoryQueryBus::new();
        // 手动插入一个错误的条目：键是 Get，但闭包返回 WrongDto 而非 NumDto
        let f: HandlerFn = Arc::new(|_boxed_q, _ctx| {
            Box::pin(async move { Ok(Box::new(WrongDto) as BoxAnySend) })
        });
        bus.handlers.insert(
            (TypeId::of::<Get>(), TypeId::of::<NumDto>()),
            (type_name::<Get>(), f),
        );

        let ctx = AppContext::default();
        let err = bus.dispatch::<Get, NumDto>(&ctx, Get).await.unwrap_err();
        assert_eq!(err.code(), "TYPE_MISMATCH");
        assert!(err.to_string().contains("NumDto"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_dispatch_is_safe() {
        let bus = Arc::new(InMemoryQueryBus::new());
        let counter = Arc::new(AtomicUsize::new(0));
        bus.register::<Get, NumDto, _>(Arc::new(GetHandler {
            counter: counter.clone(),
        }))
        .unwrap();

        let mut set = JoinSet::new();
        let ctx = AppContext::default();
        for _ in 0..100 {
            let bus = bus.clone();
            let ctx = ctx.clone();
            set.spawn(async move { bus.dispatch::<Get, NumDto>(&ctx, Get).await.unwrap() });
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

    #[derive(Debug)]
    struct Get2;

    #[derive(Debug, Serialize, PartialEq, Eq)]
    struct NameDto(pub String);

    struct Get2NumHandler;
    struct Get2NameHandler;

    #[async_trait]
    impl QueryHandler<Get2, NumDto> for Get2NumHandler {
        async fn handle(&self, _ctx: &AppContext, _q: Get2) -> Result<NumDto, AppError> {
            Ok(NumDto(42))
        }
    }

    #[async_trait]
    impl QueryHandler<Get2, NameDto> for Get2NameHandler {
        async fn handle(&self, _ctx: &AppContext, _q: Get2) -> Result<NameDto, AppError> {
            Ok(NameDto("Alice".to_string()))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn same_query_with_different_results() {
        // 同一查询类型 Get2，分别注册返回 NumDto 与 NameDto 的两个处理器
        let bus = InMemoryQueryBus::new();
        bus.register::<Get2, NumDto, _>(Arc::new(Get2NumHandler))
            .unwrap();
        bus.register::<Get2, NameDto, _>(Arc::new(Get2NameHandler))
            .unwrap();

        let ctx = AppContext::default();
        let NumDto(n) = bus.dispatch::<Get2, NumDto>(&ctx, Get2).await.unwrap();
        let NameDto(name) = bus.dispatch::<Get2, NameDto>(&ctx, Get2).await.unwrap();

        assert_eq!(n, 42);
        assert_eq!(name, "Alice");
    }
}
