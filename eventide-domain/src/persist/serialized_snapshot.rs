//! Persistence-layer model for aggregate snapshots (`SerializedSnapshot`).
//!
//! Defines the canonical shape an aggregate takes when serialized to a
//! snapshot store, together with bidirectional helpers to convert between
//! a live [`Aggregate`] instance and its on-disk form.
//!
//! Snapshots are used by [`SnapshotPolicyRepo`](super::SnapshotPolicyRepo)
//! to bound the cost of replaying long event histories: instead of always
//! starting from version zero, a load can begin at the most recent
//! snapshot and only replay events recorded after it.
//!
use crate::{
    aggregate::Aggregate,
    error::{DomainError, DomainResult as Result},
};
use bon::Builder;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Storage-layer representation of an aggregate snapshot.
///
/// The structure is intentionally minimal: an identity triple
/// (`aggregate_id`, `aggregate_type`, `aggregate_version`) plus the
/// JSON-encoded aggregate state. Adapters typically persist this directly
/// as one row per aggregate (latest snapshot only) or one row per
/// aggregate + version (full snapshot history).
#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
pub struct SerializedSnapshot {
    aggregate_id: String,
    aggregate_type: String,
    aggregate_version: usize,
    payload: Value,
}

impl SerializedSnapshot {
    pub fn aggregate_id(&self) -> &str {
        &self.aggregate_id
    }

    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    pub fn aggregate_version(&self) -> usize {
        self.aggregate_version
    }

    pub fn payload(&self) -> &Value {
        &self.payload
    }

    /// Deserializes the snapshot back into a typed aggregate instance.
    ///
    /// The conversion verifies that `aggregate_type` matches the requested
    /// `A::TYPE` before attempting JSON deserialization, which catches
    /// programming mistakes (a snapshot of `Order` being read as `Customer`)
    /// early and with a clear error.
    ///
    /// # Errors
    ///
    /// - Returns [`DomainError::type_mismatch`] when the stored
    ///   `aggregate_type` does not equal `A::TYPE`.
    /// - Returns a JSON-conversion [`DomainError`] when `payload` cannot be
    ///   decoded into `A`.
    pub fn to_aggregate<A>(&self) -> Result<A>
    where
        A: Aggregate,
    {
        if A::TYPE != self.aggregate_type {
            return Err(DomainError::type_mismatch(
                A::TYPE,
                self.aggregate_type.as_str(),
            ));
        }

        let aggregate = serde_json::from_value(self.payload.clone())?;
        Ok(aggregate)
    }

    /// Serializes a live aggregate into a snapshot ready for storage.
    ///
    /// The aggregate's id, type tag and current version are copied into the
    /// dedicated fields, and the entire aggregate is JSON-encoded into
    /// `payload`. The resulting `SerializedSnapshot` is what
    /// [`SnapshotRepository::save`](super::SnapshotRepository::save) ultimately
    /// receives.
    ///
    /// # Errors
    ///
    /// Returns a [`DomainError`] when the aggregate cannot be encoded into
    /// JSON.
    pub fn from_aggregate<A>(aggregate: &A) -> Result<Self>
    where
        A: Aggregate,
    {
        Ok(Self {
            aggregate_id: aggregate.id().to_string(),
            aggregate_type: A::TYPE.to_string(),
            aggregate_version: aggregate.version().value(),
            payload: serde_json::to_value(aggregate)?,
        })
    }
}
