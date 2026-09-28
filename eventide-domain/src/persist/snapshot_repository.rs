//! Snapshot repository contract and write policy.
//!
//! Snapshots are an optional optimisation that lets event-sourced aggregates
//! avoid replaying their entire history on every load. This module defines:
//!
//! - [`SnapshotRepository`] — the secondary port for reading and writing
//!   serialized aggregate snapshots, kept intentionally narrow so any
//!   key/value store, table, or document database can implement it.
//! - [`SnapshotPolicy`] — a tiny enum that decides *when* a snapshot should
//!   actually be written (never, or every N versions).
//! - [`SnapshotRepositoryWithPolicy`] — a decorator that combines the two:
//!   reads pass straight through to the wrapped repository, writes are
//!   gated by the policy.
//!
use crate::{aggregate::Aggregate, error::DomainResult as Result, persist::SerializedSnapshot};
use async_trait::async_trait;
use std::sync::Arc;

/// Storage abstraction for aggregate snapshots.
///
/// Implementations persist and retrieve [`SerializedSnapshot`] values keyed
/// by aggregate id (and optionally by version, for time-travel reads).
///
/// The contract is intentionally minimal:
///
/// - [`get_snapshot`](Self::get_snapshot) returns the latest snapshot for
///   an aggregate when `version` is `None`, or a specific version when
///   `Some(v)` is supplied. Implementations are free to return `None` when
///   no snapshot exists.
/// - [`save`](Self::save) writes a fresh snapshot of the supplied aggregate.
///   Whether the call ultimately stores anything is left to the caller —
///   wrap the implementation in [`SnapshotRepositoryWithPolicy`] to gate it
///   on a [`SnapshotPolicy`].
#[async_trait]
pub trait SnapshotRepository: Send + Sync {
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> Result<Option<SerializedSnapshot>>;

    async fn save<A: Aggregate>(&self, aggregate: &A) -> Result<()>;
}

#[async_trait]
impl<T> SnapshotRepository for Arc<T>
where
    T: SnapshotRepository + ?Sized,
{
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> Result<Option<SerializedSnapshot>> {
        (**self).get_snapshot::<A>(aggregate_id, version).await
    }

    async fn save<A: Aggregate>(&self, aggregate: &A) -> Result<()> {
        (**self).save::<A>(aggregate).await
    }
}

/// Decision rule for when to actually persist a snapshot.
///
/// `SnapshotPolicy::Never` disables snapshotting entirely (useful in tests
/// or for aggregates with very short histories), while
/// `SnapshotPolicy::Every(n)` writes a snapshot every time the aggregate
/// version is a strictly positive multiple of `n`. Values of `n < 1` are
/// silently clamped to `1` to avoid degenerate behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPolicy {
    Never,
    Every(usize),
}

impl SnapshotPolicy {
    /// Returns `true` when, given the supplied aggregate `version`, this
    /// policy considers a snapshot due.
    pub fn should_snapshot(&self, version: usize) -> bool {
        match self {
            SnapshotPolicy::Never => false,
            SnapshotPolicy::Every(interval) => {
                let interval = (*interval).max(1);
                version > 0 && version.is_multiple_of(interval)
            }
        }
    }
}

/// Decorator over a [`SnapshotRepository`] that gates writes on a
/// [`SnapshotPolicy`].
///
/// Reads are forwarded unconditionally to the inner repository. Writes are
/// only forwarded when [`SnapshotPolicy::should_snapshot`] returns `true`
/// for the aggregate's current version, otherwise [`save`](Self::save)
/// returns `Ok(())` without touching storage.
pub struct SnapshotRepositoryWithPolicy<R> {
    inner: R,
    policy: SnapshotPolicy,
}

impl<R> SnapshotRepositoryWithPolicy<R> {
    pub fn new(inner: R, policy: SnapshotPolicy) -> Self {
        Self { inner, policy }
    }
}

#[async_trait]
impl<R> SnapshotRepository for SnapshotRepositoryWithPolicy<R>
where
    R: SnapshotRepository + Send + Sync,
{
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> Result<Option<SerializedSnapshot>> {
        self.inner.get_snapshot::<A>(aggregate_id, version).await
    }

    async fn save<A: Aggregate>(&self, aggregate: &A) -> Result<()> {
        if !self.policy.should_snapshot(aggregate.version().value()) {
            return Ok(());
        }

        self.inner.save::<A>(aggregate).await
    }
}
