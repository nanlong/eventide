//! 事件仓储协议
//!
//! 定义按聚合读取全部或增量事件与批量保存的接口；
//! 并提供扩展方法将读取结果与上抬链组合为 `AggregateEvents`。
use std::sync::Arc;

use async_trait::async_trait;

use crate::{
    aggregate::Aggregate,
    domain_event::AggregateEvents,
    error::DomainResult as Result,
    event_upcaster::EventUpcasterChain,
    persist::{SerializedEvent, deserialize_events},
};

#[async_trait]
pub trait EventRepository: Send + Sync {
    async fn get_events<A: Aggregate>(&self, aggregate_id: &A::Id) -> Result<Vec<SerializedEvent>>;

    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        last_version: usize,
    ) -> Result<Vec<SerializedEvent>>;

    async fn save(&self, events: Vec<SerializedEvent>) -> Result<()>;
}

#[async_trait]
pub trait EventRepositoryExt: EventRepository {
    /// 拉取并上抬（Upcast）指定聚合的全部事件，返回 `AggregateEvents`
    async fn get_aggregate_events_upcasted<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        upcaster_chain: &EventUpcasterChain,
    ) -> Result<AggregateEvents<A>> {
        let serialized = self.get_events::<A>(aggregate_id).await?;
        let envelopes = deserialize_events::<A>(upcaster_chain, serialized)?;
        Ok(AggregateEvents::new(envelopes))
    }
}

#[async_trait]
impl<T> EventRepository for Arc<T>
where
    T: EventRepository + ?Sized,
{
    async fn get_events<A: Aggregate>(&self, aggregate_id: &A::Id) -> Result<Vec<SerializedEvent>> {
        (**self).get_events::<A>(aggregate_id).await
    }

    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        last_version: usize,
    ) -> Result<Vec<SerializedEvent>> {
        (**self)
            .get_last_events::<A>(aggregate_id, last_version)
            .await
    }

    async fn save(&self, events: Vec<SerializedEvent>) -> Result<()> {
        (**self).save(events).await
    }
}

#[async_trait]
impl<T> EventRepositoryExt for T where T: EventRepository + ?Sized {}
