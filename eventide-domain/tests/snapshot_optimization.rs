#![cfg(feature = "eventing")]
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use chrono::Utc;
use eventide_domain::aggregate::Aggregate;
use eventide_domain::domain_event::EventContext;
use eventide_domain::entity::Entity;
use eventide_domain::error::{DomainError, DomainResult};
use eventide_domain::persist::{
    AggregateRepository, EventRepository, SerializedEvent, SerializedSnapshot, SnapshotPolicy,
    SnapshotPolicyRepo, SnapshotRepository, SnapshotRepositoryWithPolicy,
};
use eventide_domain::value_object::Version;
use eventide_macros::{domain_event, entity};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[entity]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Counter {
    value: i64,
}

#[domain_event(version = 1)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum CounterEvent {
    Incr { by: i64 },
}

impl Aggregate for Counter {
    const TYPE: &'static str = "counter";
    type Command = ();
    type Event = CounterEvent;
    type Error = DomainError;
    fn execute(&self, _c: Self::Command) -> Result<Vec<Self::Event>, Self::Error> {
        Ok(vec![])
    }
    fn apply(&mut self, e: &Self::Event) {
        match e {
            CounterEvent::Incr {
                aggregate_version,
                by,
                ..
            } => {
                self.value += *by;
                self.version = *aggregate_version;
            }
        }
    }
}

#[derive(Default, Clone)]
struct CountingEventRepo {
    events: Arc<Mutex<HashMap<String, Vec<SerializedEvent>>>>,
    pub get_all_calls: Arc<Mutex<usize>>,
    pub get_last_calls: Arc<Mutex<usize>>,
}

#[async_trait]
impl EventRepository for CountingEventRepo {
    async fn get_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
    ) -> DomainResult<Vec<SerializedEvent>> {
        *self.get_all_calls.lock().unwrap() += 1;
        Ok(self
            .events
            .lock()
            .unwrap()
            .get(&aggregate_id.to_string())
            .cloned()
            .unwrap_or_default())
    }
    async fn get_last_events<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        last_version: usize,
    ) -> DomainResult<Vec<SerializedEvent>> {
        *self.get_last_calls.lock().unwrap() += 1;
        Ok(self
            .events
            .lock()
            .unwrap()
            .get(&aggregate_id.to_string())
            .map(|v| {
                v.iter()
                    .filter(|e| e.aggregate_version() > last_version)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default())
    }
    async fn save(&self, events: Vec<SerializedEvent>) -> DomainResult<()> {
        if events.is_empty() {
            return Ok(());
        }
        let mut g = self.events.lock().unwrap();
        let k = events[0].aggregate_id().to_string();
        g.entry(k).or_default().extend_from_slice(&events);
        Ok(())
    }
}

#[derive(Default, Clone)]
struct InMemorySnapshotPolicyRepo {
    snaps: Arc<Mutex<HashMap<String, SerializedSnapshot>>>,
}

#[async_trait]
impl SnapshotRepository for InMemorySnapshotPolicyRepo {
    async fn get_snapshot<A: Aggregate>(
        &self,
        aggregate_id: &A::Id,
        _version: Option<usize>,
    ) -> DomainResult<Option<SerializedSnapshot>> {
        Ok(self
            .snaps
            .lock()
            .unwrap()
            .get(&aggregate_id.to_string())
            .cloned())
    }
    async fn save<A: Aggregate>(&self, aggregate: &A) -> DomainResult<()> {
        let snap = SerializedSnapshot::from_aggregate(aggregate)?;
        self.snaps
            .lock()
            .unwrap()
            .insert(aggregate.id().to_string(), snap);
        Ok(())
    }
}

fn mk_incr(id: &str, version: usize, by: i64) -> SerializedEvent {
    let eid = ulid::Ulid::new().to_string();
    let payload = serde_json::json!({"Incr": {"id": eid, "aggregate_version": version, "by": by }});
    let event_context = EventContext::builder()
        .maybe_correlation_id(Some(format!("cor-{id}")))
        .maybe_causation_id(Some(format!("cau-{id}")))
        .maybe_actor_type(Some("user".into()))
        .maybe_actor_id(Some("u-1".into()))
        .build();

    SerializedEvent::builder()
        .event_id(eid)
        .event_type("CounterEvent.Incr".into())
        .event_version(1)
        .maybe_sequence_number(None)
        .aggregate_id(id.to_string())
        .aggregate_type("counter".into())
        .aggregate_version(version)
        .correlation_id(format!("cor-{id}"))
        .causation_id(format!("cau-{id}"))
        .actor_type("user".into())
        .actor_id("u-1".into())
        .occurred_at(Utc::now())
        .payload(payload)
        .context(serde_json::to_value(&event_context).expect("serialize EventContext"))
        .build()
}

#[tokio::test]
async fn snapshot_optimization_by_call_count() -> AnyResult<()> {
    let repo = Arc::new(CountingEventRepo::default());
    let snaps = Arc::new(SnapshotRepositoryWithPolicy::new(
        InMemorySnapshotPolicyRepo::default(),
        SnapshotPolicy::Every(1),
    ));
    let chain = Arc::new(eventide_domain::event_upcaster::EventUpcasterChain::default());
    let store = SnapshotPolicyRepo::new(repo.clone(), snaps.clone(), chain);

    let id = "c-1".to_string();

    // arrange: persist a long event history (versions 1..=100)
    let mut all = Vec::new();
    for v in 1..=100 {
        all.push(mk_incr(&id, v, 1));
    }
    repo.save(all).await?;

    // arrange: build the matching aggregate state and store it as a snapshot at version 100
    let mut agg = <Counter as Entity>::new(id.clone(), Version::new());
    for v in 1..=100 {
        agg.apply(&CounterEvent::Incr {
            id: ulid::Ulid::new().to_string(),
            aggregate_version: Version::from_value(v),
            by: 1,
        });
    }
    snaps.save(&agg).await?;

    // arrange: append incremental events after the snapshot (versions 101..=105)
    let mut inc = Vec::new();
    for v in 101..=105 {
        inc.push(mk_incr(&id, v, 1));
    }
    repo.save(inc).await?;

    // act + assert: load() must hit only get_last_events once (post-snapshot tail),
    // never get_events (full history) — confirms the snapshot fast-path is used.
    let loaded: Counter = store.load(&id).await?.unwrap();
    assert_eq!(loaded.version(), Version::from_value(105));
    assert_eq!(loaded.value, 105);
    assert_eq!(*repo.get_all_calls.lock().unwrap(), 0);
    assert_eq!(*repo.get_last_calls.lock().unwrap(), 1);
    Ok(())
}
