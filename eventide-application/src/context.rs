use eventide_domain::{domain_event::EventContext, persist::SerializedEvent};

/// Application Context for a single command/query invocation.
///
/// `AppContext` carries the cross-cutting metadata required by the
/// application layer when handling one logical request. It is created at the
/// system boundary (HTTP handler, message consumer, scheduled job) and
/// passed unchanged through the command/query bus down to the handler so
/// that downstream domain events can be tagged with the correct provenance.
///
/// The context bundles two concerns:
///
/// - Business / tracing context ([`EventContext`]): correlation id, causation
///   id, and the actor (type and id) responsible for the operation. These
///   fields are persisted alongside emitted domain events so that the full
///   causal chain can be reconstructed for auditing and debugging.
/// - Idempotency key (`idempotency_key`): an optional infrastructure-level
///   token used by the API or messaging layer to deduplicate retried
///   submissions. The application/domain layers do not interpret this
///   value — they merely propagate it to the infrastructure boundary.
///
/// # Examples
///
/// ```rust
/// use eventide_application::context::AppContext;
/// use eventide_domain::domain_event::EventContext;
///
/// let ctx = AppContext {
///     event_context: EventContext::builder()
///         .maybe_correlation_id(Some("cor-123".into()))
///         .maybe_causation_id(Some("cau-abc".into()))
///         .maybe_actor_type(Some("user".into()))
///         .maybe_actor_id(Some("u-1".into()))
///         .build(),
///     idempotency_key: Some("idem-xyz".into()),
/// };
/// ```
#[derive(Clone, Debug, Default)]
pub struct AppContext {
    /// Business context: tracing identifiers, audit subject, causal lineage.
    pub event_context: EventContext,
    /// Optional idempotency key. When `None`, the upper layer or
    /// infrastructure decides whether and how to apply idempotent semantics.
    pub idempotency_key: Option<String>,
}

impl From<&SerializedEvent> for AppContext {
    fn from(event: &SerializedEvent) -> Self {
        Self {
            event_context: EventContext::builder()
                .maybe_correlation_id(event.correlation_id().map(ToString::to_string))
                .causation_id(event.event_id().to_string())
                .maybe_actor_type(event.actor_type().map(ToString::to_string))
                .maybe_actor_id(event.actor_id().map(ToString::to_string))
                .build(),
            idempotency_key: None,
        }
    }
}
