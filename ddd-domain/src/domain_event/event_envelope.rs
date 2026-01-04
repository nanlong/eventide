use chrono::Utc;

use super::{event_context::EventContext, metadata::Metadata};
use crate::aggregate::Aggregate;

/// 事件信封，包含事件载荷、元数据与业务上下文
#[derive(Debug, Clone)]
pub struct EventEnvelope<A>
where
    A: Aggregate,
{
    pub metadata: Metadata,
    pub payload: A::Event,
    pub context: EventContext,
}

impl<A> EventEnvelope<A>
where
    A: Aggregate,
{
    pub fn new(aggregate_id: &A::Id, payload: A::Event, context: EventContext) -> Self {
        let metadata = Metadata::builder()
            .aggregate_id(aggregate_id.to_string())
            .aggregate_type(A::TYPE.to_string())
            .occurred_at(Utc::now())
            .build();

        Self {
            metadata,
            payload,
            context,
        }
    }
}
