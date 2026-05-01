//! Event bus protocol definition.
//!
//! This module defines the [`EventBus`] trait — the unified abstraction
//! for event publishing and subscription. Implementations may target
//! any underlying transport (in-memory broadcast channels, Kafka, NATS,
//! Redis Streams, etc.) without leaking transport-specific details to
//! the rest of the system.
//!
//! The trait supports both individual and batched publication, and
//! returns a `'static`-lifetime event stream from `subscribe` so that
//! consumers may freely move the stream into spawned async tasks
//! (e.g. `tokio::spawn`) without lifetime constraints.
//!
use crate::{error::DomainResult as Result, persist::SerializedEvent};
use async_trait::async_trait;
use futures_core::stream::BoxStream;

/// Event bus: responsible for dispatching events to subscribers and
/// exposing event streams for consumption.
#[async_trait]
pub trait EventBus: Send + Sync {
    async fn publish(&self, event: &SerializedEvent) -> Result<()>;

    async fn publish_batch(&self, events: &[SerializedEvent]) -> Result<()> {
        for event in events {
            self.publish(event).await?;
        }
        Ok(())
    }

    /// Returns an event stream with a `'static` lifetime, allowing it
    /// to be moved into spawned tasks such as `tokio::spawn`.
    async fn subscribe(&self) -> BoxStream<'static, Result<SerializedEvent>>;
}
