//! 聚合（Aggregate）抽象
//!
//! 约束一个聚合的核心行为：
//! - `execute` 将命令转换为事件（不改变状态）；
//! - `apply` 将事件投影到状态（改变状态）；
//! - 通过 `Entity` 约束聚合具备标识与版本。
use std::error::Error;

use serde::{Serialize, de::DeserializeOwned};

use crate::{domain_event::DomainEvent, entity::Entity};

/// 聚合根接口
pub trait Aggregate: Entity + Default + Serialize + DeserializeOwned + Send + Sync {
    const TYPE: &'static str;

    /// 该聚合支持的命令类型
    type Command;
    /// 该聚合产生的领域事件类型
    type Event: DomainEvent;
    /// 命令执行或持久化环节的错误类型
    type Error: Error + Send + Sync + 'static;

    /// 执行命令，返回产生的事件列表
    fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error>;

    /// 应用事件，更新聚合状态
    fn apply(&mut self, event: &Self::Event);
}

#[cfg(test)]
mod tests {
    use ddd_macros::{domain_event, entity};
    use serde::{Deserialize, Serialize};

    use super::Aggregate;
    use crate::{
        domain_event::{DomainEvent, EventContext, EventEnvelope},
        entity::Entity,
        error::DomainError,
        value_object::Version,
    };

    #[entity]
    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    struct Counter {
        value: i32,
    }

    #[derive(Debug)]
    enum CounterCommand {
        Add { amount: i32 },
        Sub { amount: i32 },
    }

    #[domain_event(version = 1)]
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    enum CounterEvent {
        Added { amount: i32 },
        Subtracted { amount: i32 },
    }

    impl Aggregate for Counter {
        const TYPE: &'static str = "counter";
        type Command = CounterCommand;
        type Event = CounterEvent;
        type Error = DomainError;

        fn execute(&self, command: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
            match command {
                CounterCommand::Add { amount } => {
                    if amount <= 0 {
                        return Err(DomainError::invalid_command("amount must be > 0"));
                    }
                    Ok(vec![CounterEvent::Added {
                        id: ulid::Ulid::new().to_string(),
                        aggregate_version: self.version().next(),
                        amount,
                    }])
                }
                CounterCommand::Sub { amount } => {
                    if amount <= 0 {
                        return Err(DomainError::invalid_command("amount must be > 0"));
                    }
                    if self.value < amount {
                        return Err(DomainError::invalid_state("insufficient"));
                    }
                    Ok(vec![CounterEvent::Subtracted {
                        id: ulid::Ulid::new().to_string(),
                        aggregate_version: self.version().next(),
                        amount,
                    }])
                }
            }
        }

        fn apply(&mut self, event: &Self::Event) {
            match event {
                CounterEvent::Added {
                    aggregate_version,
                    amount,
                    ..
                } => {
                    self.value += *amount;
                    self.version = *aggregate_version;
                }
                CounterEvent::Subtracted {
                    aggregate_version,
                    amount,
                    ..
                } => {
                    self.value -= *amount;
                    self.version = *aggregate_version;
                }
            }
        }
    }

    #[tokio::test]
    async fn aggregate_lifecycle_create_execute_apply_envelope() {
        let id = "c-1".to_string();
        let agg = Counter::new(id.clone(), Version::new());
        assert_eq!(agg.id(), &id);
        assert_eq!(agg.version(), Version::new());
        assert_eq!(agg.value, 0);

        // 执行加法命令 -> 产生事件
        let events = agg.execute(CounterCommand::Add { amount: 3 }).unwrap();
        assert_eq!(events.len(), 1);
        match &events[0] {
            CounterEvent::Added {
                aggregate_version,
                amount,
                ..
            } => {
                assert_eq!(*aggregate_version, Version::from_value(1));
                assert_eq!(*amount, 3);
            }
            _ => panic!("unexpected event"),
        }

        // 应用事件到聚合
        let mut agg2 = agg.clone();
        for e in &events {
            agg2.apply(e);
        }
        assert_eq!(agg2.version(), Version::from_value(1));
        assert_eq!(agg2.value, 3);

        // 继续执行/应用（按顺序执行并逐步提升版本）
        let ev2 = agg2.execute(CounterCommand::Add { amount: 2 }).unwrap();
        let mut agg3 = agg2.clone();
        for e in &ev2 {
            agg3.apply(e);
        }
        let ev3 = agg3.execute(CounterCommand::Sub { amount: 1 }).unwrap();
        for e in &ev3 {
            agg3.apply(e);
        }
        assert_eq!(agg3.version(), Version::from_value(3));
        assert_eq!(agg3.value, 4);

        // 事件信封封装（用于持久化前）
        let ctx = EventContext::default();
        let envelopes: Vec<EventEnvelope<Counter>> = vec![EventEnvelope::new(
            agg3.id(),
            CounterEvent::Added {
                id: ulid::Ulid::new().to_string(),
                aggregate_version: agg3.version().next(),
                amount: 10,
            },
            ctx.clone(),
        )];
        assert_eq!(envelopes.len(), 1);
        assert_eq!(
            envelopes[0].payload.aggregate_version(),
            agg3.version().next()
        );
    }

    #[test]
    fn invalid_commands_should_error() {
        let agg = Counter::new("c-2".to_string(), Version::new());
        use crate::error::ErrorKind;

        let err = agg.execute(CounterCommand::Sub { amount: 1 }).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidState);

        let err = agg.execute(CounterCommand::Add { amount: 0 }).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::InvalidCommand);
    }
}
