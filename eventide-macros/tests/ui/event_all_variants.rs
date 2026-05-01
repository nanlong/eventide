use eventide_domain::domain_event::DomainEvent;
use eventide_domain::value_object::Version;
use eventide_macros::domain_event;

// 测试单元变体
#[domain_event(id = String)]
enum UnitVariantEvent {
    Activated,
    Deactivated,
}

// 测试单字段元组变体
#[domain_event(id = String)]
enum SingleTupleEvent {
    Updated(String),
}

// 测试多字段元组变体
#[domain_event(id = String)]
enum MultiTupleEvent {
    Changed(i32, String, bool),
}

// 测试混合变体类型
#[domain_event(id = String)]
enum MixedEvent {
    Started,
    Updated(String),
    Completed { result: i32 },
}

// 测试单元变体带属性覆盖
#[domain_event(id = String)]
enum UnitWithAttrEvent {
    #[event(event_type = "custom.started", event_version = 2)]
    Started,
}

fn main() {
    // 测试单元变体
    let event = UnitVariantEvent::Activated {
        id: "e1".to_string(),
        aggregate_version: Version::from_value(1),
    };
    assert_eq!(event.event_id(), "e1");
    assert_eq!(event.event_type(), "UnitVariantEvent.Activated");
    assert_eq!(event.event_version(), 1);
    assert_eq!(event.aggregate_version(), Version::from_value(1));

    // 测试单字段元组变体
    let event = SingleTupleEvent::Updated {
        value: "new_value".to_string(),
        id: "e2".to_string(),
        aggregate_version: Version::from_value(2),
    };
    assert_eq!(event.event_id(), "e2");
    assert_eq!(event.event_type(), "SingleTupleEvent.Updated");

    // 测试多字段元组变体
    let event = MultiTupleEvent::Changed {
        value_0: 42,
        value_1: "changed".to_string(),
        value_2: true,
        id: "e3".to_string(),
        aggregate_version: Version::from_value(3),
    };
    assert_eq!(event.event_id(), "e3");
    assert_eq!(event.event_type(), "MultiTupleEvent.Changed");

    // 测试混合变体类型
    let event1 = MixedEvent::Started {
        id: "e4".to_string(),
        aggregate_version: Version::from_value(1),
    };
    let event2 = MixedEvent::Updated {
        value: "updated".to_string(),
        id: "e5".to_string(),
        aggregate_version: Version::from_value(2),
    };
    let event3 = MixedEvent::Completed {
        result: 100,
        id: "e6".to_string(),
        aggregate_version: Version::from_value(3),
    };
    assert_eq!(event1.event_type(), "MixedEvent.Started");
    assert_eq!(event2.event_type(), "MixedEvent.Updated");
    assert_eq!(event3.event_type(), "MixedEvent.Completed");

    // 测试单元变体带属性覆盖
    let event = UnitWithAttrEvent::Started {
        id: "e7".to_string(),
        aggregate_version: Version::from_value(1),
    };
    assert_eq!(event.event_type(), "custom.started");
    assert_eq!(event.event_version(), 2);

    println!("All tests passed!");
}
