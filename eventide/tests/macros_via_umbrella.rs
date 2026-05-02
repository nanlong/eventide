//! Verifies that `#[entity]`, `#[entity_id]`, `#[domain_event]`,
//! `#[value_object]`, `#[async_trait]` and the `tokio` runtime work when
//! the user depends only on the `eventide` umbrella crate — no direct
//! dependency on `eventide-domain`, `serde`, `async-trait` or `tokio`.
//!
//! The macros must route every generated path through `::eventide::domain`
//! (and `::eventide::domain::__serde` for `Serialize` / `Deserialize`)
//! when invoked from this crate. The async-runtime helpers must be reachable
//! through `eventide::async_trait` and `eventide::tokio`. If anything falls
//! back to `::eventide_domain`, `::serde`, `::async_trait` or `::tokio`,
//! this file fails to compile.

use eventide::async_trait;
use eventide::prelude::*;
use eventide::tokio;

#[entity_id]
struct AccountId(u64);

#[entity(id = AccountId)]
#[derive(Clone)]
struct Account {
    balance: i64,
}

#[derive(Debug)]
enum AccountCommand {
    Deposit { amount: i64 },
}

#[domain_event(version = 1)]
enum AccountEvent {
    #[event(event_type = "account.deposited")]
    Deposited { amount: i64 },
}

#[value_object]
struct Money {
    amount: i64,
    currency: String,
}

impl Aggregate for Account {
    const TYPE: &'static str = "account";
    type Command = AccountCommand;
    type Event = AccountEvent;
    type Error = DomainError;

    fn execute(&self, cmd: AccountCommand) -> Result<Vec<AccountEvent>, DomainError> {
        match cmd {
            AccountCommand::Deposit { amount } if amount > 0 => Ok(vec![AccountEvent::Deposited {
                id: "evt-1".to_string(),
                aggregate_version: self.version().next(),
                amount,
            }]),
            _ => Err(DomainError::invalid_command("amount must be > 0")),
        }
    }

    fn apply(&mut self, event: &AccountEvent) {
        match event {
            AccountEvent::Deposited {
                aggregate_version,
                amount,
                ..
            } => {
                self.balance += *amount;
                self.version = *aggregate_version;
            }
        }
    }
}

#[test]
fn entity_id_roundtrips_through_umbrella_path() {
    let id = AccountId::new(42);
    assert_eq!(id.as_ref(), &42u64);
    assert_eq!(format!("{id}"), "42");
}

#[test]
fn aggregate_executes_and_applies_via_umbrella_path() {
    let mut acc = Account::new(AccountId::new(1), Version::default());
    let events = acc
        .execute(AccountCommand::Deposit { amount: 100 })
        .expect("deposit succeeds");
    for e in &events {
        acc.apply(e);
    }
    assert_eq!(acc.balance, 100);
    assert_eq!(acc.version(), Version::from_value(1));
}

#[test]
fn domain_event_metadata_via_umbrella_path() {
    let evt = AccountEvent::Deposited {
        id: "evt-1".to_string(),
        aggregate_version: Version::from_value(1),
        amount: 50,
    };
    assert_eq!(evt.event_id(), "evt-1");
    assert_eq!(evt.event_type(), "account.deposited");
    assert_eq!(evt.event_version(), 1);
    assert_eq!(evt.aggregate_version(), Version::from_value(1));
}

#[test]
fn value_object_derives_apply_via_umbrella_path() {
    let m = Money {
        amount: 10,
        currency: "USD".into(),
    };
    let cloned = m.clone();
    assert_eq!(m, cloned);
}

#[async_trait]
trait BalanceProbe: Send + Sync {
    async fn current(&self) -> i64;
}

struct InMemoryProbe(i64);

#[async_trait]
impl BalanceProbe for InMemoryProbe {
    async fn current(&self) -> i64 {
        self.0
    }
}

#[tokio::test]
async fn async_trait_and_tokio_via_umbrella_path() {
    let probe: Box<dyn BalanceProbe> = Box::new(InMemoryProbe(42));
    assert_eq!(probe.current().await, 42);

    // Exercise `eventide::tokio` directly so that re-export reaching the
    // runtime — not just the macro — is part of the compile-time contract.
    tokio::time::sleep(std::time::Duration::from_millis(0)).await;
}
