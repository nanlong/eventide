//! 事件持久化模型（SerializedEvent）
//!
//! 定义事件在持久化层的标准形态与在 `EventEnvelope` 间的转换，
//! 并提供批量序列化/反序列化与上抬组合的工具函数。
use bon::Builder;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    aggregate::Aggregate,
    domain_event::{DomainEvent, EventContext, EventEnvelope, Metadata},
    error::{DomainError, DomainResult},
    event_upcaster::EventUpcasterChain,
};

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct SerializedEvent {
    /// 事件唯一标识符
    event_id: String,
    /// 事件类型，用于区分不同的事件
    event_type: String,
    /// 事件版本，用于事件版本控制和升级
    event_version: usize,
    /// 全局事件位点，由存储层在持久化后赋值
    sequence_number: Option<i64>,
    /// 聚合 ID，标识事件所属的聚合根实例
    aggregate_id: String,
    /// 聚合类型，用于区分不同的聚合根
    aggregate_type: String,
    /// 聚合版本，用于乐观锁和并发控制
    aggregate_version: usize,
    /// 关联 ID，用于将多个事件关联到同一个业务操作
    correlation_id: Option<String>,
    /// 因果 ID，用于表示事件的触发来源
    causation_id: Option<String>,
    /// 触发事件的主体类型（如用户、系统等）
    actor_type: Option<String>,
    /// 触发事件的主体 ID
    actor_id: Option<String>,
    /// 事件发生时间
    occurred_at: DateTime<Utc>,
    /// 事件负载，存储事件的具体数据
    payload: Value,
    /// 业务上下文信息（冗余存储，便于查询）
    context: Value,
}

impl SerializedEvent {
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    pub fn event_version(&self) -> usize {
        self.event_version
    }

    pub fn sequence_number(&self) -> Option<i64> {
        self.sequence_number
    }

    pub fn aggregate_id(&self) -> &str {
        &self.aggregate_id
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub fn aggregate_version(&self) -> usize {
        self.aggregate_version
    }

    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    pub fn causation_id(&self) -> Option<&str> {
        self.causation_id.as_deref()
    }

    pub fn actor_type(&self) -> Option<&str> {
        self.actor_type.as_deref()
    }

    pub fn actor_id(&self) -> Option<&str> {
        self.actor_id.as_deref()
    }

    pub fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    pub fn payload(&self) -> &Value {
        &self.payload
    }

    pub fn context(&self) -> &Value {
        &self.context
    }
}

impl<A> TryFrom<&EventEnvelope<A>> for SerializedEvent
where
    A: Aggregate,
{
    type Error = serde_json::Error;

    fn try_from(envelope: &EventEnvelope<A>) -> Result<Self, Self::Error> {
        Ok(SerializedEvent {
            event_id: envelope.payload.event_id().to_string(),
            event_type: envelope.payload.event_type().to_string(),
            event_version: envelope.payload.event_version(),
            sequence_number: None,
            aggregate_id: envelope.metadata.aggregate_id().to_string(),
            aggregate_type: envelope.metadata.aggregate_type().to_string(),
            aggregate_version: envelope.payload.aggregate_version().value(),
            correlation_id: envelope.context.correlation_id().map(|s| s.to_string()),
            causation_id: envelope.context.causation_id().map(|s| s.to_string()),
            actor_type: envelope.context.actor_type().map(|s| s.to_string()),
            actor_id: envelope.context.actor_id().map(|s| s.to_string()),
            occurred_at: *envelope.metadata.occurred_at(),
            payload: serde_json::to_value(&envelope.payload)?,
            context: serde_json::to_value(&envelope.context)?,
        })
    }
}

impl<A> TryFrom<&SerializedEvent> for EventEnvelope<A>
where
    A: Aggregate,
{
    type Error = serde_json::Error;

    fn try_from(value: &SerializedEvent) -> Result<Self, Self::Error> {
        let metadata = Metadata::builder()
            .aggregate_id(value.aggregate_id.clone())
            .aggregate_type(value.aggregate_type.clone())
            .occurred_at(value.occurred_at)
            .build();

        let payload: A::Event = serde_json::from_value(value.payload.clone())?;

        let context: EventContext = serde_json::from_value(value.context.clone())?;

        Ok(EventEnvelope {
            metadata,
            payload,
            context,
        })
    }
}

pub fn serialize_events<A>(events: &[EventEnvelope<A>]) -> DomainResult<Vec<SerializedEvent>>
where
    A: Aggregate,
{
    let events = events
        .iter()
        .map(SerializedEvent::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(events)
}

pub fn deserialize_events<A>(
    upcaster_chain: &EventUpcasterChain,
    events: Vec<SerializedEvent>,
) -> DomainResult<Vec<EventEnvelope<A>>>
where
    A: Aggregate,
{
    let events = upcaster_chain.upcast_all(events)?;

    let events = events
        .iter()
        .map(EventEnvelope::try_from)
        .collect::<Result<Vec<_>, _>>()
        .map_err(DomainError::from)?;

    Ok(events)
}
