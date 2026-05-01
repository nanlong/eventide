//! Persistence-layer model for events (`SerializedEvent`).
//!
//! Defines the canonical shape an event takes when it crosses the
//! persistence boundary, the bidirectional conversion to and from
//! [`EventEnvelope`], and the batch helpers `serialize_events` /
//! `deserialize_events` that integrate with the
//! [`EventUpcasterChain`] so legacy on-disk events are transparently
//! upgraded on read.
//!
//! `SerializedEvent` flattens the structured [`EventEnvelope`] into a row of
//! primitive types and JSON values: every field is either an indexable
//! scalar (timestamps, ids, versions, the type tag) or an opaque
//! `serde_json::Value` payload. That layout maps directly onto a typical
//! event-store table and avoids forcing the storage adapter to know
//! anything about the concrete domain event types.
//!
use crate::{
    aggregate::Aggregate,
    domain_event::{DomainEvent, EventContext, EventEnvelope, Metadata},
    error::{DomainError, DomainResult},
    event_upcaster::EventUpcasterChain,
};
use bon::Builder;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Storage-layer representation of a single domain event.
///
/// `SerializedEvent` is the wire format used between the domain layer and
/// any persistence backend. It carries everything the store needs to:
///
/// - identify the event (`event_id`, `event_type`, `event_version`),
/// - place it on the global timeline (`sequence_number`, `occurred_at`) and
///   on its aggregate's timeline (`aggregate_id`, `aggregate_type`,
///   `aggregate_version`),
/// - reconstruct propagation context for tracing and audit
///   (`correlation_id`, `causation_id`, `actor_type`, `actor_id`,
///   `context`), and
/// - rebuild the typed payload via [`serde_json`] (`payload`).
///
/// Construct instances either via the generated [`SerializedEvent::builder`]
/// (manual construction in tests or adapters) or via the
/// `TryFrom<&EventEnvelope<A>>` impl below (the typical production path).
#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct SerializedEvent {
    /// Globally unique identifier of this event instance.
    event_id: String,
    /// Stable type discriminator used to dispatch upcasters and handlers.
    event_type: String,
    /// Schema version of `payload` for this `event_type`.
    event_version: usize,
    /// Global sequence number assigned by the storage layer once the event
    /// has been persisted; `None` for events that have not been written yet.
    sequence_number: Option<i64>,
    /// Identifier of the aggregate instance the event belongs to.
    aggregate_id: String,
    /// Discriminator for the aggregate type (`Aggregate::TYPE`).
    aggregate_type: String,
    /// Aggregate version *after* this event has been applied. Used by the
    /// storage layer for optimistic concurrency control.
    aggregate_version: usize,
    /// Identifier shared by every event produced for the same business
    /// request, propagated end to end.
    correlation_id: Option<String>,
    /// Identifier of the immediate cause of this event (typically the
    /// `event_id` of the source event when one event reactively produces
    /// another).
    causation_id: Option<String>,
    /// Type of the principal that triggered the event (for example
    /// `"user"`, `"system"`).
    actor_type: Option<String>,
    /// Stable identifier of the principal that triggered the event.
    actor_id: Option<String>,
    /// Wall-clock timestamp at which the event occurred.
    occurred_at: DateTime<Utc>,
    /// JSON-encoded domain event payload (`A::Event`).
    payload: Value,
    /// JSON-encoded copy of [`EventContext`]. Stored alongside the
    /// hoisted scalar fields above so adapters can run ad-hoc queries on
    /// either representation.
    context: Value,
}

impl SerializedEvent {
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    pub fn event_version(&self) -> usize {
        self.event_version
    }

    pub fn sequence_number(&self) -> Option<i64> {
        self.sequence_number
    }

