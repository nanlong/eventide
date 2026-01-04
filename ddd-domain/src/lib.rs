//! DDD 领域层基础库（ddd-domain）
//!
//! 提供以 DDD 为中心的通用抽象与构件，用于在应用中实现：
//! - 聚合（`aggregate`）与实体（`entity`）建模
//! - 领域事件（`domain_event`）与事件上抬（`event_upcaster`）
//! - 基于事件溯源与快照的仓储（`persist`）
//! - 事件系统（`eventing`）：总线、投递/回收器、引擎与处理器
//! - 规约（`specification`）与值对象（`value_object`）等通用模式
//!
//! 本 crate 尽量保持与存储与传输实现解耦，仅定义领域层接口与最小必要的错误类型，
//! 以便在不同基础设施（例如 Postgres、消息中间件等）上进行适配实现。
//!
//! 典型用法：
//! 1. 定义聚合、命令与事件，实现在 `Aggregate` 上的 `execute/apply`；
//! 2. 选择 `persist` 中的仓储接口并提供具体实现；
//! 3. 使用 `eventing` 构建事件引擎，连接总线与投递/回收组件；
//! 4. 通过 `AggregateRoot` 编排一条完整的命令到事件持久化的流程。
pub mod aggregate;
pub mod aggregate_root;
pub mod domain_event;
pub mod domain_service;
pub mod entity;
pub mod error;
pub mod event_upcaster;
#[cfg(feature = "eventing")]
pub mod eventing;
pub mod persist;
pub mod specification;
pub mod value_object;

// 允许在本 crate 内部通过 ::ddd_domain 进行自引用，
// 以便过程宏在本 crate 的单元测试中也能解析到 ::ddd_domain 路径。
extern crate self as ddd_domain;
