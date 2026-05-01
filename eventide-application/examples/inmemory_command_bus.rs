use std::sync::Arc;

use async_trait::async_trait;
use eventide_application::{
    InMemoryCommandBus, command_bus::CommandBus, command_handler::CommandHandler,
    context::AppContext, error::AppError,
};
use eventide_domain::domain_event::EventContext;

#[derive(Debug)]
struct CreateUser {
    name: String,
}

struct CreateUserHandler;

#[async_trait]
impl CommandHandler<CreateUser, ()> for CreateUserHandler {
    async fn handle(&self, _ctx: &AppContext, cmd: CreateUser) -> Result<(), AppError> {
        println!("CreateUser: name={}", cmd.name);
        Ok(())
    }
}

#[derive(Debug)]
struct DeleteUser {
    id: u32,
}

struct DeleteUserHandler;

#[async_trait]
impl CommandHandler<DeleteUser, ()> for DeleteUserHandler {
    async fn handle(&self, _ctx: &AppContext, cmd: DeleteUser) -> Result<(), AppError> {
        println!("DeleteUser: id={}", cmd.id);
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bus = InMemoryCommandBus::new();
    bus.register::<CreateUser, (), _>(Arc::new(CreateUserHandler))?;
    bus.register::<DeleteUser, (), _>(Arc::new(DeleteUserHandler))?;

    let ctx = AppContext {
        event_context: EventContext::builder()
            .maybe_correlation_id(Some("cor-1".into()))
            .maybe_causation_id(Some("cau-1".into()))
            .maybe_actor_type(Some("user".into()))
            .maybe_actor_id(Some("u-1".into()))
            .build(),
        idempotency_key: Some("idem-1".into()),
    };
    bus.dispatch(
        &ctx,
        CreateUser {
            name: "Alice".into(),
        },
    )
    .await?;
    bus.dispatch(&ctx, DeleteUser { id: 42 }).await?;

    // 未注册的命令 -> 返回 HandlerNotFound 错误
    #[allow(dead_code)]
    #[derive(Debug)]
    struct UpdateUser {
        id: u32,
        name: String,
    }

    let result: Result<(), AppError> = bus
        .dispatch(
            &ctx,
            UpdateUser {
                id: 7,
                name: "Eve".into(),
            },
        )
        .await;

    if let Err(e) = result {
        use eventide_domain::error::ErrorCode;
        if e.code() == "HANDLER_NOT_FOUND" {
            eprintln!("HandlerNotFound as expected for command: {}", e);
        }
    }

    eprintln!("Registered Commands: {:?}", bus.registered_commands());
    Ok(())
}
