//! 聚合根编排器（AggregateRoot）
//!
//! 封装从“加载聚合 → 执行命令 → 应用事件 → 持久化事件”的标准流程，
//! 以仓储实现（`AggregateRepository`）为依赖，便于在应用层直接调用。
use std::marker::PhantomData;

use crate::{
    aggregate::Aggregate,
    domain_event::{EventContext, EventEnvelope},
    persist::AggregateRepository,
    value_object::Version,
};

/// 面向应用层的聚合根编排器。
///
/// - `A`：聚合类型（实现 `Aggregate`）
/// - `R`：聚合仓储（实现 `AggregateRepository<A>`）
pub struct AggregateRoot<A, R>
where
    A: Aggregate,
    R: AggregateRepository<A>,
{
    repo: R,
    _marker: PhantomData<A>,
}

impl<A, R> AggregateRoot<A, R>
where
    A: Aggregate,
    R: AggregateRepository<A>,
{
    /// 创建编排器实例
    pub fn new(repo: R) -> Self {
        Self {
            repo,
            _marker: PhantomData,
        }
    }

    /// 执行聚合命令：
    /// 1. 若未持久化则创建新聚合；
    /// 2. 执行命令得到新事件；
    /// 3. 应用事件到聚合状态；
    /// 4. 调用仓储持久化并返回事件信封。
    pub async fn execute(
        &self,
        aggregate_id: &A::Id,
        commands: Vec<A::Command>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<A>>, A::Error> {
        // 如果不存在则创建新的聚合实例
        let mut aggregate = self
            .load(aggregate_id)
            .await?
            .unwrap_or_else(|| A::new(aggregate_id.clone(), Version::new()));

        // 执行命令，获取事件
        let events = commands.into_iter().try_fold(Vec::new(), |mut acc, cmd| {
            let mut events = aggregate.execute(cmd)?;

            for event in &events {
                aggregate.apply(event);
            }

            acc.append(&mut events);

            Ok(acc)
        })?;

        if events.is_empty() {
            return Ok(vec![]);
        }

        // 保存聚合状态和未提交的事件
        self.repo.save(&aggregate, events, context).await
    }

    /// 加载聚合实例
    pub async fn load(&self, aggregate_id: &A::Id) -> Result<Option<A>, A::Error> {
        self.repo.load(aggregate_id).await
    }
}
