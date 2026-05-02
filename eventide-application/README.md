# eventide-application

[![Crates.io](https://img.shields.io/crates/v/eventide-application.svg)](https://crates.io/crates/eventide-application)
[![Documentation](https://docs.rs/eventide-application/badge.svg)](https://docs.rs/eventide-application)

> 中文版本: [README.zh.md](README.zh.md)

Application layer of the [`eventide`](https://crates.io/crates/eventide)
DDD/CQRS toolkit. It orchestrates use cases and translates external
requests (HTTP / CLI / job runner) into domain commands and queries
without leaking infrastructure concerns.

## What is in here

| Type / module                       | Role                                                                             |
|-------------------------------------|----------------------------------------------------------------------------------|
| `CommandHandler<C>`                 | Handles a single command type with side effects (writes).                        |
| `QueryHandler<Q, R>`                | Handles a single query type and returns a typed result `R`.                      |
| `CommandBus`                        | Type-erased dispatch. Routes a command to its registered handler by `TypeId`.    |
| `QueryBus`                          | Same idea for queries, with a typed return value.                                |
| `InMemoryCommandBus` / `InMemoryQueryBus` | Concurrent reference implementations backed by `dashmap::DashMap`.        |
| `AppContext`                        | Cross-cutting context: `EventContext` (correlation / causation / actor) + `idempotency_key`. |
| `AppError`                          | `Domain` / `Validation` / `Authorization` / `Infra` / `HandlerNotFound` / `AggregateNotFound` / `TypeMismatch`. |

## Add it

```toml
[dependencies]
eventide-application = "0.1"
async-trait = "0.1"
tokio = { version = "1", features = ["full"] }
```

Or via the umbrella crate (recommended), which re-exports `tokio` and
`async_trait` so a single dependency is enough:

```toml
[dependencies]
eventide = "0.1"
```

> When using the umbrella crate, write `#[eventide::tokio::main]` and
> `use eventide::async_trait::async_trait;` instead of depending on
> `tokio` / `async-trait` directly.

## Command example

```rust
use async_trait::async_trait;
use eventide_application::command_bus::CommandBus;
use eventide_application::command_handler::CommandHandler;
use eventide_application::context::AppContext;
use eventide_application::InMemoryCommandBus;
use std::sync::Arc;

#[derive(Debug)]
struct CreateUser { name: String }

struct CreateUserHandler;

#[async_trait]
impl CommandHandler<CreateUser> for CreateUserHandler {
    async fn handle(
        &self,
        _ctx: &AppContext,
        _cmd: CreateUser,
    ) -> Result<(), eventide_application::error::AppError> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let bus = InMemoryCommandBus::new();
    bus.register::<CreateUser, _>(Arc::new(CreateUserHandler)).unwrap();

    bus.dispatch(&AppContext::default(), CreateUser { name: "Alice".into() })
        .await
        .unwrap();
}
```

## Query example

```rust
use async_trait::async_trait;
use eventide_application::context::AppContext;
use eventide_application::query_bus::QueryBus;
use eventide_application::query_handler::QueryHandler;
use eventide_application::InMemoryQueryBus;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug)]
struct GetUser { id: u32 }

#[derive(Debug, Serialize)]
struct UserDto { id: u32, name: String }

struct GetUserHandler;

#[async_trait]
impl QueryHandler<GetUser, UserDto> for GetUserHandler {
    async fn handle(
        &self,
        _ctx: &AppContext,
        q: GetUser,
    ) -> Result<UserDto, eventide_application::error::AppError> {
        Ok(UserDto { id: q.id, name: "Alice".into() })
    }
}

#[tokio::main]
async fn main() {
    let bus = InMemoryQueryBus::new();
    bus.register::<GetUser, UserDto, _>(Arc::new(GetUserHandler)).unwrap();

    let _ = bus
        .dispatch::<GetUser, UserDto>(&AppContext::default(), GetUser { id: 1 })
        .await;
}
```

## Concurrency notes

- Bus registries use `dashmap::DashMap`. Handlers are wrapped in `Arc` and
  cloned out before dispatch, so locks never span an `await`.
- `AppError::HandlerNotFound(name)` reports the missing handler with
  `std::any::type_name::<T>()`, keeping the message short and stable.
- `AppError::TypeMismatch` is a defensive error used when the registry is
  unexpectedly mutated to hold the wrong handler type.

## Run tests / examples

```bash
cargo build -p eventide-application
cargo test  -p eventide-application
cargo run   -p eventide-application --example inmemory_command_bus
cargo run   -p eventide-application --example inmemory_query_bus
```

## Layered architecture

```text
eventide-application  →  eventide-domain  ←  eventide-macros
```

Application code depends only on the domain layer. Concrete repositories,
event stores, and message buses live in your own infrastructure crate and
are wired into handlers through dependency injection.

## License

Licensed under either of [Apache-2.0](../LICENSE-APACHE) or
[MIT](../LICENSE-MIT) at your option.
