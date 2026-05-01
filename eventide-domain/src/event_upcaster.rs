//! Event upcasting.
//!
//! As the schema of a domain event evolves, historical events stored under
//! the old schema must still be readable. An [`EventUpcaster`] performs a
//! single transformation on the read path — it can split a legacy event
//! into multiple newer ones, merge several into one, rename / reshape
//! fields, or drop the event entirely.
//!
//! [`EventUpcasterChain`] strings several upcasters together and applies
//! them repeatedly to a batch of events until the output stops changing
//! (i.e. every event has reached its current schema).
//!
use crate::{error::DomainResult as Result, persist::SerializedEvent};
use std::sync::Arc;

/// Single stage in an upcasting pipeline.
///
/// An upcaster decides whether it [`EventUpcaster::applies`] to a given
/// event (by inspecting its type and version) and, if so, transforms it via
/// [`EventUpcaster::upcast`]. The transformation is expressed as an
/// [`EventUpcasterResult`] so an upcaster can produce zero, one, or many
/// successor events without committing to a single shape up front.
pub trait EventUpcaster: Send + Sync {
    fn applies(&self, event_type: &str, event_version: usize) -> bool;

    fn upcast(&self, event: SerializedEvent) -> Result<EventUpcasterResult>;
}

impl<T> EventUpcaster for Arc<T>
where
    T: EventUpcaster + ?Sized,
{
    fn applies(&self, event_type: &str, event_version: usize) -> bool {
        (**self).applies(event_type, event_version)
    }

    fn upcast(&self, event: SerializedEvent) -> Result<EventUpcasterResult> {
        (**self).upcast(event)
    }
}

/// Result of a single upcasting step.
///
/// - [`EventUpcasterResult::One`] replaces the event with a new event.
/// - [`EventUpcasterResult::Many`] expands one event into several.
/// - [`EventUpcasterResult::Drop`] discards the event entirely.
#[allow(clippy::large_enum_variant)]
pub enum EventUpcasterResult {
    One(SerializedEvent),
    Many(Vec<SerializedEvent>),
    Drop,
}

/// Ordered chain of [`EventUpcaster`]s applied as an event-loading
/// pipeline.
///
/// The chain runs every stage in sequence and then iterates until no
/// further transformation occurs. This guarantees that even multi-step
/// migrations (e.g. v1 -> v2 -> v3 with different upcasters per step)
/// converge to the latest schema in a single call to
/// [`EventUpcasterChain::upcast_all`].
pub struct EventUpcasterChain {
    stages: Vec<Arc<dyn EventUpcaster>>,
}

impl Default for EventUpcasterChain {
    fn default() -> Self {
        Self::from_iter(vec![])
    }
}

impl EventUpcasterChain {
    /// Upcast every event in `events` repeatedly until the result is
    /// stable, i.e. one full pass through the chain produces no further
    /// changes.
    pub fn upcast_all(&self, mut events: Vec<SerializedEvent>) -> Result<Vec<SerializedEvent>> {
        loop {
            let (upcasted, has_changes) = self.upcast_once(events)?;
            if !has_changes {
                return Ok(upcasted);
            }
            events = upcasted;
        }
    }

    /// Run a single full pass through the chain, returning the resulting
    /// events together with a flag indicating whether any upcaster matched.
    fn upcast_once(&self, events: Vec<SerializedEvent>) -> Result<(Vec<SerializedEvent>, bool)> {
        let mut has_changes = false;

        let upcasted = events
            .into_iter()
            .map(|event| self.upcast_single_event(event, &mut has_changes))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();

        Ok((upcasted, has_changes))
    }

    /// Push a single event through every stage of the chain in order.
    fn upcast_single_event(
        &self,
        event: SerializedEvent,
        has_changes: &mut bool,
    ) -> Result<Vec<SerializedEvent>> {
        self.stages.iter().try_fold(vec![event], |events, stage| {
            self.apply_stage(stage, events, has_changes)
        })
    }

