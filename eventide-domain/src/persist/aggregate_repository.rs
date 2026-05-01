//! Composable aggregate repository implementations.
//!
//! This module wires the lower-level event and snapshot abstractions into
//! ready-to-use [`AggregateRepository`] implementations. Two flavours are
//! offered out of the box:
//!
//! - [`EventSourcedRepo`] — pure event sourcing. Aggregates are rebuilt by
//!   replaying every event ever recorded for them, with the
//!   [`EventUpcasterChain`] transparently upgrading legacy schemas during
//!   replay so the in-memory aggregate always sees the latest event shape.
//! - [`SnapshotPolicyRepo`] — event sourcing combined with periodic
//!   snapshots. Reads start from the most recent snapshot and only replay
//!   the delta of events recorded after it; writes delegate to the
//!   underlying event store and then ask the snapshot repository to persist
//!   a new snapshot if the configured [`SnapshotPolicy`](super::SnapshotPolicy)
//!   says so.
//!
//! Both implementations rely on [`EventRepository`] and (optionally)
//! [`SnapshotRepository`] traits, so swapping storage backends is purely a
//! matter of choosing different adapter implementations.
//!
use crate::error::DomainError;
use crate::persist::SnapshotRepositoryWithPolicy;
use crate::{
    aggregate::Aggregate,
    domain_event::{EventContext, EventEnvelope},
    event_upcaster::EventUpcasterChain,
    persist::{EventRepository, SnapshotRepository, deserialize_events, serialize_events},
    value_object::Version,
};
use async_trait::async_trait;
use std::sync::Arc;

/// Abstraction over loading and saving a single aggregate.
///
/// `AggregateRepository` is the high-level secondary port the application
/// layer depends on. It hides whether the underlying mechanism is pure
/// event sourcing, snapshotting, or something else entirely — callers only
/// need to ask for an aggregate by id and to persist a fresh batch of
/// events together with the [`EventContext`] that produced them.
///
/// Implementations are responsible for:
///
/// - rehydrating an aggregate from its durable state in [`load`](Self::load),
///   returning `Ok(None)` when no events have been stored yet for that id;
/// - persisting a batch of newly produced [`Aggregate::Event`]s atomically
///   in [`save`](Self::save), wrapping each event in an [`EventEnvelope`]
///   with the supplied context.
///
/// A blanket implementation for `Arc<T>` is provided so concrete repositories
/// can be shared across async tasks without boilerplate.
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

/// Generic aggregate repository implementation backed by a pure event store.
///
/// `EventSourcedRepo` materialises an aggregate by replaying every event
/// recorded for it, using:
///
/// - an [`EventRepository`] to read raw [`SerializedEvent`](super::SerializedEvent)s
///   from the underlying store and to append new ones, and
/// - an [`EventUpcasterChain`] to transparently upgrade legacy events into
///   their current shape during replay.
///
/// It is the default building block for write-side repositories. Combine it
/// with [`SnapshotPolicyRepo`] when replay cost becomes a concern.
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

    /// Replays incremental events on top of an already-initialised aggregate
    /// instance.
    ///
    /// This is the workhorse used both for cold loads (where the input
    /// aggregate is a fresh `A::new(...)` at version zero) and for warm
    /// loads on top of a snapshot (where the input aggregate has been
    /// rehydrated from a [`SerializedSnapshot`](super::SerializedSnapshot)
    /// and only the events past that snapshot's version need to be
    /// applied).
    ///
    /// Returns:
    ///
    /// - `Ok(None)` when the aggregate is brand new and no events exist for
    ///   it yet — the caller has no aggregate to operate on.
    /// - `Ok(Some(aggregate))` once every applicable event has been applied,
    ///   leaving the aggregate fully up to date.
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

/// Aggregate repository that combines an event store with a snapshot store.
///
/// This implementation is the standard answer to "replaying every event is
/// getting expensive". Loading proceeds in two stages:
///
/// 1. ask the [`SnapshotRepositoryWithPolicy`] for the most recent snapshot
///    of the aggregate; if one exists, deserialize it into an aggregate
///    instance at the snapshot's version;
/// 2. delegate to an internal [`EventSourcedRepo`] to replay any events
///    recorded *after* that snapshot, bringing the aggregate fully up to
///    date.
///
/// On save the new events are appended via the event store first and the
/// snapshot store is then asked to persist a fresh snapshot. The decision
/// of whether to actually write a snapshot is delegated to the
/// [`SnapshotPolicy`](super::SnapshotPolicy) configured on
/// `SnapshotRepositoryWithPolicy`, so callers only need to choose a policy
/// that matches their throughput / replay-cost trade-off.
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
