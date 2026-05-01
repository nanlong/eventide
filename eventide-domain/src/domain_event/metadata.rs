use bon::Builder;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Technical metadata attached to every [`EventEnvelope`](super::EventEnvelope).
///
/// Where [`EventContext`](super::EventContext) carries *business-level*
/// propagation data (correlation, causation, actor), `Metadata` carries the
/// *technical* facts the persistence layer needs to route and persist an
/// event:
///
/// - which aggregate instance the event belongs to (`aggregate_id`),
/// - what kind of aggregate that is (`aggregate_type`),
/// - when the event happened in wall-clock terms (`occurred_at`).
///
/// Construct instances with [`Metadata::builder`].
#[derive(Builder, Default, Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    aggregate_id: String,
    aggregate_type: String,
    occurred_at: DateTime<Utc>,
}

impl Metadata {
    pub fn aggregate_id(&self) -> &str {
        &self.aggregate_id
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub fn occurred_at(&self) -> &DateTime<Utc> {
        &self.occurred_at
    }
}
