//! 快照仓储协议与策略
//!
//! 定义聚合快照读写接口与简单的落盘策略（按版本间隔）。
use std::sync::Arc;

use async_trait::async_trait;

use crate::{aggregate::Aggregate, error::DomainResult as Result, persist::SerializedSnapshot};

#[async_trait]
pub trait SnapshotRepository: Send + Sync {
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> Result<Option<SerializedSnapshot>>;

    async fn save<A: Aggregate>(&self, aggregate: &A) -> Result<()>;
}

#[async_trait]
impl<T> SnapshotRepository for Arc<T>
where
    T: SnapshotRepository + ?Sized,
{
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> Result<Option<SerializedSnapshot>> {
        (**self).get_snapshot::<A>(aggregate_id, version).await
    }

    async fn save<A: Aggregate>(&self, aggregate: &A) -> Result<()> {
        (**self).save::<A>(aggregate).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotPolicy {
    Never,
    Every(usize),
}

impl SnapshotPolicy {
    pub fn should_snapshot(&self, version: usize) -> bool {
        match self {
            SnapshotPolicy::Never => false,
            SnapshotPolicy::Every(interval) => {
                let interval = (*interval).max(1);
                version > 0 && version.is_multiple_of(interval)
            }
        }
    }
}

/// SnapshotRepository 的装饰器，根据策略决定是否落盘快照
pub struct SnapshotRepositoryWithPolicy<R> {
    inner: R,
    policy: SnapshotPolicy,
}

impl<R> SnapshotRepositoryWithPolicy<R> {
    pub fn new(inner: R, policy: SnapshotPolicy) -> Self {
        Self { inner, policy }
    }
}

#[async_trait]
impl<R> SnapshotRepository for SnapshotRepositoryWithPolicy<R>
where
    R: SnapshotRepository + Send + Sync,
{
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        version: Option<usize>,
    ) -> Result<Option<SerializedSnapshot>> {
        self.inner.get_snapshot::<A>(aggregate_id, version).await
    }

    async fn save<A: Aggregate>(&self, aggregate: &A) -> Result<()> {
        if !self.policy.should_snapshot(aggregate.version().value()) {
            return Ok(());
        }

        self.inner.save::<A>(aggregate).await
    }
}
