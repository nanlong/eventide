# eventide

[![Crates.io](https://img.shields.io/crates/v/eventide.svg)](https://crates.io/crates/eventide)
[![Documentation](https://docs.rs/eventide/badge.svg)](https://docs.rs/eventide)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

> 中文版本: [README.zh.md](README.zh.md)

A pragmatic Rust toolkit for **Domain-Driven Design** with first-class
support for **event sourcing** and **CQRS**, organised as a hexagonal
workspace.

```text
eventide-application  →  eventide-domain  ←  eventide-macros
                                 ↑
                                 └──── (optional) infra-sqlx adapters
```

## Workspace layout

| Crate                                             | What it ships                                                                          |
|---------------------------------------------------|----------------------------------------------------------------------------------------|
| [`eventide`](eventide/)                           | Umbrella crate. Re-exports the three layers and a curated `prelude`.                   |
| [`eventide-domain`](eventide-domain/)             | Pure domain layer: aggregates, entities, value objects, events, repository traits.     |
| [`eventide-application`](eventide-application/)   | Application layer: command bus, query bus, handlers, application context.              |
| [`eventide-macros`](eventide-macros/)             | Procedural macros (`#[entity]`, `#[entity_id]`, `#[domain_event]`, `#[value_object]`). |

## Quick start

Most users only need to depend on the umbrella crate:

```toml
[dependencies]
eventide = "0.1"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"
```

A minimal aggregate looks like this:

```rust
use eventide::prelude::*;

#[entity_id]
struct AccountId(String);

#[entity(id = AccountId)]
#[derive(Clone)]
struct BankAccount {
    balance: i64,
}

#[derive(Debug)]
enum AccountCommand {
    Deposit(i64),
    Withdraw(i64),
}

#[domain_event(version = 1)]
enum AccountEvent {
    #[event(event_type = "account.deposited")]
    Deposited { amount: i64 },
    #[event(event_type = "account.withdrawn")]
    Withdrawn { amount: i64 },
}

impl Aggregate for BankAccount {
    const TYPE: &'static str = "bank_account";
    type Command = AccountCommand;
    type Event = AccountEvent;
    type Error = DomainError;

    fn execute(&self, cmd: AccountCommand) -> Result<Vec<AccountEvent>, DomainError> {
        match cmd {
            AccountCommand::Deposit(n) if n > 0 => Ok(vec![AccountEvent::Deposited {
                id: uuid::Uuid::new_v4().to_string(),
                aggregate_version: self.version().next().value(),
                amount: n,
            }]),
            AccountCommand::Withdraw(n) if n > 0 && self.balance >= n => {
                Ok(vec![AccountEvent::Withdrawn {
                    id: uuid::Uuid::new_v4().to_string(),
                    aggregate_version: self.version().next().value(),
                    amount: n,
                }])
            }
            _ => Err(DomainError::invalid_command("invalid amount or insufficient")),
        }
    }

    fn apply(&mut self, event: &AccountEvent) {
        match event {
            AccountEvent::Deposited { aggregate_version, amount, .. } => {
                self.balance += amount;
                self.version = Version::from_value(*aggregate_version);
            }
            AccountEvent::Withdrawn { aggregate_version, amount, .. } => {
                self.balance -= amount;
                self.version = Version::from_value(*aggregate_version);
            }
        }
    }
}
```

## Build, test, run examples

Requires Rust 1.85+ (workspace uses Rust 2024 edition).

```bash
cargo build --workspace
cargo test  --workspace --all-features

# Domain examples
cargo run -p eventide-domain --example event_upcasting
cargo run -p eventide-domain --example event_repository
cargo run -p eventide-domain --example snapshot_repository
cargo run -p eventide-domain --example eventing_inmemory

# Application examples
cargo run -p eventide-application --example inmemory_command_bus
cargo run -p eventide-application --example inmemory_query_bus
```

## Why eventide

- **Hexagonal-friendly.** The domain layer defines abstractions (`EventRepository`, `SnapshotRepository`, `AggregateRepository`); infrastructure crates implement them.
- **Event sourcing built in.** Aggregates emit events, the engine persists them, and an upcasting chain handles schema evolution without touching historical data.
- **CQRS by default.** Separate `CommandBus` and `QueryBus` with type-safe handler registration.
- **Procedural macros that disappear.** `#[entity]`, `#[entity_id]`, `#[domain_event]`, and `#[value_object]` remove the boilerplate so business invariants stay readable.
- **Async-native.** Tokio-based event engine with cooperative dispatch, retry, and dead-letter handling.
- **No vendor lock-in.** The domain crate has zero database dependencies; pick (or write) the infrastructure adapter you need.

## Feature flags (umbrella crate)

| Flag           | Default | What it does                                                                                       |
|----------------|---------|----------------------------------------------------------------------------------------------------|
| `eventing`     | yes     | Asynchronous event subsystem (bus / engine / dispatcher / reclaimer) on top of `tokio`.            |
| `macros`       | yes     | Re-export `eventide-macros` as `eventide::macros`.                                                 |
| `application`  | yes     | Re-export `eventide-application` as `eventide::application`.                                       |
| `infra-sqlx`   | no      | Opt-in `sqlx` conversions on serialized events / snapshots for Postgres-backed event stores.       |

Pick individual sub-crates instead of the umbrella when you want finer control:

```toml
[dependencies]
eventide-domain = "0.1"
eventide-macros = "0.1"
# Skip eventide-application if you do not need the bus.
```

## Documentation

- API reference: <https://docs.rs/eventide>
- Per-layer docs:
  - <https://docs.rs/eventide-domain>
  - <https://docs.rs/eventide-application>
  - <https://docs.rs/eventide-macros>

Each sub-crate also ships its own `README.md` with focused documentation.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

## Contributing

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
