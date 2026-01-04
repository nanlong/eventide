use ddd_domain::{domain_event::EventContext, persist::SerializedEvent};

/// 应用层上下文（Application Context）
///
/// 承载一次应用层调用（命令/查询）所需的横切信息，例如：
/// - 业务语境（`EventContext`）：关联追踪 `correlation_id`、因果链 `causation_id`、 执行者类型/ID
///   等；
/// - 幂等键（`idempotency_key`）：用于在基础设施层实现请求幂等（如 API 层重复提交保护）。
///
/// 典型用法：
/// ```rust
/// use ddd_application::context::AppContext;
/// use ddd_domain::domain_event::EventContext;
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
    /// 业务语境（链路追踪、审计主体、操作因果）
    pub event_context: EventContext,
    /// 幂等键（可选）：为空则由上层或基础设施决定是否参与幂等
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