    pub fn aggregate_id(&self) -> &str {
        &self.aggregate_id
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub fn aggregate_version(&self) -> usize {
        self.aggregate_version
    }

    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    pub fn actor_type(&self) -> Option<&str> {
        self.actor_type.as_deref()
    }

    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    pub fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    pub fn payload(&self) -> &Value {
        &self.payload
    }

    pub fn context(&self) -> &Value {
        &self.context
    }
}

impl<A> TryFrom<&EventEnvelope<A>> for SerializedEvent
where
    A: Aggregate,
{
    type Error = serde_json::Error;

    fn try_from(envelope: &EventEnvelope<A>) -> Result<Self, Self::Error> {
        Ok(SerializedEvent {
            event_id: envelope.payload.event_id().to_string(),
            event_type: envelope.payload.event_type().to_string(),
            event_version: envelope.payload.event_version(),
            sequence_number: None,
            aggregate_id: envelope.metadata.aggregate_id().to_string(),
            aggregate_type: envelope.metadata.aggregate_type().to_string(),
            aggregate_version: envelope.payload.aggregate_version().value(),
            correlation_id: envelope.context.correlation_id().map(|s| s.to_string()),
            causation_id: envelope.context.causation_id().map(|s| s.to_string()),
            actor_type: envelope.context.actor_type().map(|s| s.to_string()),
            actor_id: envelope.context.actor_id().map(|s| s.to_string()),
            occurred_at: *envelope.metadata.occurred_at(),
            payload: serde_json::to_value(&envelope.payload)?,
            context: serde_json::to_value(&envelope.context)?,
        })
    }
}

impl<A> TryFrom<&SerializedEvent> for EventEnvelope<A>
where
    A: Aggregate,
{
    type Error = serde_json::Error;

    fn try_from(value: &SerializedEvent) -> Result<Self, Self::Error> {
        let metadata = Metadata::builder()
            .aggregate_id(value.aggregate_id.clone())
            .aggregate_type(value.aggregate_type.clone())
            .occurred_at(value.occurred_at)
            .build();

        let payload: A::Event = serde_json::from_value(value.payload.clone())?;

        let context: EventContext = serde_json::from_value(value.context.clone())?;

        Ok(EventEnvelope {
            metadata,
            payload,
            context,
        })
    }
}

/// Serializes a slice of in-memory event envelopes into their
/// persistence-layer representation.
///
/// This is the inverse of [`deserialize_events`] (modulo upcasting) and is
/// typically used inside [`AggregateRepository::save`](super::AggregateRepository::save)
/// implementations just before handing the batch off to an
/// [`EventRepository`](super::EventRepository).
///
/// # Errors
///
/// Returns a [`DomainError`] when any envelope's payload or context cannot
/// be encoded into JSON.
pub fn serialize_events<A>(events: &[EventEnvelope<A>]) -> DomainResult<Vec<SerializedEvent>>
where
    A: Aggregate,
{
    let events = events
        .iter()
        .map(SerializedEvent::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(events)
}

/// Deserializes a batch of stored events back into typed
/// [`EventEnvelope`]s, transparently running each event through the
/// supplied [`EventUpcasterChain`] first.
///
/// The upcaster chain is applied *before* JSON deserialization, so each
/// chain step sees the raw [`SerializedEvent`] and can rewrite the
/// `event_type`, bump the `event_version`, drop the event entirely, or
/// fan one event out into many. This keeps the deserialization step
/// schema-free from the caller's perspective: by the time JSON is parsed
/// into `A::Event`, the payload is guaranteed to match the current Rust
/// type definition.
///
/// # Errors
///
/// Returns a [`DomainError`] either when an upcaster reports failure or
/// when the upcasted JSON cannot be decoded into the target `A::Event`.
pub fn deserialize_events<A>(
    upcaster_chain: &EventUpcasterChain,
    events: Vec<SerializedEvent>,
) -> DomainResult<Vec<EventEnvelope<A>>>
where
    A: Aggregate,
{
    let events = upcaster_chain.upcast_all(events)?;

    let events = events
        .iter()
        .map(EventEnvelope::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(DomainError::from)?;

    Ok(events)
}
