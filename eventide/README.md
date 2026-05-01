# eventide

[![Crates.io](https://img.shields.io/crates/v/eventide.svg)](https://crates.io/crates/eventide)
[![Documentation](https://docs.rs/eventide/badge.svg)](https://docs.rs/eventide)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

> 中文版本: [README.zh.md](README.zh.md)

A pragmatic Rust toolkit for **Domain-Driven Design** with first-class
support for **event sourcing** and **CQRS**.

`eventide` is the umbrella crate that re-exports the three building blocks
of the toolkit so you can depend on a single crate and pull in everything
you need.

| Module          | Source crate                                                | Purpose                                                            |
|-----------------|-------------------------------------------------------------|--------------------------------------------------------------------|
| `domain`        | [`eventide-domain`](https://crates.io/crates/eventide-domain) | Aggregates, entities, value objects, events, repositories.         |
| `application`   | [`eventide-application`](https://crates.io/crates/eventide-application) | Command bus, query bus, handlers, application context.             |
| `macros`        | [`eventide-macros`](https://crates.io/crates/eventide-macros) | `#[entity]`, `#[entity_id]`, `#[domain_event]`, `#[value_object]`. |

## Quick start

Add the crate to your `Cargo.toml`:

```toml
[dependencies]
eventide = "0.1"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

Define an aggregate:

```rust
use eventide::prelude::*;

#[entity_id]
struct UserId(String);

#[entity(id = UserId)]
#[derive(Clone)]
struct User {
    name: String,
}

#[derive(Debug)]
enum UserCommand {
    Rename { name: String },
}

#[domain_event(version = 1)]
enum UserEvent {
    #[event(event_type = "user.renamed")]
    Renamed { name: String },
}

impl Aggregate for User {
    const TYPE: &'static str = "user";
    type Command = UserCommand;
    type Event = UserEvent;
    type Error = DomainError;

    fn execute(&self, cmd: UserCommand) -> Result<Vec<UserEvent>, DomainError> {
        match cmd {
            UserCommand::Rename { name } if !name.is_empty() => Ok(vec![UserEvent::Renamed {
                id: uuid::Uuid::new_v4().to_string(),
                aggregate_version: self.version().next().value(),
                name,
            }]),
            _ => Err(DomainError::invalid_value("name must not be empty")),
        }
    }

    fn apply(&mut self, event: &UserEvent) {
        match event {
            UserEvent::Renamed { aggregate_version, name, .. } => {
                self.name = name.clone();
                self.version = Version::from_value(*aggregate_version);
            }
        }
    }
}
```

## Feature flags

All flags are enabled by default. Disable them selectively with
`default-features = false` to trim the dependency tree.

| Flag           | Default | What it does                                                                                       |
|----------------|---------|----------------------------------------------------------------------------------------------------|
| `eventing`     | yes     | Asynchronous event subsystem (bus / engine / dispatcher / reclaimer) on top of `tokio`.            |
| `macros`       | yes     | Re-export `eventide-macros` as `eventide::macros`.                                                 |
| `application`  | yes     | Re-export `eventide-application` as `eventide::application`.                                       |
| `infra-sqlx`   | no      | Opt-in `sqlx` conversions on serialized events / snapshots for Postgres-backed event stores.       |

Example: depend on the domain layer only.

```toml
[dependencies]
eventide = { version = "0.1", default-features = false }
```

## Layered architecture

`eventide` is intentionally split into independent crates so the pieces
you do not need can be left out. The dependency graph flows in one
direction:

```text
eventide-application  →  eventide-domain  ←  eventide-macros
                                 ↑
                                 └──── (optional) infra-sqlx
```

The domain crate never depends on application or infrastructure types,
which keeps your business logic pure and easy to test.

## Why eventide

- **Hexagonal-friendly.** The domain layer defines the abstractions (`EventRepository`, `SnapshotRepository`, `AggregateRepository`); infrastructure crates implement them.
- **Event sourcing built in.** Aggregates emit events, the engine persists them, and an upcasting chain handles schema evolution without touching historical data.
- **CQRS by default.** Separate `CommandBus` and `QueryBus` with type-safe handler registration.
- **Procedural macros that disappear.** `#[entity]`, `#[entity_id]`, `#[domain_event]`, and `#[value_object]` remove the boilerplate so the business invariants stay readable.
- **Async-native.** Tokio-based event engine with cooperative dispatch, retry, and dead-letter handling.
- **No vendor lock-in.** The domain crate has zero database dependencies; pick (or write) the infrastructure adapter you need.

## Documentation

- API reference: <https://docs.rs/eventide>
- Workspace overview: [GitHub](https://github.com/nanlong/eventide)
- Examples: see the `examples/` directory in each sub-crate.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](../LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
