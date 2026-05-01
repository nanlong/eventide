//! Asynchronous eventing subsystem.
//!
//! This module provides the foundational abstractions and runtime for
//! event publishing, subscription, and processing within an event-driven
//! DDD/CQRS architecture:
//!
//! - [`EventBus`]: A unified publish/subscribe interface decoupled from
//!   any specific transport implementation.
//! - [`EventDeliverer`]: Pulls pending events in batches from local
//!   persistent storage (such as the transactional Outbox pattern) and
//!   forwards them to the bus.
//! - [`EventReclaimer`]: Performs compensation for events that failed,
//!   timed out, or were missed during normal delivery, ensuring
//!   eventual consistency across aggregate boundaries.
//! - [`EventHandler`]: Defines the contract for consumer-side processing
//!   of one, many, or all event types.
//! - [`EventEngine`]: Orchestrates the full pipeline of delivery,
//!   subscription, and dispatch — running concurrent handler execution,
//!   marking failures, and triggering compensation flows.
//!
//! This module intentionally defines only the protocol and the
//! orchestration engine; it is not bound to any specific transport.
//! It can be wired up to any messaging system (Kafka, NATS, RabbitMQ,
//! etc.) or used with the in-memory implementation provided here for
//! testing and local development.
//!
pub mod bus;
pub mod bus_inmemory;
pub mod deliverer;
pub mod engine;
pub mod handler;
pub mod reclaimer;

pub use bus::EventBus;
pub use bus_inmemory::InMemoryEventBus;
pub use deliverer::EventDeliverer;
pub use engine::{EngineHandle, EventEngine, EventEngineConfig};
pub use handler::{EventHandler, HandledEventType};
pub use reclaimer::EventReclaimer;
