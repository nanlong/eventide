use std::{ops::Deref, slice::Iter, vec::IntoIter};

use chrono::{DateTime, Utc};

use super::event_envelope::EventEnvelope;
use crate::aggregate::Aggregate;

/// Chronologically ordered collection of [`EventEnvelope`]s for a single
/// aggregate instance.
///
/// `AggregateEvents` is a thin wrapper around `Vec<EventEnvelope<A>>` that
/// expresses an additional invariant: the events inside are stored in the
/// order they were produced, oldest first. That ordering makes it cheap to
/// derive audit-style information — who created the aggregate, who last
/// modified it, when each happened — without scanning the vector twice.
///
/// The type implements `IntoIterator` (both owned and borrowed) and
/// dereferences to `[EventEnvelope<A>]`, so it composes naturally with
/// iterator adapters and slice methods.
pub struct AggregateEvents<A>
where
    A: Aggregate,
{
    events: Vec<EventEnvelope<A>>,
}

impl<A> AggregateEvents<A>
where
    A: Aggregate,
{
    pub fn new(events: Vec<EventEnvelope<A>>) -> Self {
        Self { events }
    }

    /// Identifier of the actor that created the aggregate.
    ///
    /// Returns the `actor_id` recorded on the *first* event of the
    /// collection (or `None` if no actor was attached or the collection
    /// is empty).
    pub fn created_by(&self) -> Option<String> {
        self.events
            .first()
            .and_then(|e| e.context.actor_id().map(|s| s.to_string()))
    }

    /// Identifier of the actor that performed the most recent change.
    ///
    /// Returns the `actor_id` recorded on the *last* event of the
    /// collection (or `None` if no actor was attached or the collection
    /// is empty).
    pub fn last_modified_by(&self) -> Option<String> {
        self.events
            .last()
            .and_then(|e| e.context.actor_id().map(|s| s.to_string()))
    }

    /// Timestamp at which the aggregate was created.
    ///
    /// Returns the `occurred_at` of the *first* event in the collection,
    /// or `None` if the collection is empty.
    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.events.first().map(|e| *e.metadata.occurred_at())
    }

    /// Timestamp of the most recent change to the aggregate.
    ///
    /// Returns the `occurred_at` of the *last* event in the collection,
    /// or `None` if the collection is empty.
    pub fn last_modified_at(&self) -> Option<DateTime<Utc>> {
        self.events.last().map(|e| *e.metadata.occurred_at())
    }

    /// Borrows the underlying event slice without consuming `self`.
    pub fn events(&self) -> &[EventEnvelope<A>] {
        &self.events
    }

    /// Number of events held in the collection.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// `true` when no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Returns an iterator yielding references to each event in
    /// chronological order. The collection is **not** consumed; use
    /// `into_iter()` for an owning iterator.
    pub fn iter(&self) -> Iter<'_, EventEnvelope<A>> {
        self.events.iter()
    }
}

impl<A> IntoIterator for AggregateEvents<A>
where
    A: Aggregate,
{
    type Item = EventEnvelope<A>;
    type IntoIter = IntoIter<EventEnvelope<A>>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

impl<'a, A> IntoIterator for &'a AggregateEvents<A>
where
    A: Aggregate,
{
    type Item = &'a EventEnvelope<A>;
    type IntoIter = Iter<'a, EventEnvelope<A>>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}

impl<A> Deref for AggregateEvents<A>
where
    A: Aggregate,
{
    type Target = [EventEnvelope<A>];

    fn deref(&self) -> &Self::Target {
        &self.events
    }
}
