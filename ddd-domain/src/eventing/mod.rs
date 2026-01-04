//! 事件子系统（eventing）
//!
//! 提供事件发布/订阅与处理的基础抽象与运行时：
//! - `EventBus`：统一发布/订阅接口；
//! - `EventDeliverer`：从本地存储（如 Outbox）批量取出待投递事件；
//! - `EventReclaimer`：对失败/超时/漏投递事件进行补偿；
//! - `EventHandler`：对外部事件进行消费处理；
//! - `EventEngine`：编排投递、订阅与调度处理，并发执行、失败标记与补偿。
//!
//! 该模块仅定义协议与引擎，不绑定具体传输实现，可对接任意消息系统或内存实现。
pub mod bus;
pub mod bus_inmemory;
pub mod deliverer;
pub mod engine;
pub mod handler;
pub mod reclaimer;

pub use bus::EventBus;
pub use bus_inmemory::InMemoryEventBus;
pub use deliverer::EventDeliverer;
pub use engine::{EngineHandle, EventEngine, EventEngineConfig};
pub use handler::{EventHandler, HandledEventType};
pub use reclaimer::EventReclaimer;
