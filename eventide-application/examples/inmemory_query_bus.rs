//! Example: in-process query dispatch with [`InMemoryQueryBus`].
//!
//! This example wires up two query/handler pairs (`GetUser`/`UserDto` and
//! `ListUsers`/`UsersDto`), dispatches each through the in-memory bus, and
//! demonstrates the `HANDLER_NOT_FOUND` error path by dispatching a query
//! type that was never registered.

use async_trait::async_trait;
use eventide_application::InMemoryQueryBus;
use eventide_application::context::AppContext;
use eventide_application::error::AppError;
use eventide_application::query_bus::QueryBus;
use eventide_application::query_handler::QueryHandler;
use eventide_domain::domain_event::EventContext;
use serde::Serialize;
use std::sync::Arc;

#[derive(Debug)]
struct GetUser {
    id: u32,
}

#[derive(Debug, Serialize)]
struct UserDto {
    id: u32,
    name: String,
}

struct GetUserHandler;

#[async_trait]
impl QueryHandler<GetUser, UserDto> for GetUserHandler {
    async fn handle(&self, _ctx: &AppContext, q: GetUser) -> Result<UserDto, AppError> {
        Ok(UserDto {
            id: q.id,
            name: "Alice".into(),
        })
    }
}

#[derive(Debug)]
struct ListUsers;

#[derive(Debug, Serialize)]
struct UsersDto(Vec<UserDto>);

struct ListUsersHandler;

#[async_trait]
impl QueryHandler<ListUsers, UsersDto> for ListUsersHandler {
    async fn handle(&self, _ctx: &AppContext, _q: ListUsers) -> Result<UsersDto, AppError> {
        Ok(UsersDto(vec![
            UserDto {
                id: 1,
                name: "Alice".into(),
            },
            UserDto {
                id: 2,
                name: "Bob".into(),
            },
        ]))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bus = InMemoryQueryBus::new();
    bus.register::<GetUser, UserDto, _>(Arc::new(GetUserHandler))?;
    bus.register::<ListUsers, UsersDto, _>(Arc::new(ListUsersHandler))?;

    let ctx = AppContext {
        event_context: EventContext::builder()
            .maybe_correlation_id(Some("cor-2".into()))
            .maybe_causation_id(Some("cau-2".into()))
            .maybe_actor_type(Some("user".into()))
            .maybe_actor_id(Some("u-2".into()))
            .build(),
        idempotency_key: None,
    };
    let dto = bus
        .dispatch::<GetUser, UserDto>(&ctx, GetUser { id: 1 })
        .await?;

    println!("GetUser: id={}, name={}", dto.id, dto.name);

    let list = bus.dispatch::<ListUsers, UsersDto>(&ctx, ListUsers).await?;

    println!("ListUsers: count={}", list.0.len());

    // Dispatching an unregistered query type should yield a
    // HandlerNotFound error.
    #[derive(Debug)]
    struct GetOrders;

    if let Err(e) = bus.dispatch::<GetOrders, UsersDto>(&ctx, GetOrders).await {
        use eventide_domain::error::ErrorCode;
        if e.code() == "HANDLER_NOT_FOUND" {
            eprintln!("HandlerNotFound as expected for query: {}", e);
        }
    }

    eprintln!("Registered Queries: {:?}", bus.registered_queries());
    Ok(())
}
