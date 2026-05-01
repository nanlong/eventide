//! Event deliverer protocol.
//!
//! The [`EventDeliverer`] is responsible for pulling pending events in
//! batches from local persistent storage (typically a transactional
//! Outbox table populated alongside aggregate writes) and, after the
//! engine publishes them to the bus, marking each event as either
//! successfully delivered or failed. The success/failure marks drive
//! retry behavior and provide an auditable trail of delivery attempts.
//!
use crate::{error::DomainResult as Result, persist::SerializedEvent};
use async_trait::async_trait;

/// Event deliverer: fetches outbound events from local storage
/// (Outbox) and reports their delivery outcome.
#[async_trait]
pub trait EventDeliverer: Send + Sync {
    /// Fetches the batch of events that are pending delivery from the
    /// underlying Outbox.
    async fn fetch_events(&self) -> Result<Vec<SerializedEvent>>;

    /// Marks the given events as successfully delivered to the bus.
    async fn mark_delivered(&self, events: &[&SerializedEvent]) -> Result<()>;

    /// Marks the given events as failed during delivery. Implementations
    /// typically use this to increment an `attempts` counter, set a
    /// `next_retry_at` timestamp for backoff, or move events into a
    /// dead-letter table after exceeding a retry limit.
    async fn mark_failed(&self, events: &[&SerializedEvent], reason: &str) -> Result<()>;
}
