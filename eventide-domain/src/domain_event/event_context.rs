use bon::Builder;
use serde::{Deserialize, Serialize};

/// Business-level propagation context attached to every [`EventEnvelope`](super::EventEnvelope).
///
/// Where [`Metadata`](super::Metadata) describes *what* happened from a
/// purely technical standpoint, `EventContext` describes the *circumstances*
/// of the operation: which user or system triggered it, which other event
/// caused it, how long the work took, and which higher-level business
/// transaction it belongs to.
///
/// Carrying this context end-to-end is what enables distributed tracing of
/// business workflows: when a downstream handler reacts to an event and
/// produces a new one, it should preserve `correlation_id` and use the
/// source event's id as the new `causation_id`. The crate's
/// `From<&SerializedEvent>` implementations for `AppContext` automate
/// exactly that pattern.
///
/// All fields are optional because not every entry point provides every
/// piece of information. Use [`EventContext::builder`] to construct
/// instances explicitly:
///
/// ```ignore
/// let ctx = EventContext::builder()
///     .correlation_id("req-42".to_string())
///     .actor_type("user".to_string())
///     .actor_id("u-007".to_string())
///     .build();
/// ```
#[derive(Builder, Default, Debug, Clone, Serialize, Deserialize)]
pub struct EventContext {
    /// Identifier shared by every event produced for the same business
    /// request. Generated once at the system boundary and propagated
    /// unchanged across every reactive step.
    correlation_id: Option<String>,
    /// Identifier of the immediate cause of this event. Typically set to
    /// the `event_id` of the source event when one event reactively
    /// produces another.
    causation_id: Option<String>,
    /// Wall-clock duration in milliseconds spent producing this event,
    /// useful for ad-hoc performance auditing.
    duration_ms: Option<u128>,
    /// Type of the principal that triggered the event (for example
    /// `"user"`, `"system"`, `"scheduler"`).
    actor_type: Option<String>,
    /// Stable identifier of the principal that triggered the event.
    actor_id: Option<String>,

    /// Open-ended JSON bag for ad-hoc context that does not belong to any
    /// of the structured fields above. Skipped during serialization when
    /// `None` to keep stored payloads compact.
    #[serde(skip_serializing_if = "Option::is_none")]
    extensions: Option<serde_json::Value>,
}

impl EventContext {
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    pub fn duration_ms(&self) -> Option<u128> {
        self.duration_ms
    }

    pub fn actor_type(&self) -> Option<&str> {
        self.actor_type.as_deref()
    }

    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    pub fn extensions(&self) -> Option<&serde_json::Value> {
        self.extensions.as_ref()
    }

    pub fn set_actor(&mut self, actor_type: impl Into<String>, actor_id: impl Into<String>) {
        self.actor_type = Some(actor_type.into());
        self.actor_id = Some(actor_id.into());
    }
}
