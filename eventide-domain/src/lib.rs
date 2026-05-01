//! DDD domain-layer foundational library (`eventide-domain`).
//!
//! Provides the common abstractions and building blocks needed to model and
//! operate domain logic in a Domain-Driven Design + CQRS / event-sourcing
//! style. The crate intentionally focuses on the *domain* layer and ships:
//!
//! - Aggregate (`aggregate`) and entity (`entity`) modelling primitives.
//! - Domain events (`domain_event`) and event upcasting (`event_upcaster`).
//! - Repositories built on event sourcing and snapshots (`persist`).
//! - The eventing system (`eventing`): bus, dispatchers / collectors, engine,
//!   and event handlers.
//! - Reusable patterns such as specifications (`specification`) and value
//!   objects (`value_object`).
//!
//! This crate is deliberately decoupled from concrete storage and transport
//! technologies. It only defines domain-level traits and the minimum error
//! types that consumers need, so adapter crates can plug it into different
//! infrastructure backends (Postgres, message brokers, etc.).
//!
//! # Typical usage
//!
//! 1. Define your aggregate, its commands, and its events; implement
//!    [`aggregate::Aggregate`] with `execute` (command -> events) and
//!    `apply` (event -> state).
//! 2. Pick a repository trait from [`persist`] and provide a concrete
//!    infrastructure implementation.
//! 3. Use [`eventing`] to wire up an event engine connecting the bus to your
//!    dispatcher / collector components.
//! 4. Use [`aggregate_root::AggregateRoot`] to orchestrate the full
//!    "command -> events -> persistence" flow from the application layer.
//!
pub mod aggregate;
pub mod aggregate_root;
pub mod domain_event;
pub mod domain_service;
pub mod entity;
pub mod error;
pub mod event_upcaster;
#[cfg(feature = "eventing")]
pub mod eventing;
pub mod persist;
pub mod specification;
pub mod value_object;

// Allow the crate to refer to itself as `::eventide_domain` from inside this
// crate, so that the procedural macros — which generate fully qualified
// `::eventide_domain::...` paths — keep resolving correctly when used in the
// crate's own unit tests.
extern crate self as eventide_domain;