    /// Apply a single upcaster stage to a list of events that have already
    /// been produced by the previous stages, splatting any `Many` results
    /// and dropping `Drop` results.
    fn apply_stage(
        &self,
        stage: &Arc<dyn EventUpcaster>,
        events: Vec<SerializedEvent>,
        has_changes: &mut bool,
    ) -> Result<Vec<SerializedEvent>> {
        let results = events
            .into_iter()
            .map(|event| {
                if stage.applies(event.event_type(), event.event_version()) {
                    *has_changes = true;
                    stage.upcast(event)
                } else {
                    Ok(EventUpcasterResult::One(event))
                }
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(results
            .into_iter()
            .flat_map(|result| match result {
                EventUpcasterResult::One(e) => vec![e],
                EventUpcasterResult::Many(v) => v,
                EventUpcasterResult::Drop => vec![],
            })
            .collect())
    }
}

impl FromIterator<Arc<dyn EventUpcaster>> for EventUpcasterChain {
    fn from_iter<I: IntoIterator<Item = Arc<dyn EventUpcaster>>>(iter: I) -> Self {
        Self {
            stages: iter.into_iter().collect(),
        }
    }
}

impl Extend<Arc<dyn EventUpcaster>> for EventUpcasterChain {
    fn extend<I: IntoIterator<Item = Arc<dyn EventUpcaster>>>(&mut self, iter: I) {
        self.stages.extend(iter);
    }
}

#[cfg(test)]
mod tests {
    use super::{EventUpcaster, EventUpcasterChain, EventUpcasterResult};
    use crate::domain_event::EventContext;
    use crate::error::{DomainError, DomainResult};
    use crate::persist::SerializedEvent;
    use chrono::Utc;
    use std::sync::Arc;

    fn mk_event(ty: &str, ver: usize, payload: serde_json::Value) -> SerializedEvent {
        let id = ulid::Ulid::new().to_string();
        let event_context = EventContext::builder()
            .maybe_correlation_id(Some(format!("cor-{id}")))
            .maybe_causation_id(Some(format!("cau-{id}")))
            .maybe_actor_type(Some("user".into()))
            .maybe_actor_id(Some("u-1".into()))
            .build();
        SerializedEvent::builder()
            .event_id(id)
            .event_type(ty.to_string())
            .event_version(ver)
            .maybe_sequence_number(None)
            .aggregate_id("a-1".to_string())
            .aggregate_type("Order".to_string())
            .aggregate_version(0)
            .correlation_id("cor-a-1".into())
            .causation_id("cau-a-1".into())
            .actor_type("user".into())
            .actor_id("u-1".into())
            .occurred_at(Utc::now())
            .payload(payload)
            .context(serde_json::to_value(&event_context).expect("serialize EventContext"))
            .build()
    }

    struct SplitV1; // v1 -> two events
    impl EventUpcaster for SplitV1 {
        fn applies(&self, event_type: &str, event_version: usize) -> bool {
            event_type == "legacy.order.created" && event_version == 1
        }

        fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
            let base = event.payload();
            let id = base.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let business_context = EventContext::builder()
                .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
                .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
                .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
                .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
                .build();

            let init = SerializedEvent::builder()
                .event_id(event.event_id().to_string())
                .event_type("order.init".to_string())
                .event_version(2)
                .maybe_sequence_number(None)
                .aggregate_id(event.aggregate_id().to_string())
                .aggregate_type(event.aggregate_type().to_string())
                .aggregate_version(event.aggregate_version())
                .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
                .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
                .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
                .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
                .occurred_at(event.occurred_at())
                .payload(serde_json::json!({ "id": id, "stage": "init" }))
                .context(serde_json::to_value(&business_context).expect("serialize EventContext"))
                .build();

            let meta = SerializedEvent::builder()
                .event_id(event.event_id().to_string())
                .event_type("order.meta".to_string())
                .event_version(1)
                .maybe_sequence_number(None)
                .aggregate_id(event.aggregate_id().to_string())
                .aggregate_type(event.aggregate_type().to_string())
                .aggregate_version(event.aggregate_version())
                .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
                .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
                .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
                .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
                .occurred_at(event.occurred_at())
                .payload(serde_json::json!({ "id": id, "meta": {"source": "legacy"} }))
                .context(serde_json::to_value(&business_context).expect("serialize EventContext"))
                .build();

            Ok(EventUpcasterResult::Many(vec![init, meta]))
        }
    }

    struct DropMeta; // drop order.meta events
    impl EventUpcaster for DropMeta {
        fn applies(&self, event_type: &str, _event_version: usize) -> bool {
            event_type == "order.meta"
        }
        fn upcast(&self, _event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
            Ok(EventUpcasterResult::Drop)
        }
    }

    struct RenameInitToCreated; // v2 init -> v3 created
    impl EventUpcaster for RenameInitToCreated {
        fn applies(&self, event_type: &str, event_version: usize) -> bool {
            event_type == "order.init" && event_version == 2
        }
        fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
            let business_context = EventContext::builder()
                .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
                .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
                .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
                .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
                .build();

            let next = SerializedEvent::builder()
                .event_id(event.event_id().to_string())
                .event_type("order.created".to_string())
                .event_version(3)
                .maybe_sequence_number(None)
                .aggregate_id(event.aggregate_id().to_string())
                .aggregate_type(event.aggregate_type().to_string())
                .aggregate_version(event.aggregate_version())
                .maybe_correlation_id(event.correlation_id().map(|s| s.to_string()))
                .maybe_causation_id(event.causation_id().map(|s| s.to_string()))
                .maybe_actor_type(event.actor_type().map(|s| s.to_string()))
                .maybe_actor_id(event.actor_id().map(|s| s.to_string()))
                .occurred_at(event.occurred_at())
                .payload(event.payload().clone())
                .context(serde_json::to_value(&business_context).expect("serialize EventContext"))
                .build();
            Ok(EventUpcasterResult::One(next))
        }
    }

