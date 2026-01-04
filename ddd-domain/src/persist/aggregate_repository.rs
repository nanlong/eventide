//! 聚合仓储组合实现
//!
//! 基于事件溯源（Event Store）与快照（Snapshot）的通用聚合仓储实现，
//! 通过事件上抬链在重建过程中完成旧事件兼容。
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    aggregate::Aggregate,
    domain_event::{EventContext, EventEnvelope},
    error::DomainError,
    event_upcaster::EventUpcasterChain,
    persist::{
        EventRepository, SnapshotRepository, SnapshotRepositoryWithPolicy, deserialize_events,
        serialize_events,
    },
    value_object::Version,
};

#[async_trait]
pub trait AggregateRepository<A>: Send + Sync
where
    A: Aggregate,
{
    async fn load(&self, aggregate_id: &A::Id) -> Result<Option<A>, A::Error>;

    async fn save(
        &self,
        aggregate: &A,
        events: Vec<A::Event>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<A>>, A::Error>;
}

#[async_trait]
impl<A, T> AggregateRepository<A> for Arc<T>
where
    A: Aggregate,
    T: AggregateRepository<A> + ?Sized,
{
    async fn load(&self, aggregate_id: &A::Id) -> Result<Option<A>, A::Error> {
        (**self).load(aggregate_id).await
    }

    async fn save(
        &self,
        aggregate: &A,
        events: Vec<A::Event>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<A>>, A::Error> {
        (**self).save(aggregate, events, context).await
    }
}

/// 基于事件存储的通用聚合仓储实现。
/// - 使用 `EventRepository` 读取/保存事件
/// - 在重建聚合时通过 `EventUpcasterChain` 对事件进行上抬
pub struct EventSourcedRepo<E> {
    event_repo: Arc<E>,
    upcaster_chain: Arc<EventUpcasterChain>,
}

impl<E> EventSourcedRepo<E>
where
    E: EventRepository,
{
    pub fn new(event_repo: Arc<E>, upcaster_chain: Arc<EventUpcasterChain>) -> Self {
        Self {
            event_repo,
            upcaster_chain,
        }
    }

    pub async fn replay<A>(&self, mut aggregate: A) -> Result<Option<A>, DomainError>
    where
        A: Aggregate,
    {
        let serialized = self
            .event_repo
            .get_last_events::<A>(aggregate.id(), aggregate.version().value())
            .await?;

        if serialized.is_empty() && aggregate.version().is_new() {
            return Ok(None);
        }

        if serialized.is_empty() {
            return Ok(Some(aggregate));
        }

        let envelopes = deserialize_events::<A>(&self.upcaster_chain, serialized)?;

        for env in envelopes {
            aggregate.apply(&env.payload);
        }

        Ok(Some(aggregate))
    }
}

#[async_trait]
impl<A, E> AggregateRepository<A> for EventSourcedRepo<E>
where
    A: Aggregate,
    E: EventRepository + Send + Sync,
    A::Error: From<DomainError> + Send + Sync,
{
    async fn load(&self, aggregate_id: &A::Id) -> Result<Option<A>, A::Error> {
        let aggregate = self
            .replay(A::new(aggregate_id.clone(), Version::new()))
            .await
            .map_err(A::Error::from)?;

        Ok(aggregate)
    }

    async fn save(
        &self,
        aggregate: &A,
        events: Vec<A::Event>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<A>>, A::Error> {
        let envelopes: Vec<EventEnvelope<A>> = events
            .into_iter()
            .map(|e| EventEnvelope::new(aggregate.id(), e, context.clone()))
            .collect();

        if envelopes.is_empty() {
            return Ok(envelopes);
        }

        let serialized = serialize_events(&envelopes).map_err(A::Error::from)?;

        self.event_repo
            .save(serialized)
            .await
            .map_err(A::Error::from)?;

        Ok(envelopes)
    }
}

/// 基于事件存储 + 快照 的通用聚合仓储实现。
/// - 优先使用 `SnapshotRepository` 恢复最近快照
/// - 然后加载快照版本之后的增量事件并上抬（Upcast）重放
pub struct SnapshotPolicyRepo<E, S>
where
    E: EventRepository,
    S: SnapshotRepository,
{
    event_repo: Arc<E>,
    snapshot_repo: Arc<SnapshotRepositoryWithPolicy<S>>,
    upcaster_chain: Arc<EventUpcasterChain>,
}

impl<E, S> SnapshotPolicyRepo<E, S>
where
    E: EventRepository,
    S: SnapshotRepository,
{
    pub fn new(
        event_repo: Arc<E>,
        snapshot_repo: Arc<SnapshotRepositoryWithPolicy<S>>,
        upcaster_chain: Arc<EventUpcasterChain>,
    ) -> Self {
        Self {
            event_repo,
            snapshot_repo,
            upcaster_chain,
        }
    }
}

#[async_trait]
impl<A, E, S> AggregateRepository<A> for SnapshotPolicyRepo<E, S>
where
    A: Aggregate,
    E: EventRepository + Send + Sync,
    S: SnapshotRepository + Send + Sync,
    A::Error: From<DomainError> + Send + Sync,
{
    async fn load(&self, aggregate_id: &A::Id) -> Result<Option<A>, A::Error> {
        let event_sourced_repo = EventSourcedRepo::new(
            Arc::clone(&self.event_repo),
            Arc::clone(&self.upcaster_chain),
        );

        if let Some(snapshot) = self
            .snapshot_repo
            .get_snapshot::<A>(aggregate_id, None)
            .await?
        {
            let aggregate = snapshot.to_aggregate::<A>()?;
            let aggregate = event_sourced_repo.replay(aggregate).await?;

            return Ok(aggregate);
        }

        let aggregate: Option<A> = <EventSourcedRepo<E> as AggregateRepository<A>>::load(
            &event_sourced_repo,
            aggregate_id,
        )
        .await?;

        Ok(aggregate)
    }

    async fn save(
        &self,
        aggregate: &A,
        events: Vec<A::Event>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<A>>, A::Error> {
        let event_sourced_repo = EventSourcedRepo::new(
            Arc::clone(&self.event_repo),
            Arc::clone(&self.upcaster_chain),
        );

        let envelopes = event_sourced_repo.save(aggregate, events, context).await?;

        self.snapshot_repo
            .save(aggregate)
            .await
            .map_err(A::Error::from)?;

        Ok(envelopes)
    }
}
