//! 快照持久化模型（SerializedSnapshot）
//!
//! 定义聚合快照在持久化层的标准形态与与聚合实例之间的转换。
use bon::Builder;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    aggregate::Aggregate,
    error::{DomainError, DomainResult as Result},
};

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

    /// 将快照反序列化为聚合实例
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

    /// 从聚合实例创建快照
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
