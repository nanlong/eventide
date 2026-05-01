//! Entity foundations.
//!
//! Provides the [`Entity`] trait used by aggregates and entities alike to
//! expose their unique identity ([`Entity::Id`]) and their monotonically
//! increasing [`Version`] (used for optimistic concurrency control).
//!
use std::{fmt::Display, str::FromStr};

use crate::value_object::Version;

/// Domain entity contract: something that has a stable identity and a
/// version which advances every time the entity changes.
///
/// The version is used by repositories for optimistic locking: a write that
/// is based on a stale version must be rejected with a conflict so callers
/// can retry. New entities start at [`Version::new`] (zero).
pub trait Entity: Send + Sync {
    /// Identifier type.
    ///
    /// Entity IDs must be parseable from a string (for deserialisation),
    /// displayable (for logging and persistence), cloneable, and safe to
    /// share across threads.
    type Id: FromStr + Clone + Display + Send + Sync;

    /// Construct a new entity with the supplied identity at the supplied
    /// starting version. Implementations typically delegate to the
    /// [`#[entity]`](eventide_macros::entity) macro, which fills in the
    /// `id` and `version` fields automatically.
    fn new(aggregate_id: Self::Id, version: Version) -> Self;

    /// Return a borrow of this entity's identity.
    fn id(&self) -> &Self::Id;

    /// Return the current version, used by repositories for optimistic
    /// concurrency checks.
    fn version(&self) -> Version;

    /// Convenience: returns `true` once at least one event has been applied
    /// (i.e. the version is greater than zero).
    fn is_created(&self) -> bool {
        self.version().is_created()
    }
}
