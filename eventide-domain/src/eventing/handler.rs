//! Event handler protocol.
//!
//! Defines the consumer-side abstraction for processing events,
//! together with metadata describing what the handler subscribes to
//! (one specific event type, an explicit set, or all events) and how
//! the handler identifies itself for failure tracking and auditing.
//!
use crate::persist::SerializedEvent;
use async_trait::async_trait;

#[derive(Clone, Debug)]
pub enum HandledEventType {
    One(String),
    Many(Vec<String>),
    All,
}

/// Event handler: consumes events of one or more types and performs
/// the associated side effects (projection updates, integrations,
/// notifications, etc.).
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// Stable, unique handler name used for failure marking and audit
    /// trails. The reclaimer records failures against this name so
    /// that retries and diagnostics can attribute issues to a specific
    /// handler instance.
    fn handler_name(&self) -> &str;
    /// Returns the set of event types this handler is interested in.
    fn handled_event_type(&self) -> HandledEventType;
    /// Processes a single event. Returning an error signals that the
    /// engine should mark this handler as failed for the given event,
    /// triggering compensation through the reclaimer.
    async fn handle(&self, event: &SerializedEvent) -> anyhow::Result<()>;
}
