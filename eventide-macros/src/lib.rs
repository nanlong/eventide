//! Procedural macros for the `eventide` Domain-Driven Design / CQRS toolkit.
//!
//! This crate provides four attribute macros that remove boilerplate when
//! modelling DDD building blocks in Rust:
//!
//! | Macro              | Applies to            | Purpose                                                                |
//! |--------------------|-----------------------|------------------------------------------------------------------------|
//! | [`entity`]         | `struct` (named)      | Mark a type as a DDD entity / aggregate root.                          |
//! | [`entity_id`]      | tuple `struct`        | Newtype wrapper around a primitive ID with a rich set of conversions.  |
//! | [`domain_event`]   | `enum` (named fields) | Tag an enum as the event family for an aggregate.                      |
//! | [`value_object`]   | `struct` or `enum`    | Add the standard derive set expected of immutable value objects.       |
//!
//! Each macro lives in its own module; this crate root only re-exports the
//! procedural-macro entry points and forwards to the per-macro `expand`
//! function. See the documentation on each macro for the supported
//! attribute arguments and concrete examples.
mod domain_event;
mod entity;
mod entity_id;
mod utils;
mod value_object;

use proc_macro::TokenStream;

/// Mark a struct as a DDD entity / aggregate root.
///
/// The macro guarantees that the annotated struct has the two fields every
/// `eventide_domain::entity::Entity` implementation needs:
///
/// * `id: <IdType>` — the entity's identity.
/// * `version: ::eventide_domain::value_object::Version` — the optimistic-
///   concurrency version used during event-sourced rehydration.
///
/// If either field is missing it is inserted; if both are present they are
/// repositioned so they appear at the top of the struct (in that order),
/// keeping the layout consistent across all entities. The macro then
/// generates an `impl Entity for ...` block exposing `new`, `id` and
/// `version`.
///
/// A normalised `derive` attribute is also produced. By default this
/// includes `Debug`, `Default`, `serde::Serialize` and `serde::Deserialize`;
/// any derives the user already wrote are merged in (de-duplicated against
/// the standard set, e.g. `Serialize` and `serde::Serialize` collapse).
///
/// # Attribute arguments
///
/// `#[entity(id = <Type>, debug = <bool>)]`
///
/// * `id` — type used for the `id` field. Defaults to `String`.
/// * `debug` — when `false` the macro will *not* derive `Debug`, so callers
///   can provide a custom `Debug` impl. Defaults to `true`.
///
/// Both keys are optional and may appear in any order.
///
/// # Examples
///
/// Minimal usage with a `String` identifier:
///
/// ```ignore
/// use eventide_macros::entity;
///
/// #[entity]
/// struct Account {
///     name: String,
///     balance: i64,
/// }
/// ```
///
/// With a custom newtype identifier:
///
/// ```ignore
/// use eventide_macros::{entity, entity_id};
/// use uuid::Uuid;
///
/// #[entity_id]
/// struct UserId(Uuid);
///
/// #[entity(id = UserId)]
/// struct User {
///     name: String,
/// }
/// ```
///
/// Suppressing the derived `Debug` to provide a custom impl:
///
/// ```ignore
/// use eventide_macros::entity;
///
/// #[entity(id = String, debug = false)]
/// struct Secret {
///     value: String,
/// }
///
/// impl std::fmt::Debug for Secret {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         f.debug_struct("Secret").field("value", &"<redacted>").finish()
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn entity(attr: TokenStream, item: TokenStream) -> TokenStream {
    entity::expand(attr, item)
}

/// Turn a single-field tuple struct into a fully-featured entity-ID newtype.
///
/// Applied to `struct Foo(Inner);` (exactly one unnamed field), this macro
/// generates the standard derive set plus the conversions and helpers users
/// expect from a strongly-typed identifier:
///
/// * Default derives: `Default`, `Clone`, `Copy`, `serde::Serialize`,
///   `serde::Deserialize`, `PartialEq`, `Eq`, `Hash`, and (by default) `Debug`.
///   User-supplied derives are merged in.
/// * `pub fn new(value: Inner) -> Self`
/// * `impl FromStr` (forwarding to the inner type's `FromStr`).
/// * `impl Display` (forwarding to the inner type's `Display`).
/// * `impl AsRef<Inner>` and `impl AsMut<Inner>`.
/// * `impl From<Inner>`/`impl From<&Inner>` and the symmetric
///   `impl From<Self>`/`impl From<&Self> for Inner` conversions.
///
/// # Attribute arguments
///
/// `#[entity_id(debug = <bool>)]`
///
/// * `debug` — disable the auto-derived `Debug` (defaults to `true`) so
///   callers can hand-write a redacting or otherwise customised impl.
///
/// # Important
///
/// Because the macro derives `Copy`, the inner type **must implement
/// `Copy`** (e.g. `Uuid`, `u64`, `i64`). Wrappers around `String` are not
/// supported through this macro.
///
/// # Examples
///
/// ```ignore
/// use eventide_macros::entity_id;
/// use uuid::Uuid;
///
/// #[entity_id]
/// struct OrderId(Uuid);
///
/// let id = OrderId::new(Uuid::new_v4());
/// let copy = id; // `Copy` works.
/// let _ = format!("{} / {:?}", id, id); // `Display` and `Debug` both work.
/// ```
///
/// Custom `Debug` impl:
///
/// ```ignore
/// use eventide_macros::entity_id;
/// use uuid::Uuid;
///
/// #[entity_id(debug = false)]
/// struct ProfileId(Uuid);
///
/// impl std::fmt::Debug for ProfileId {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         write!(f, "ProfileId(<redacted>)")
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn entity_id(attr: TokenStream, item: TokenStream) -> TokenStream {
    entity_id::expand(attr, item)
}

