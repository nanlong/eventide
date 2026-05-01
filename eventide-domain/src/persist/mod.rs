//! Persistence and event-sourcing primitives.
//!
//! This module defines the abstract contracts the domain layer uses to talk
//! to a storage backend, plus thin reusable building blocks that compose
//! those contracts into full aggregate repositories. It is the seam through
//! which the hexagonal architecture's "secondary ports" are exposed:
//! concrete adapters (Postgres, in-memory, etc.) live elsewhere and only
//! implement the traits declared here, never the other way around.
//!
//! What you will find inside:
//!
//! - [`EventRepository`] — append-only log abstraction for persisting events
//!   and reading either the full or the incremental history of an aggregate.
//!   The companion [`EventRepositoryExt`] adds an upcasting-aware helper to
//!   produce an [`AggregateEvents`](crate::domain_event::AggregateEvents)
//!   in one call.
//! - [`SnapshotRepository`] together with [`SnapshotPolicy`] and
//!   [`SnapshotRepositoryWithPolicy`] — optional support for periodic
//!   snapshots that bound the cost of replaying long histories.
//! - [`SerializedEvent`] / [`SerializedSnapshot`] — the on-the-wire shapes
//!   used to cross the persistence boundary, plus
//!   [`serialize_events`]/[`deserialize_events`] helpers that integrate
//!   transparently with the [`EventUpcasterChain`](crate::event_upcaster::EventUpcasterChain).
//! - [`AggregateRepository`] and the two ready-made implementations
//!   [`EventSourcedRepo`] (pure event sourcing) and [`SnapshotPolicyRepo`]
//!   (event sourcing + snapshot policy). Higher-level code wires either of
//!   these into [`AggregateRoot`](crate::aggregate_root::AggregateRoot) and stops
//!   thinking about replay or persistence details.
//!
//! The module is deliberately storage-agnostic: it focuses on the protocol
//! and the assembly logic, while concrete backends are expected to be
//! supplied and injected by the application's composition root.
//!
mod aggregate_repository;
mod event_repository;
mod serialized_event;
mod serialized_snapshot;
mod snapshot_repository;

pub use aggregate_repository::{AggregateRepository, EventSourcedRepo, SnapshotPolicyRepo};
pub use event_repository::{EventRepository, EventRepositoryExt};
pub use serialized_event::{SerializedEvent, deserialize_events, serialize_events};
pub use serialized_snapshot::SerializedSnapshot;
pub use snapshot_repository::{SnapshotPolicy, SnapshotRepository, SnapshotRepositoryWithPolicy};
