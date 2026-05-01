//! Domain events and event collections.
//!
//! This module defines the building blocks for representing things that have
//! happened inside an aggregate boundary, expressed in the ubiquitous language
//! of the domain. In an event-sourced system the entire state of an aggregate
//! is reconstructed by replaying its events, so the types defined here form
//! the durable backbone of the write model.
//!
//! The module exposes:
//!
//! - [`DomainEvent`] — the minimal contract every event payload must satisfy
//!   (identity, type tag, payload version, owning aggregate version).
//! - [`EventEnvelope`] — wraps a payload together with [`Metadata`] (who/what/when)
//!   and an [`EventContext`] (correlation, causation, actor) so downstream
//!   consumers have everything needed for tracing and projection.
//! - [`AggregateEvents`] — an ordered collection of envelopes for one aggregate
//!   instance, with helpers to derive audit-style information such as the
//!   creation time or last modifier.
//! - [`FieldChanged`] — a small value object for "before/after" pairs that is
//!   useful when modelling update events.
//! - [`Metadata`] — the technical envelope metadata (aggregate identity and
//!   the timestamp at which the event occurred).
//!
//! Together these types form the lingua franca exchanged between the domain
//! layer, the persistence layer ([`crate::persist`]) and any out-of-process
//! event handlers built on top of this crate.

mod aggregate_events;
mod domain_event_trait;
mod event_context;
mod event_envelope;
mod field_changed;
mod metadata;

pub use aggregate_events::AggregateEvents;
pub use domain_event_trait::DomainEvent;
pub use event_context::EventContext;
pub use event_envelope::EventEnvelope;
pub use field_changed::FieldChanged;
pub use metadata::Metadata;
