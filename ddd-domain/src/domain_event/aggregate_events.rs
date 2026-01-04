use std::{ops::Deref, slice::Iter, vec::IntoIter};

use chrono::{DateTime, Utc};

use super::event_envelope::EventEnvelope;
use crate::aggregate::Aggregate;

/// 聚合事件集合，按时间顺序排列，便于获取创建/修改者与时间等信息
pub struct AggregateEvents<A>
where
    A: Aggregate,
{
    events: Vec<EventEnvelope<A>>,
}

impl<A> AggregateEvents<A>
where
    A: Aggregate,
{
    pub fn new(events: Vec<EventEnvelope<A>>) -> Self {
        Self { events }
    }

    /// 获取创建者ID（第一个事件的触发者）
    pub fn created_by(&self) -> Option<String> {
        self.events
            .first()
            .and_then(|e| e.context.actor_id().map(|s| s.to_string()))
    }

    /// 获取最后修改者ID（最后一个事件的触发者）
    pub fn last_modified_by(&self) -> Option<String> {
        self.events
            .last()
            .and_then(|e| e.context.actor_id().map(|s| s.to_string()))
    }

    /// 获取创建时间（第一个事件的发生时间）
    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        self.events.first().map(|e| *e.metadata.occurred_at())
    }

    /// 获取最后修改时间（最后一个事件的发生时间）
    pub fn last_modified_at(&self) -> Option<DateTime<Utc>> {
        self.events.last().map(|e| *e.metadata.occurred_at())
    }

    /// 获取事件列表的不可变引用
    pub fn events(&self) -> &[EventEnvelope<A>] {
        &self.events
    }

    /// 获取事件数量
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 判断是否为空
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 迭代事件引用（不消费 AggregateEvents）
    pub fn iter(&self) -> Iter<'_, EventEnvelope<A>> {
        self.events.iter()
    }
}

impl<A> IntoIterator for AggregateEvents<A>
where
    A: Aggregate,
{
    type Item = EventEnvelope<A>;
    type IntoIter = IntoIter<EventEnvelope<A>>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.into_iter()
    }
}

impl<'a, A> IntoIterator for &'a AggregateEvents<A>
where
    A: Aggregate,
{
    type Item = &'a EventEnvelope<A>;
    type IntoIter = Iter<'a, EventEnvelope<A>>;

    fn into_iter(self) -> Self::IntoIter {
        self.events.iter()
    }
}

impl<A> Deref for AggregateEvents<A>
where
    A: Aggregate,
{
    type Target = [EventEnvelope<A>];

    fn deref(&self) -> &Self::Target {
        &self.events
    }
}
