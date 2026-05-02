# eventide-macros

[![Crates.io](https://img.shields.io/crates/v/eventide-macros.svg)](https://crates.io/crates/eventide-macros)
[![Documentation](https://docs.rs/eventide-macros/badge.svg)](https://docs.rs/eventide-macros)

> 中文版本: [README.zh.md](README.zh.md)

Procedural macros that generate the boilerplate around the
[`eventide`](https://crates.io/crates/eventide) DDD/CQRS toolkit:
`#[entity]`, `#[entity_id]`, `#[domain_event]`, `#[value_object]`.

> Generated code uses the absolute path `::eventide_domain::...`.
> The companion crate [`eventide-domain`](https://crates.io/crates/eventide-domain)
> declares `extern crate self as eventide_domain;` so the macros also
> resolve inside its own tests and examples.

## Default derives

The macros merge user-supplied derives with sensible defaults so you
rarely have to repeat the same list:

| Macro            | Auto-applied derives                                                                                       |
|------------------|------------------------------------------------------------------------------------------------------------|
| `#[entity]`      | `Debug`*, `Default`, `serde::Serialize`, `serde::Deserialize`                                              |
| `#[entity_id]`   | `Default`, `Clone`, `Debug`*, `serde::Serialize`, `serde::Deserialize`, `PartialEq`, `Eq`, `Hash`          |
| `#[domain_event]`| `Debug`, `Clone`, `PartialEq`, `serde::Serialize`, `serde::Deserialize`                                    |
| `#[value_object]`| `Default`, `Clone`, `Debug`*, `serde::Serialize`, `serde::Deserialize`, `PartialEq`, `Eq`                  |

`*` = can be turned off with `debug = false`.

The macro-generated `Serialize` / `Deserialize` derives resolve through
`eventide_domain`'s internal `serde` re-export, so you do **not** need a
direct `serde` dependency for the macros to compile — depending on
`eventide` (or `eventide-domain`) is enough.

Add `serde` to your own `Cargo.toml` only when you reach for it outside
the macro-generated derives — e.g. extra `#[derive(Serialize)]` on
hand-written types or custom `#[serde(...)]` attributes:

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
```

## `#[entity]`

Apply to a named-field struct. The macro:

- Adds `id: IdType` and `version: usize` if missing, ordered first.
- Implements [`Entity`](https://docs.rs/eventide-domain/latest/eventide_domain/entity/trait.Entity.html)
  with `new` / `id` / `version`.
- Merges in the default derives (deduplicated).

```rust
#[entity(id = AccountId)]              // id type defaults to String when omitted
#[entity(debug = false)]                // opt out of automatic Debug derive
struct BankAccount {
    balance: i64,
}
```

Restrictions: named-field struct only.

## `#[entity_id]`

Apply to a single-field tuple struct (e.g. `struct AccountId(String);`). The
macro:

- Implements [`FromStr`](https://doc.rust-lang.org/std/str/trait.FromStr.html)
  (delegated to the inner type) and [`Display`](https://doc.rust-lang.org/std/fmt/trait.Display.html).
- Provides `pub fn new(value: Inner) -> Self`.
- Adds bidirectional conversions (`AsRef`, `AsMut`, `From<&Self> for Inner`, etc.).
- Merges in the default derives.

```rust
#[entity_id]
struct AccountId(String);
```

Restrictions: tuple struct with exactly one field.

## `#[domain_event]`

Apply to an enum whose variants use named fields. The macro:

- Adds `id: IdType` and `aggregate_version: usize` to each variant if
  missing.
- Implements [`DomainEvent`](https://docs.rs/eventide-domain/latest/eventide_domain/domain_event/trait.DomainEvent.html).
- Sets the event type to `EnumName.Variant` by default; override per
  variant.
- Sets the event version from the enum-level `version` argument; override
  per variant.

```rust
#[domain_event(id = String, version = 1)]
enum BankAccountEvent {
    #[event(event_type = "bank_account.deposited")]
    Deposited { amount: i64 },

    #[event(event_type = "bank_account.withdrawn", event_version = 2)]
    Withdrawn { amount: i64 },
}
```

Per-variant overrides are written as
`#[event(event_type = "...", event_version = N)]`.

## `#[value_object]`

Apply to a struct (named-field or tuple) or an enum. The macro merges in
the default derives without changing the existing fields/variants.

```rust
#[value_object]
struct Money {
    amount: i64,
    currency: String,
}

#[value_object(debug = false)]
struct Amount(i64);

#[value_object]
enum Level {
    #[default]
    Low,
    High,
}
```

If the target is an enum and `Default` is enabled (the default), one
variant must be marked with `#[default]`.

## UI tests

```bash
cargo test -p eventide-macros
```

The crate uses [`trybuild`](https://crates.io/crates/trybuild) to verify
that the generated code compiles for representative inputs. Test cases
live under `tests/ui/`.

## Layered architecture

```text
eventide-application  →  eventide-domain  ←  eventide-macros
```

`eventide-macros` only emits domain-layer items, keeping infrastructure
concerns out of generated code.

## License

Licensed under either of [Apache-2.0](../LICENSE-APACHE) or
[MIT](../LICENSE-MIT) at your option.
