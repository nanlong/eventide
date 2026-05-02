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

// Re-export `serde` so `eventide-macros` can route generated `Serialize` /
// `Deserialize` derive paths through this crate. Lets downstream users
// depend only on `eventide` (or `eventide-domain`) without also pulling
// `serde` into their own `Cargo.toml`.
#[doc(hidden)]
pub use serde as __serde;

// Re-export `async_trait` so users can write `#[async_trait]` on their own
// trait impls (e.g. `DomainService`, `EventHandler`) without adding a
// direct `async-trait` dependency. Two re-exports are needed because the
// crate (module namespace) and the attribute macro (macro namespace) live
// in separate Rust namespaces:
//
// * `pub use ::async_trait;` — exposes the crate as `eventide_domain::async_trait`,
//   so paths like `eventide_domain::async_trait::Boxed` keep working.
// * `pub use ::async_trait::async_trait;` — exposes the attribute macro under
//   the same path, so `use eventide_domain::async_trait;` followed by
//   `#[async_trait]` resolves correctly. The leading `::` anchors the
//   lookup at the crate root rather than re-entering the module re-export
//   on the previous line, which would otherwise create a resolution loop.
pub use ::async_trait;
pub use ::async_trait::async_trait;

/// Re-export of the `tokio` runtime, available when the `eventing` feature
/// is enabled.
///
/// Lets downstream users write `#[tokio::main]` / `tokio::spawn` / etc.
/// through `eventide_domain::tokio` (or `eventide::tokio`) without adding a
/// direct `tokio` dependency. Gated on `eventing` because that is the only
/// feature that already pulls `tokio` into the build graph; pure modelling
/// users who opt out of `eventing` keep a runtime-free dependency tree.
#[cfg(feature = "eventing")]
#[cfg_attr(docsrs, doc(cfg(feature = "eventing")))]
pub use tokio;