/// Tag an enum as the domain-event family for an aggregate.
///
/// The macro is applied to an enum where every variant uses **named fields**
/// (`Variant { .. }`). For each variant it ensures the presence of:
///
/// * `id: <IdType>` — the unique event identifier.
/// * `aggregate_version: ::eventide_domain::value_object::Version` — the
///   aggregate version this event belongs to.
///
/// Missing fields are appended (existing variant fields keep their relative
/// order). The macro then generates an `impl
/// ::eventide_domain::domain_event::DomainEvent` whose four methods —
/// `event_id`, `event_type`, `event_version`, `aggregate_version` — are
/// implemented as `match` expressions over the variants.
///
/// A normalised `derive` set (`Debug`, `Clone`, `PartialEq`,
/// `serde::Serialize`, `serde::Deserialize`) is merged with any derives the
/// user already wrote.
///
/// # Enum-level attribute arguments
///
/// `#[domain_event(id = <Type>, version = <int>)]`
///
/// * `id` — the type used for every variant's `id` field. Defaults to
///   `String`.
/// * `version` — the default `event_version` returned for variants that do
///   not override it. Defaults to `1`.
///
/// # Per-variant overrides
///
/// Every variant may carry an optional `#[event(...)]` attribute:
///
/// `#[event(event_type = "...", event_version = N)]`
///
/// * `event_type` — string returned from `event_type()` for that variant.
///   When omitted it falls back to `"<EnumName>.<VariantName>"`.
/// * `event_version` — integer returned from `event_version()` for that
///   variant. When omitted it falls back to the enum-level `version`.
///
/// The legacy syntax `#[event_type = "..."]` / `#[event_version = N]` is
/// **rejected** with a compile error pointing users to the unified
/// `#[event(...)]` form.
///
/// # Examples
///
/// ```ignore
/// use eventide_macros::domain_event;
///
/// #[domain_event(version = 1)]
/// enum BankEvent {
///     #[event(event_type = "bank.opened")]
///     Opened { name: String },
///
///     // Override only the version; the event_type defaults to
///     // "BankEvent.Renamed".
///     #[event(event_version = 2)]
///     Renamed { to: String },
/// }
/// ```
///
/// Using a custom identifier type at the enum level:
///
/// ```ignore
/// use eventide_macros::domain_event;
///
/// #[domain_event(id = String, version = 1)]
/// enum UserEvent {
///     Created { name: String },
/// }
/// ```
#[proc_macro_attribute]
pub fn domain_event(attr: TokenStream, item: TokenStream) -> TokenStream {
    domain_event::expand(attr, item)
}

/// Add the conventional derive set for an immutable value object.
///
/// Applied to either a `struct` (named or tuple) or an `enum`. The macro
/// merges the user's existing derives with: `Default`, `Clone`,
/// `serde::Serialize`, `serde::Deserialize`, `PartialEq`, `Eq`, and (by
/// default) `Debug`. Unlike [`entity`] or [`domain_event`] no fields,
/// methods or trait impls are generated — value objects are intentionally
/// minimal pieces of data.
///
/// # Attribute arguments
///
/// `#[value_object(debug = <bool>)]`
///
/// * `debug` — disable the auto-derived `Debug` (defaults to `true`) when
///   callers want to provide a manual implementation (e.g. to redact
///   sensitive content).
///
/// # Examples
///
/// On a struct:
///
/// ```ignore
/// use eventide_macros::value_object;
///
/// #[value_object]
/// struct Money {
///     amount: i64,
///     currency: String,
/// }
/// ```
///
/// On an enum (note the `#[default]` variant required by the derived
/// `Default`):
///
/// ```ignore
/// use eventide_macros::value_object;
///
/// #[value_object]
/// enum Level {
///     #[default]
///     Low,
///     High,
/// }
/// ```
///
/// Disabling auto-`Debug` to write a redacting impl:
///
/// ```ignore
/// use eventide_macros::value_object;
///
/// #[value_object(debug = false)]
/// struct ApiKey(String);
///
/// impl std::fmt::Debug for ApiKey {
///     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
///         f.write_str("ApiKey(<redacted>)")
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn value_object(attr: TokenStream, item: TokenStream) -> TokenStream {
    value_object::expand(attr, item)
}
