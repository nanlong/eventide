//! Event repository contract.
//!
//! Defines the secondary port the domain layer uses to read and write the
//! durable event log of an aggregate. The trait deliberately exposes only
//! three primitives — read all events for an aggregate, read events past a
//! given version, append a batch of events — because anything richer would
//! constrain the choice of backend and leak persistence concerns into the
//! domain.
//!
//! The companion extension trait [`EventRepositoryExt`] adds an
//! upcasting-aware helper that combines a read with the
//! [`EventUpcasterChain`] to produce a ready-to-consume
//! [`AggregateEvents`] collection — useful for projections and read-model
//! builders that want to walk an aggregate's history end to end.
//!
use crate::{
    aggregate::Aggregate,
    domain_event::AggregateEvents,
    error::DomainResult as Result,
    event_upcaster::EventUpcasterChain,
    persist::{SerializedEvent, deserialize_events},
};
use async_trait::async_trait;
use std::sync::Arc;

/// Append-only event log abstraction for a single aggregate type.
///
/// Implementations are typically backed by a relational table, an event
/// store product, or an in-memory vector for tests. They are responsible
/// for atomicity of [`save`](Self::save) (every event in a batch must be
/// persisted together or not at all) and for enforcing optimistic
/// concurrency via the `aggregate_version` field of
/// [`SerializedEvent`].
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

/// Convenience extension of [`EventRepository`] for upcasting-aware reads.
///
/// `EventRepositoryExt` is automatically implemented for every type that
/// implements [`EventRepository`] (via a blanket impl below), so callers can
/// use it without any additional setup.
#[async_trait]
pub trait EventRepositoryExt: EventRepository {
    /// Loads every event for the aggregate, runs them through the supplied
    /// [`EventUpcasterChain`] to bring legacy schemas up to the current
    /// shape, and returns them packaged as an [`AggregateEvents`] collection
    /// that can be iterated, queried for `created_by`/`created_at`, etc.
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
