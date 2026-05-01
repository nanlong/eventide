//! Aggregate abstraction.
//!
//! Defines the core behaviour every aggregate must implement:
//!
//! - [`Aggregate::execute`] turns a command into a list of events without
//!   mutating state — it is a pure decision function.
//! - [`Aggregate::apply`] projects an event onto the aggregate state — it is
//!   the only place where state may be mutated.
//! - The [`crate::entity::Entity`] super-trait constrains the aggregate to
//!   carry an identity and a version (used for optimistic concurrency).
//!
use crate::domain_event::DomainEvent;
use crate::entity::Entity;
use serde::{Serialize, de::DeserializeOwned};
use std::error::Error;

/// Aggregate-root contract.
///
/// Implementors are the consistency boundary in a DDD model: a single command
/// produces a list of [`DomainEvent`]s, all of which must be persisted
/// atomically. State changes are derived solely by replaying / applying
/// events, which keeps `execute` pure and makes event sourcing trivial.
///
/// Every aggregate must be [`Default`] so that it can be instantiated as a
/// blank slate before its events are replayed, and it must be
/// [`Serialize`] / [`DeserializeOwned`] so that it can participate in
/// snapshotting.
pub trait Aggregate: Entity + Default + Serialize + DeserializeOwned + Send + Sync {
    const TYPE: &'static str;

    /// Command type this aggregate accepts via [`Aggregate::execute`].
    type Command;
    /// Domain-event type this aggregate emits.
    type Event: DomainEvent;
    /// Error type returned from command execution and persistence.
    type Error: Error + Send + Sync + 'static;

    /// Validate the command against current state and return the events that
    /// would be produced. Must not mutate `self`.
    fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error>;

    /// Apply a previously produced event to update the aggregate's state.
    /// This is the only place where the aggregate's fields may be mutated,
    /// and it must be infallible — events are facts that already happened.
    fn apply(&mut self, event: &Self::Event);
}

#[cfg(test)]
mod tests {
    use super::Aggregate;
    use crate::domain_event::EventEnvelope;
    use crate::domain_event::{DomainEvent, EventContext};
    use crate::entity::Entity;
    use crate::error::DomainError;
    use crate::value_object::Version;
    use eventide_macros::{domain_event, entity};
    use serde::{Deserialize, Serialize};

    #[entity]
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct Counter {
        value: i32,
    }

    #[derive(Debug)]
    enum CounterCommand {
        Add { amount: i32 },
        Sub { amount: i32 },
    }

    #[domain_event(version = 1)]
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    enum CounterEvent {
        Added { amount: i32 },
        Subtracted { amount: i32 },
    }

    impl Aggregate for Counter {
        const TYPE: &'static str = "counter";
        type Command = CounterCommand;
        type Event = CounterEvent;
        type Error = DomainError;

        fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
            match command {
                CounterCommand::Add { amount } => {
                    if amount <= 0 {
                        return Err(DomainError::invalid_command("amount must be > 0"));
                    }
                    Ok(vec![CounterEvent::Added {
                        id: ulid::Ulid::new().to_string(),
                        aggregate_version: self.version().next(),
                        amount,
                    }])
                }
                CounterCommand::Sub { amount } => {
                    if amount <= 0 {
                        return Err(DomainError::invalid_command("amount must be > 0"));
                    }
                    if self.value < amount {
                        return Err(DomainError::invalid_state("insufficient"));
                    }
                    Ok(vec![CounterEvent::Subtracted {
                        id: ulid::Ulid::new().to_string(),
                        aggregate_version: self.version().next(),
                        amount,
                    }])
                }
            }
        }

        fn apply(&mut self, event: &Self::Event) {
            match event {
                CounterEvent::Added {
                    aggregate_version,
                    amount,
                    ..
                } => {
                    self.value += *amount;
                    self.version = *aggregate_version;
                }
                CounterEvent::Subtracted {
                    aggregate_version,
                    amount,
                    ..
                } => {
                    self.value -= *amount;
                    self.version = *aggregate_version;
                }
            }
        }
    }

    #[tokio::test]
    async fn aggregate_lifecycle_create_execute_apply_envelope() {
        let id = "c-1".to_string();
        let agg = Counter::new(id.clone(), Version::new());
        assert_eq!(agg.id(), &id);
        assert_eq!(agg.version(), Version::new());
        assert_eq!(agg.value, 0);

        // Execute an Add command and inspect the resulting event.
        let events = agg.execute(CounterCommand::Add { amount: 3 }).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            CounterEvent::Added {
                aggregate_version,
                amount,
                ..
            } => {
                assert_eq!(*aggregate_version, Version::from_value(1));
                assert_eq!(*amount, 3);
            }
            _ => panic!("unexpected event"),
        }

        // Apply the event onto a fresh copy of the aggregate.
        let mut agg2 = agg.clone();
        for e in &events {
            agg2.apply(e);
        }
        assert_eq!(agg2.version(), Version::from_value(1));
        assert_eq!(agg2.value, 3);

        // Continue executing and applying so the version monotonically grows.
        let ev2 = agg2.execute(CounterCommand::Add { amount: 2 }).unwrap();
        let mut agg3 = agg2.clone();
        for e in &ev2 {
            agg3.apply(e);
        }
        let ev3 = agg3.execute(CounterCommand::Sub { amount: 1 }).unwrap();
        for e in &ev3 {
            agg3.apply(e);
        }
        assert_eq!(agg3.version(), Version::from_value(3));
        assert_eq!(agg3.value, 4);

        // Wrap an event in an EventEnvelope as it would be just before
        // persistence, attaching the propagation context.
        let ctx = EventContext::default();
        let envelopes: Vec<EventEnvelope<Counter>> = vec![EventEnvelope::new(
            agg3.id(),
            CounterEvent::Added {
                id: ulid::Ulid::new().to_string(),
                aggregate_version: agg3.version().next(),
                amount: 10,
            },
            ctx.clone(),
        )];
        assert_eq!(envelopes.len(), 1);
        assert_eq!(
            envelopes[0].payload.aggregate_version(),
            agg3.version().next()
        );
    }

    #[test]
    fn invalid_commands_should_error() {
        let agg = Counter::new("c-2".to_string(), Version::new());
        use crate::error::ErrorKind;

        let err = agg.execute(CounterCommand::Sub { amount: 1 }).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidState);

        let err = agg.execute(CounterCommand::Add { amount: 0 }).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidCommand);
    }
}
