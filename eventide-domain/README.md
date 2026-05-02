# eventide-domain

[![Crates.io](https://img.shields.io/crates/v/eventide-domain.svg)](https://crates.io/crates/eventide-domain)
[![Documentation](https://docs.rs/eventide-domain/badge.svg)](https://docs.rs/eventide-domain)

> 中文版本: [README.zh.md](README.zh.md)

Domain layer of the [`eventide`](https://crates.io/crates/eventide)
DDD/CQRS toolkit. This crate ships the pure-Rust building blocks that
your business logic actually depends on — and *only* those. It has no
database driver, no message broker, and no HTTP framework.

## What is in here

| Module             | Highlights                                                                              |
|--------------------|------------------------------------------------------------------------------------------|
| `aggregate`        | `Aggregate` trait (`TYPE`, `Command`, `Event`, `Error`, `execute`, `apply`).             |
| `aggregate_root`   | `AggregateRoot<A, R>` orchestrating *load → execute → apply → save*.                     |
| `entity`           | `Entity` trait with `id` / `version` for optimistic concurrency.                         |
| `value_object`     | `ValueObject` marker trait and `Version` newtype.                                        |
| `domain_event`     | `DomainEvent` trait, `EventEnvelope`, `EventContext`, `Metadata`, `FieldChanged`.        |
| `event_upcaster`   | `EventUpcaster` + `EventUpcasterChain` for evolving event schemas.                       |
| `persist`          | `EventRepository`, `SnapshotRepository`, `AggregateRepository` and reusable adapters.    |
| `eventing`         | Optional async event subsystem (bus / engine / dispatcher / reclaimer) on `tokio`.       |
| `specification`    | `Specification<T>` combinator pattern (`and`, `or`, `not`).                              |
| `error`            | `DomainError`, `ErrorKind`, `ErrorCode` for typed domain failures.                       |

## Add it

```toml
[dependencies]
eventide-domain = "0.1"
```

The macro-generated `Serialize` / `Deserialize` derives route through
this crate's internal `serde` re-export, so `#[entity]`, `#[entity_id]`,
`#[domain_event]` and `#[value_object]` work without a direct `serde`
dependency. The crate also re-exports `eventide_domain::async_trait`
(write `#[async_trait]` on your own trait impls) and
`eventide_domain::tokio` (runtime, gated on the `eventing` feature).
Add `serde` / `tokio` / `async-trait` to your own `Cargo.toml` only
when you reach for them outside these re-exports (extra derives,
custom `#[serde(...)]` attributes, advanced runtime features).

Or pull everything in via the umbrella crate:

```toml
[dependencies]
eventide = "0.1"
```

## Feature flags

| Flag         | Default | What it enables                                                                                            |
|--------------|---------|------------------------------------------------------------------------------------------------------------|
| `eventing`   | yes     | Asynchronous event subsystem (bus / engine / dispatcher / reclaimer). Pulls in `tokio` and `futures-util`. |
| `infra-sqlx` | no      | `sqlx` conversions on `SerializedEvent` / `SerializedSnapshot` for Postgres-backed event stores.           |

Disable `eventing` when you only need pure aggregate modelling:

```toml
[dependencies]
eventide-domain = { version = "0.1", default-features = false }
```

## Macros

The companion crate [`eventide-macros`](https://crates.io/crates/eventide-macros)
generates the boilerplate (`Entity`, `DomainEvent`, etc.). The generated
code uses absolute paths `::eventide_domain::...`, and this crate exposes
a self-alias so the macros also resolve inside its own tests:

```rust
// src/lib.rs
extern crate self as eventide_domain;
```

## Repository helpers

```rust
use std::sync::Arc;
use eventide_domain::event_upcaster::EventUpcasterChain;
use eventide_domain::persist::{
    EventSourcedRepo, SnapshotPolicy, SnapshotPolicyRepo, SnapshotRepositoryWithPolicy,
};

let upcasters = Arc::new(EventUpcasterChain::default());
let event_repo = Arc::new(MyEventRepo::new());
let snapshot_repo = Arc::new(SnapshotRepositoryWithPolicy::new(
    MySnapshotRepo::new(),
    SnapshotPolicy::Every(100),
));

// Pure event sourcing.
let es = EventSourcedRepo::new(event_repo.clone(), upcasters.clone());

// Event sourcing + snapshots.
let snap = SnapshotPolicyRepo::new(event_repo.clone(), snapshot_repo, upcasters);
```

## Examples

```bash
cargo run -p eventide-domain --example event_upcasting       # upcaster chain + repos
cargo run -p eventide-domain --example event_repository      # custom EventRepository
cargo run -p eventide-domain --example snapshot_repository   # snapshot policies
cargo run -p eventide-domain --example eventing_inmemory     # async event engine
cargo run -p eventide-domain --example account_aggregate     # full aggregate flow
```

## Layered architecture

`eventide-domain` is the centre of the dependency graph:

```text
eventide-application  →  eventide-domain  ←  eventide-macros
                                 ↑
                                 └──── (optional) infra-sqlx
```

The domain crate intentionally never imports application or
infrastructure types, which keeps your business logic pure and trivially
testable.

## License

Licensed under either of [Apache-2.0](../LICENSE-APACHE) or
[MIT](../LICENSE-MIT) at your option.
