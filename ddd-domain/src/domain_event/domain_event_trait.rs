use std::fmt;

use serde::{Serialize, de::DeserializeOwned};

use crate::value_object::Version;

/// 领域事件载荷需要满足的通用能力边界
pub trait DomainEvent:
    Clone + PartialEq + fmt::Debug + Serialize + DeserializeOwned + Send + Sync + 'static
{
    /// 事件唯一标识
    fn event_id(&self) -> &str;

    /// 事件类型（形如 `OrderEvent.Created` 或自定义类型名）
    fn event_type(&self) -> &str;

    /// 事件载荷版本（用于版本兼容与上抬）
    fn event_version(&self) -> usize;

    /// 事件对应的聚合版本（用于并发控制）
    fn aggregate_version(&self) -> Version;
}