    #[test]
    fn complex_chain_split_drop_until_stable() {
        let chain: EventUpcasterChain = vec![
            Arc::new(SplitV1) as Arc<dyn EventUpcaster>,
            Arc::new(DropMeta) as Arc<dyn EventUpcaster>,
            Arc::new(RenameInitToCreated) as Arc<dyn EventUpcaster>,
        ]
        .into_iter()
        .collect();

        let legacy = mk_event("legacy.order.created", 1, serde_json::json!({"id": "o-1"}));
        let other = mk_event("noop", 1, serde_json::json!({"x": 1}));

        let input = vec![legacy, other.clone()];
        let out = chain.upcast_all(input).unwrap();

        // Expected pipeline: legacy splits into init(v2) + meta(v1); meta is
        // then dropped, init(v2) is renamed to created(v3); the unrelated
        // `noop` event is left untouched.
        assert_eq!(out.len(), 2);
        let types: Vec<(String, usize)> = out
            .iter()
            .map(|e| (e.event_type().to_string(), e.event_version()))
            .collect();
        assert!(types.contains(&("order.created".to_string(), 3)));
        assert!(types.contains(&(other.event_type().to_string(), other.event_version())));
    }

    struct AlwaysFail;
    impl EventUpcaster for AlwaysFail {
        fn applies(&self, _event_type: &str, _event_version: usize) -> bool {
            true
        }
        fn upcast(&self, event: SerializedEvent) -> DomainResult<EventUpcasterResult> {
            Err(DomainError::upcast_failed(
                event.event_type(),
                event.event_version(),
                Some("AlwaysFail"),
                "boom",
            ))
        }
    }

    #[test]
    fn upcast_failure_returns_error() {
        use crate::error::{ErrorCode, ErrorKind};

        let chain: EventUpcasterChain = vec![Arc::new(AlwaysFail) as Arc<dyn EventUpcaster>]
            .into_iter()
            .collect();
        let input = vec![mk_event("noop", 1, serde_json::json!({}))];
        let err = chain.upcast_all(input).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::Internal);
        assert_eq!(err.code(), "UPCAST_FAILED");
    }
}
