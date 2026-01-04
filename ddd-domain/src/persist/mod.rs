//! 持久化与事件溯源（persist）
//!
//! 定义事件仓储、快照仓储及其通用组合实现，支持：
//! - 事件持久化与按聚合查询（`EventRepository`）；
//! - 快照读写与策略（`SnapshotRepository`/`SnapshotPolicy`）；
//! - 事件上抬（Upcast）与反序列化（`deserialize_events`）；
//! - 纯事件或事件+快照的聚合仓储实现（`EventSourcedRepo`、`SnapshotPolicyRepo`）。
//!
//! 该模块聚焦协议与装配逻辑，具体存储后端（如 Postgres）由上层提供实现并注入。
mod aggregate_repository;
mod event_repository;
mod serialized_event;
mod serialized_snapshot;
mod snapshot_repository;

pub use aggregate_repository::{AggregateRepository, EventSourcedRepo, SnapshotPolicyRepo};
pub use event_repository::{EventRepository, EventRepositoryExt};
pub use serialized_event::{SerializedEvent, deserialize_events, serialize_events};
pub use serialized_snapshot::SerializedSnapshot;
pub use snapshot_repository::{SnapshotPolicy, SnapshotRepository, SnapshotRepositoryWithPolicy};
