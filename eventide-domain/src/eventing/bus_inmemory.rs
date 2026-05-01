//! In-memory event bus implementation.
//!
//! Provides [`InMemoryEventBus`], a lightweight [`EventBus`]
//! implementation backed by `tokio::sync::broadcast`:
//!
//! - `publish`: clones the event and broadcasts it to all current
//!   subscribers.
//! - `subscribe`: returns a stream with a `'static` lifetime, suitable
//!   for use inside `tokio::spawn` tasks.
//! - Typical use cases: unit/integration tests, examples, and local
//!   development where no external broker is desired.
//!
//! Note: this implementation provides at-most-once delivery semantics.
//! If there are no active subscribers at the moment of publication,
//! the event is silently dropped. For at-least-once guarantees in
//! production, pair the engine with the durable
//! [`EventDeliverer`](crate::eventing::EventDeliverer) (Outbox) and
//! [`EventReclaimer`](crate::eventing::EventReclaimer) (compensation)
//! abstractions.

use crate::error::{DomainError, DomainResult as Result};
use crate::eventing::EventBus;
use crate::persist::SerializedEvent;
use async_trait::async_trait;
use futures_core::stream::BoxStream;
use futures_util::StreamExt;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

/// A simple in-memory event bus implementation.
#[derive(Clone)]
pub struct InMemoryEventBus {
    tx: broadcast::Sender<SerializedEvent>,
}

impl InMemoryEventBus {
    /// Creates an in-memory bus where `capacity` is the size of the
    /// broadcast channel buffer. Slow subscribers that fall behind
    /// by more than `capacity` events will observe lag errors on
    /// their stream.
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish(&self, event: &SerializedEvent) -> Result<()> {
        // If there are no current subscribers, broadcast `send` returns
        // an error; we treat that as non-fatal and silently discard it,
        // since publishing without consumers is a valid runtime state.
        let _ = self.tx.send(event.clone());
        Ok(())
    }

    async fn subscribe(&self) -> BoxStream<'static, Result<SerializedEvent>> {
        let rx = self.tx.subscribe();
        let stream =
            BroadcastStream::new(rx).map(|r| r.map_err(|e| DomainError::event_bus(e.to_string())));
        Box::pin(stream)
    }
}
