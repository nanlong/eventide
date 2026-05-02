# eventide

[![Crates.io](https://img.shields.io/crates/v/eventide.svg)](https://crates.io/crates/eventide)
[![Documentation](https://docs.rs/eventide/badge.svg)](https://docs.rs/eventide)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license--许可证)

> English version: [README.md](README.md)

面向 **领域驱动设计（DDD）** 的 Rust 工具集，原生支持 **事件溯源（Event Sourcing）** 与 **CQRS**。

`eventide` 是顶层 umbrella crate，重导出工具集的三大基础块。引入这一个依赖即可获得全部能力。

| 模块            | 来源 crate                                                  | 职责                                                                |
|-----------------|-------------------------------------------------------------|---------------------------------------------------------------------|
| `domain`        | [`eventide-domain`](https://crates.io/crates/eventide-domain) | 聚合、实体、值对象、领域事件、仓储抽象。                              |
| `application`   | [`eventide-application`](https://crates.io/crates/eventide-application) | 命令总线、查询总线、处理器、应用上下文。                              |
| `macros`        | [`eventide-macros`](https://crates.io/crates/eventide-macros) | `#[entity]`、`#[entity_id]`、`#[domain_event]`、`#[value_object]`。  |

## 快速开始

在 `Cargo.toml` 中添加：

```toml
[dependencies]
eventide = "0.1"
```

> 一条依赖即可覆盖常用路径。Umbrella crate 在 `eventide` 之上再 re-export
> 了所有运行时符号：
>
> - `serde`：宏生成的 `Serialize` / `Deserialize` 派生通过 `eventide` 内部的
>   `serde` re-export 解析，所以使用 `#[entity]`、`#[entity_id]`、
>   `#[domain_event]`、`#[value_object]` 无需直接依赖 `serde`。
> - `eventide::tokio`：Tokio 运行时（如 `#[eventide::tokio::main]`、
>   `eventide::tokio::spawn`）。在 `eventing` feature 启用时可用（默认开）。
> - `eventide::async_trait`：在自己实现的 trait（如 `DomainService`、
>   `EventHandler`）上直接写 `#[async_trait]`，无需直接依赖 `async-trait`。
>
> 仅当你在 re-export 之外另行使用 `serde`/`tokio`/`async-trait`
> （如额外派生、自定义 `#[serde(...)]` 属性、运行时高级特性）时才需要在
> 自己的 `Cargo.toml` 中显式声明。

定义一个聚合：

```rust
use eventide::prelude::*;

#[entity_id]
struct UserId(String);

#[entity(id = UserId)]
#[derive(Clone)]
struct User {
    name: String,
}

#[derive(Debug)]
enum UserCommand {
    Rename { name: String },
}

#[domain_event(version = 1)]
enum UserEvent {
    #[event(event_type = "user.renamed")]
    Renamed { name: String },
}

impl Aggregate for User {
    const TYPE: &'static str = "user";
    type Command = UserCommand;
    type Event = UserEvent;
    type Error = DomainError;

    fn execute(&self, cmd: UserCommand) -> Result<Vec<UserEvent>, DomainError> {
        match cmd {
            UserCommand::Rename { name } if !name.is_empty() => Ok(vec![UserEvent::Renamed {
                id: uuid::Uuid::new_v4().to_string(),
                aggregate_version: self.version().next().value(),
                name,
            }]),
            _ => Err(DomainError::invalid_value("name must not be empty")),
        }
    }

    fn apply(&mut self, event: &UserEvent) {
        match event {
            UserEvent::Renamed { aggregate_version, name, .. } => {
                self.name = name.clone();
                self.version = Version::from_value(*aggregate_version);
            }
        }
    }
}
```

## 特性开关（feature flags）

默认全部开启。通过 `default-features = false` 选择性禁用，精简依赖树。

| Flag           | 默认 | 作用                                                                                  |
|----------------|------|---------------------------------------------------------------------------------------|
| `eventing`     | 开   | 基于 `tokio` 的异步事件子系统（总线 / 引擎 / 投递 / 回收）。                            |
| `macros`       | 开   | 重导出 `eventide-macros` 为 `eventide::macros`。                                       |
| `application`  | 开   | 重导出 `eventide-application` 为 `eventide::application`。                             |
| `infra-sqlx`   | 关   | 为序列化事件/快照启用 `sqlx` 转换，便于对接 Postgres 事件存储。                         |

示例：仅依赖领域层

```toml
[dependencies]
eventide = { version = "0.1", default-features = false }
```

## 分层架构

`eventide` 拆成多个独立 crate，按需取用。依赖方向严格单向：

```text
eventide-application  →  eventide-domain  ←  eventide-macros
                                 ↑
                                 └──── (optional) infra-sqlx
```

领域层不依赖应用层与基础设施层，业务逻辑保持纯净、易于测试。

## 为什么选 eventide

- **六边形架构友好**：领域层只定义抽象（`EventRepository`、`SnapshotRepository`、`AggregateRepository`），基础设施层实现这些抽象。
- **内建事件溯源**：聚合产出事件，引擎负责持久化；事件升级链（`EventUpcasterChain`）处理 schema 演进，不动历史数据。
- **CQRS 默认开启**：`CommandBus` 与 `QueryBus` 分离，注册即类型安全。
- **过程宏让样板代码消失**：`#[entity]`、`#[entity_id]`、`#[domain_event]`、`#[value_object]` 让业务不变量保持清晰。
- **异步原生**：基于 Tokio 的事件引擎，支持协作式分发、重试与死信处理。
- **无供应商锁定**：领域层零数据库依赖，自由选择（或自行实现）基础设施适配器。

## 文档

- API 参考：<https://docs.rs/eventide>
- 仓库总览：[GitHub](https://github.com/nanlong/eventide)
- 示例：参见各子 crate 的 `examples/` 目录。

## License / 许可证

双授权（任选其一）：

- Apache License 2.0 ([LICENSE-APACHE](../LICENSE-APACHE) 或 <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](../LICENSE-MIT) 或 <http://opensource.org/licenses/MIT>)

## 贡献

除非你明确说明否则你提交的任何贡献，按 Apache-2.0 第 5 节定义，将以上述双授权方式纳入项目，不附加任何额外条款或条件。
