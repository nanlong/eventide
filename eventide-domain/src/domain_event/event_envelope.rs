use chrono::Utc;

use super::{event_context::EventContext, metadata::Metadata};
use crate::aggregate::Aggregate;

/// Envelope around a domain event that bundles the payload with technical
/// metadata and business context.
///
/// In an event-sourced system the raw [`Aggregate::Event`] alone is rarely
/// enough — consumers also need to know which aggregate the event belongs
/// to, when it occurred, and what business request triggered it. The
/// envelope solves that by carrying three orthogonal concerns side by side:
///
/// - [`metadata`](Self::metadata) — technical facts about the event itself
///   (aggregate identity, occurrence timestamp).
/// - [`payload`](Self::payload) — the domain event variant produced by the
///   aggregate, expressing *what* happened.
/// - [`context`](Self::context) — propagation/tracing information linking
///   this event back to a correlation chain, an actor, etc.
///
/// `EventEnvelope` is what flows between the domain and persistence layers
/// (it round-trips through [`SerializedEvent`](crate::persist::SerializedEvent))
/// and is also the natural unit handed to event handlers.
#[derive(Debug, Clone)]
pub struct EventEnvelope<A>
where
    A: Aggregate,
{
    pub metadata: Metadata,
    pub payload: A::Event,
    pub context: EventContext,
}

impl<A> EventEnvelope<A>
where
    A: Aggregate,
{
    pub fn new(aggregate_id: &A::Id, payload: A::Event, context: EventContext) -> Self {
        let metadata = Metadata::builder()
            .aggregate_id(aggregate_id.to_string())
            .aggregate_type(A::TYPE.to_string())
            .occurred_at(Utc::now())
            .build();

        Self {
            metadata,
            payload,
            context,
        }
    }
}
