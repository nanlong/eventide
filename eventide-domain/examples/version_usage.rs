//! # Version Newtype Usage Example
//!
//! A focused tour of the [`Version`] value object, showing how a newtype around
//! a `usize` improves type safety and readability when modeling aggregate
//! versions in an event-sourced system.
//!
//! ## What this example demonstrates
//!
//! - Constructing `Version` instances with `Version::new()` and `Version::from_value(n)`
//! - Inspecting state with `is_new()` and `is_created()`
//! - Producing the next version immutably via `next()` (chained calls allowed)
//! - Comparing versions with the standard ordering / equality operators
//! - Converting to/from `usize` via `From` / `Into`
//! - Round-tripping through serde JSON
//! - A small simulation that shows version tracking in an aggregate context,
//!   including an optimistic-concurrency conflict check
//!
//! ## Prerequisites
//!
//! No external services. The example is purely demonstrative.
//!
//! ## Running
//!
//! ```bash
//! cargo run -p eventide-domain --example version_usage
//! ```
//!
//! ## Expected output
//!
//! Eight numbered sections covering each capability, followed by an aggregate
//! simulation that prints the version transitions for a sequence of events
//! and a final consistency check between expected and actual version.

use eventide_domain::value_object::Version;

fn main() {
    println!("=== Version newtype usage example ===\n");

    // 1. Construct an initial Version.
    println!("1. Create the initial version");
    let v0 = Version::new();
    println!("   initial version: {} (value = {})", v0, v0.value());
    println!("   is_new (no events applied yet): {}", v0.is_new());
    println!("   is_created: {}\n", v0.is_created());

    // 2. Construct a Version directly from a numeric value.
    println!("2. Create a version from a value");
    let v5 = Version::from_value(5);
    println!("   version: {} (value = {})", v5, v5.value());
    println!("   is_new: {}", v5.is_new());
    println!("   is_created: {}\n", v5.is_created());

    // 3. Produce successive versions by calling next().
    println!("3. Increment the version");
    let v1 = v0.next();
    let v2 = v1.next();
    println!("   v0.next() = {}", v1);
    println!("   v1.next() = {}", v2);
    println!(
        "   chained: v0.next().next().next() = {}\n",
        v0.next().next().next()
    );

    // 4. Each next() returns a new value; the previous value is untouched (value-object semantics).
    println!("4. Immutable increment (value-object semantics)");
    let version = Version::new();
    println!("   initial: {}", version);
    let version = version.next();
    println!("   after first next(): {}", version);
    let version = version.next();
    println!("   after second next(): {}\n", version);

    // 5. Versions compare numerically and support standard equality.
    println!("5. Version comparison");
    let v10 = Version::from_value(10);
    let v20 = Version::from_value(20);
    println!("   v10 = {}, v20 = {}", v10, v20);
    println!("   v10 < v20: {}", v10 < v20);
    println!("   v20 > v10: {}", v20 > v10);
    println!(
        "   v10 == Version::from_value(10): {}\n",
        v10 == Version::from_value(10)
    );

    // 6. Conversions to and from usize via From / Into.
    println!("6. Type conversions");
    let version_from_usize: Version = 42.into();
    println!("   from usize: 42 -> {}", version_from_usize);

    let usize_from_version: usize = version_from_usize.into();
    println!(
        "   back to usize: {} -> {}\n",
        version_from_usize, usize_from_version
    );

    // 7. Round-trip through serde JSON.
    println!("7. Serialization and deserialization");
    let v100 = Version::from_value(100);
    let json = serde_json::to_string(&v100).unwrap();
    println!("   serialized: {} -> {}", v100, json);

    let deserialized: Version = serde_json::from_str(&json).unwrap();
    println!("   deserialized: {} -> {}\n", json, deserialized);

    // 8. Realistic scenario: tracking aggregate versions over a sequence of events.
    println!("8. Real-world scenario: aggregate version management");
    simulate_aggregate_version_management();

    println!("\n=== Example complete ===");
}

/// Simulate a small aggregate version-management scenario, including a final conflict check.
fn simulate_aggregate_version_management() {
    #[derive(Debug)]
    struct Aggregate {
        id: String,
        version: Version,
        data: String,
    }

    impl Aggregate {
        fn new(id: String) -> Self {
            Self {
                id,
                version: Version::new(),
                data: String::new(),
            }
        }

        fn apply_event(&mut self, event: &str) {
            self.data.push_str(event);
            let old_version = self.version;
            self.version = self.version.next();
            println!(
                "   [{}] applied event: '{}', version: {} -> {}",
                self.id, event, old_version, self.version
            );
        }

        fn is_new(&self) -> bool {
            self.version.is_new()
        }

        fn current_version(&self) -> Version {
            self.version
        }
    }

    // Construct a brand-new aggregate.
    let mut aggregate = Aggregate::new("account-001".to_string());
    println!("   created aggregate: {:?}", aggregate.id);
    println!("   is_new: {}", aggregate.is_new());
    println!("   current version: {}", aggregate.current_version());

    // Apply a sequence of domain events.
    println!("\n   applying event sequence:");
    aggregate.apply_event("AccountOpened");
    aggregate.apply_event("Deposited 1000");
    aggregate.apply_event("Withdrew 500");

    println!("\n   final state:");
    println!("   aggregate id: {}", aggregate.id);
    println!("   current version: {}", aggregate.current_version());
    println!("   is_new: {}", aggregate.is_new());
    println!("   event count: {}", aggregate.version.value());

    // Demonstrate optimistic-concurrency conflict detection.
    println!("\n   version conflict check:");
    let expected_version = Version::from_value(2);
    let actual_version = aggregate.current_version();

    if actual_version != expected_version {
        println!(
            "   [conflict] expected {}, got {}",
            expected_version, actual_version
        );
    } else {
        println!("   [match] versions are consistent");
    }
}
