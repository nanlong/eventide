# eventide-domain

[![Crates.io](https://img.shields.io/crates/v/eventide-domain.svg)](https://crates.io/crates/eventide-domain)
[![Documentation](https://docs.rs/eventide-domain/badge.svg)](https://docs.rs/eventide-domain)

> English version: [README.md](README.md)

领域层（Domain Layer）：承载核心业务模型与规则，独立于应用层与基础设施。

## 核心抽象

- `Entity` / `Aggregate`
  - `Entity`：带标识，提供 `new/id/version`
  - `Aggregate`：聚合根接口（`TYPE`，`Command/Event/Error`，`execute/apply`）
- `DomainEvent`
  - 事件 ID、类型、版本、聚合版本等元信息
- `EventEnvelope<A>` / `AggregateEvents<A>`
  - 携带 `Metadata` 与 `EventContext`（correlation/causation/actor_*）
- `persist::*`
  - 事件/快照序列化模型与仓储接口（`EventRepository`、`SnapshotRepository` 等）
  - 通用聚合仓储实现：
    - `EventSourcedRepo<E>`：仅事件溯源，依赖 `EventRepository`
    - `SnapshotPolicyRepo<E, S>`：事件溯源 + 快照，依赖 `EventRepository` 与 `SnapshotRepository`
    - `SnapshotRepositoryWithPolicy<R>` 与 `SnapshotPolicy`：以装饰器按策略（如 `Every(n)`）决定是否落盘快照
- `eventing::*`
  - 事件总线、投递器、回收器等抽象

## 过程宏配合

本仓库配套的 `eventide-macros` 能生成常用样板：

- `#[entity]`：为具名字段结构体追加 `id`/`version` 并实现 `Entity`
- `#[domain_event]`：为具名字段变体的枚举追加 `id`/`aggregate_version` 并实现 `DomainEvent`

为了在本 crate 的测试与宏展开中解析到绝对路径 `::eventide_domain::...`，库根导出了自引用别名：

```rust
// src/lib.rs
extern crate self as eventide_domain;
```

## 仓储实现

持久化模块导出以下类型与约定（自 `eventide_domain::persist`）：

- `AggregateRepository<A>`：聚合仓储 trait（`load`/`save`）。
- `EventSourcedRepo<E>`：仅事件溯源的通用实现；自动按上抬链反序列化历史事件。
- `SnapshotPolicyRepo<E, S>`：先取快照再重放增量事件；结合 `SnapshotRepositoryWithPolicy` 自动评估快照策略。
- `SnapshotRepositoryWithPolicy<R>`：在保存时根据 `SnapshotPolicy` 判定是否写入快照。

示例（简化 API 演示）：

```rust
use std::sync::Arc;
use eventide_domain::event_upcaster::EventUpcasterChain;
use eventide_domain::persist::{EventSourcedRepo, SnapshotPolicyRepo, SnapshotRepositoryWithPolicy, SnapshotPolicy};

let upcasters = Arc::new(EventUpcasterChain::default());
let event_repo = Arc::new(MyEventRepo::new());
let snapshot_repo = Arc::new(SnapshotRepositoryWithPolicy::new(MySnapshotRepo::new(), SnapshotPolicy::Every(100)));

// 仅事件溯源
let es_repo = EventSourcedRepo::new(event_repo.clone(), upcasters.clone());

// 事件 + 快照策略
let ss_repo = SnapshotPolicyRepo::new(event_repo.clone(), snapshot_repo.clone(), upcasters.clone());
```

## 示例与运行

领域层包含多个示例：事件升级（Upcaster）链、聚合命令/事件应用、快照序列化等。

```bash
cargo build   -p eventide-domain
cargo test    -p eventide-domain

# 事件上抬 + 通用仓储（EventSourcedRepo / SnapshotPolicyRepo）
cargo run -p eventide-domain --example event_upcasting

# 事件仓储接口示例（自定义实现 + AggregateRepository）
cargo run -p eventide-domain --example event_repository

# 快照仓储接口与策略示例
cargo run -p eventide-domain --example snapshot_repository

# 内存事件引擎（总线/投递/回收）示例
cargo run -p eventide-domain --example eventing_inmemory
```

## 与应用层/基础设施的关系

- 领域层不依赖应用层与基础设施；
- 应用层依赖领域层；
- 基础设施（如事件存储、消息总线）应依赖应用层接口与领域模型；

建议在后续 `eventide-infrastructure` 中实现事件/快照仓储与消息总线，领域层保持纯净。
