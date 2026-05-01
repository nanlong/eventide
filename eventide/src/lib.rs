//! # Eventide
//!
//! A pragmatic Rust toolkit for **Domain-Driven Design** with first-class
//! support for **event sourcing** and **CQRS**.
//!
//! This umbrella crate re-exports the three building blocks that make up the
//! toolkit so you can depend on a single crate and pull in everything you
//! need:
//!
//! | Module          | Source crate             | Purpose                                                            |
//! |-----------------|--------------------------|--------------------------------------------------------------------|
//! | [`domain`]      | [`eventide-domain`]      | Aggregates, entities, value objects, events, repositories.         |
//! | [`application`] | [`eventide-application`] | Command bus, query bus, handlers, application context.             |
//! | [`mod@macros`]  | [`eventide-macros`]      | `#[entity]`, `#[entity_id]`, `#[domain_event]`, `#[value_object]`. |
//!
//! [`eventide-domain`]: https://docs.rs/eventide-domain
//! [`eventide-application`]: https://docs.rs/eventide-application
//! [`eventide-macros`]: https://docs.rs/eventide-macros
//!
//! ## Quick start
//!
//! Add the crate to your `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! eventide = "0.1"
//! serde = { version = "1", features = ["derive"] }
//! ```
//!
//! Then bring the most common items into scope through the [`prelude`]:
//!
//! ```rust,ignore
//! use eventide::prelude::*;
//!
//! #[entity_id]
//! struct UserId(String);
//!
//! #[entity(id = UserId)]
//! #[derive(Clone, Default)]
//! struct User {
//!     name: String,
//! }
//! ```
//!
//! ## Feature flags
//!
//! All flags are enabled by default. Disable them selectively with
//! `default-features = false` to trim the dependency tree.
//!
//! - **`eventing`** *(default)* — pull in [`eventide_domain::eventing`], the
//!   asynchronous event bus / engine / dispatcher built on top of `tokio`.
//! - **`macros`** *(default)* — re-export [`eventide_macros`] as
//!   [`mod@macros`].
//! - **`application`** *(default)* — re-export [`eventide_application`] as
//!   [`application`].
//! - **`infra-sqlx`** — opt-in `sqlx` conversions on serialized events and
//!   snapshots so infrastructure crates can talk to a Postgres-backed event
//!   store without forcing a database driver into the domain layer.
//!
//! ## Layered architecture
//!
//! `eventide` is intentionally split into independent crates so the pieces
//! you do not need can be left out. The dependency graph flows in one
//! direction:
//!
//! ```text
//! eventide-application  →  eventide-domain  ←  eventide-macros
//!                                  ↑
//!                                  └──── (optional) infra-sqlx
//! ```
//!
//! Domain code never depends on application or infrastructure types, which
//! keeps your business logic pure and easy to test.

#![cfg_attr(docsrs, feature(doc_cfg))]

/// Domain layer: aggregates, entities, value objects, events, repositories,
/// and the optional asynchronous event engine.
///
/// Re-export of the [`eventide_domain`] crate.
pub use eventide_domain as domain;

/// Application layer: command bus, query bus, handlers, application context.
///
/// Re-export of the [`eventide_application`] crate. Available when the
/// `application` feature is enabled (on by default).
#[cfg(feature = "application")]
#[cfg_attr(docsrs, doc(cfg(feature = "application")))]
pub use eventide_application as application;

/// Procedural macros for entities, entity IDs, value objects and domain
/// events.
///
/// Re-export of the [`eventide_macros`] crate. Available when the `macros`
/// feature is enabled (on by default).
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
pub use eventide_macros as macros;

/// Curated set of imports that cover the day-to-day needs of writing
/// aggregates, events, commands and queries.
///
/// Bring everything in with a single glob import:
///
/// ```rust,ignore
/// use eventide::prelude::*;
/// ```
///
/// The prelude is intentionally small. If you reach for something not
/// re-exported here, import it directly from [`crate::domain`],
/// [`crate::application`] or [`crate::macros`].
pub mod prelude {
    // Core building blocks: aggregates, entities, value objects.
    pub use crate::domain::aggregate::Aggregate;
    pub use crate::domain::aggregate_root::AggregateRoot;
    pub use crate::domain::entity::Entity;
    pub use crate::domain::specification::Specification;
    pub use crate::domain::value_object::{ValueObject, Version};

    // Domain events and the helpers used while writing event handlers and
    // upcasters.
    pub use crate::domain::domain_event::{
        DomainEvent, EventContext, EventEnvelope, FieldChanged, Metadata,
    };
    pub use crate::domain::event_upcaster::{
        EventUpcaster, EventUpcasterChain, EventUpcasterResult,
    };

    // Domain-level error types.
    pub use crate::domain::error::{DomainError, DomainResult, ErrorCode, ErrorKind};

    // Persistence abstractions. Concrete adapters live in infrastructure crates.
    pub use crate::domain::persist::{
        AggregateRepository, EventRepository, EventRepositoryExt, EventSourcedRepo,
        SerializedEvent, SerializedSnapshot, SnapshotPolicy, SnapshotPolicyRepo,
        SnapshotRepository,
    };

    // Asynchronous event subsystem (only available with the `eventing` feature).
    #[cfg(feature = "eventing")]
    #[cfg_attr(docsrs, doc(cfg(feature = "eventing")))]
    pub use crate::domain::eventing::{
        EngineHandle, EventBus, EventDeliverer, EventEngine, EventEngineConfig, EventHandler,
        EventReclaimer, HandledEventType, InMemoryEventBus,
    };

    // Application layer essentials (only available with the `application` feature).
    #[cfg(feature = "application")]
    #[cfg_attr(docsrs, doc(cfg(feature = "application")))]
    pub use crate::application::{
        InMemoryCommandBus, InMemoryQueryBus,
        command_bus::CommandBus,
        command_handler::CommandHandler,
        context::AppContext,
        error::{AppError, AppResult},
        query_bus::QueryBus,
        query_handler::QueryHandler,
    };

    // Procedural macros (only available with the `macros` feature).
    #[cfg(feature = "macros")]
    #[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
    pub use crate::macros::{domain_event, entity, entity_id, value_object};
}
