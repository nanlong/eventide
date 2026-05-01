//! Aggregate-root orchestrator ([`AggregateRoot`]).
//!
//! Encapsulates the canonical "load aggregate -> execute command -> apply
//! events -> persist events" pipeline so that application-layer code does not
//! have to wire the steps together by hand. The orchestrator depends only on
//! the [`AggregateRepository`] trait, so the underlying storage strategy
//! (pure event sourcing, snapshot-aware, etc.) can be swapped at the
//! composition root without touching call sites.
//!
use crate::{
    aggregate::Aggregate,
    domain_event::{EventContext, EventEnvelope},
    persist::AggregateRepository,
    value_object::Version,
};
use std::marker::PhantomData;

/// Application-facing orchestrator for an aggregate's command lifecycle.
///
/// Type parameters:
///
/// - `A`: the aggregate type (must implement [`Aggregate`]).
/// - `R`: the repository type that knows how to load and save `A` (must
///   implement [`AggregateRepository<A>`]).
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
    /// Build a new orchestrator backed by the given repository.
    pub fn new(repo: R) -> Self {
        Self {
            repo,
            _marker: PhantomData,
        }
    }

    /// Execute a sequence of commands against the aggregate identified by
    /// `aggregate_id`.
    ///
    /// The pipeline performed for each call is:
    ///
    /// 1. Load the aggregate from the repository, or build a fresh instance
    ///    at version zero if it has never been persisted.
    /// 2. Run every command through [`Aggregate::execute`] and accumulate the
    ///    produced events.
    /// 3. Apply each event back onto the in-memory aggregate so that
    ///    subsequent commands in the batch see consistent state.
    /// 4. Hand the aggregate, the new events, and the propagation
    ///    [`EventContext`] off to the repository for atomic persistence,
    ///    returning the persisted [`EventEnvelope`]s.
    ///
    /// If the command list yields no events the orchestrator short-circuits
    /// and returns an empty list without touching the repository.
    pub async fn execute(
        &self,
        aggregate_id: &A::Id,
        commands: Vec<A::Command>,
        context: EventContext,
    ) -> Result<Vec<EventEnvelope<A>>, A::Error> {
        // Materialise the aggregate, falling back to a brand-new instance
        // when it has never been persisted before.
        let mut aggregate = self
            .load(aggregate_id)
            .await?
            .unwrap_or_else(|| A::new(aggregate_id.clone(), Version::new()));

        // Drive each command through `execute` and immediately replay the
        // resulting events so the next command sees the updated state.
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

        // Persist the new state and uncommitted events atomically.
        self.repo.save(&aggregate, events, context).await
    }

    /// Load the current state of the aggregate, returning `None` if it has
    /// never been persisted.
    pub async fn load(&self, aggregate_id: &A::Id) -> Result<Option<A>, A::Error> {
        self.repo.load(aggregate_id).await
    }
}
