//! Value objects.
//!
//! Value objects are domain concepts that have no identity — two value
//! objects are considered equal if and only if their fields are equal.
//! They are used to encapsulate immutable, conceptually meaningful values
//! together with their validation rules.
//!

use std::fmt;

use eventide_macros::value_object;

/// Contract implemented by every value object.
///
/// The associated [`ValueObject::Error`] type carries domain-specific
/// validation failures, while [`ValueObject::validate`] performs the
/// invariant check. Implementors should call `validate` from their
/// constructors so that an instance can never exist in an invalid state.
pub trait ValueObject {
    /// Error type returned when business validation fails.
    type Error;

    /// Run the value object's validation rules and return an error if any
    /// invariant is violated.
    fn validate(&self) -> Result<(), Self::Error>;
}

/// Monotonically increasing version number for optimistic concurrency
/// control on entities and aggregates.
///
/// `Version` is a thin newtype around `usize` with a domain-specific API.
/// Wrapping the raw integer prevents the common bug of mixing version
/// numbers with other counters and gives every comparison and increment a
/// clear name.
///
/// # Examples
///
/// ```
/// use eventide_domain::value_object::Version;
///
/// let v1 = Version::new();
/// assert_eq!(v1.value(), 0);
/// assert!(v1.is_new());
///
/// let v2 = v1.next();
/// assert_eq!(v2.value(), 1);
/// assert!(!v2.is_new());
///
/// assert!(v2 > v1);
/// ```
// `value_object` provides the standard derives shared by every value object
// (`Debug`, `Clone`, `Default`, `Serialize`, `Deserialize`, `PartialEq`, `Eq`).
// `Version` then layers on the extra capabilities it needs as a numeric
// counter (`Copy`, `PartialOrd`, `Ord`, `Hash`).
#[value_object]
#[derive(Copy, PartialOrd, Ord, Hash)]
pub struct Version(usize);

impl Version {
    /// Build the initial version (`0`), representing an aggregate that has
    /// never had any events applied to it yet.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventide_domain::value_object::Version;
    ///
    /// let v = Version::new();
    /// assert_eq!(v.value(), 0);
    /// ```
    pub const fn new() -> Self {
        Self(0)
    }

    /// Build a version from a raw `usize`.
    ///
    /// Most callers should use [`Version::new`] together with
    /// [`Version::next`]; this constructor is intended primarily for
    /// repositories that materialise a version from storage.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventide_domain::value_object::Version;
    ///
    /// let v = Version::from_value(5);
    /// assert_eq!(v.value(), 5);
    /// ```
    pub const fn from_value(value: usize) -> Self {
        Self(value)
    }

    /// Return the next sequential version (`self + 1`).
    ///
    /// # Examples
    ///
    /// ```
    /// use eventide_domain::value_object::Version;
    ///
    /// let v1 = Version::from_value(10);
    /// let v2 = v1.next();
    /// assert_eq!(v2.value(), 11);
    /// ```
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    /// Return the underlying numeric value.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventide_domain::value_object::Version;
    ///
    /// let v = Version::from_value(42);
    /// assert_eq!(v.value(), 42);
    /// ```
    pub const fn value(&self) -> usize {
        self.0
    }

    /// Returns `true` when this is the initial version (`0`).
    ///
    /// # Examples
    ///
    /// ```
    /// use eventide_domain::value_object::Version;
    ///
    /// let v0 = Version::new();
    /// assert!(v0.is_new());
    ///
    /// let v1 = v0.next();
    /// assert!(!v1.is_new());
    /// ```
    pub fn is_new(&self) -> bool {
        self.0 == 0
    }

    /// Returns `true` once at least one event has been applied (i.e. the
    /// version is greater than zero), meaning the aggregate already exists
    /// in storage.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventide_domain::value_object::Version;
    ///
    /// let v0 = Version::new();
    /// assert!(!v0.is_created());
    ///
    /// let v1 = v0.next();
    /// assert!(v1.is_created());
    /// ```
    pub fn is_created(&self) -> bool {
        self.0 > 0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl From<usize> for Version {
    fn from(value: usize) -> Self {
        Self::from_value(value)
    }
}

impl From<Version> for usize {
    fn from(version: Version) -> Self {
        version.value()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The default-constructed `Version` should equal zero and be flagged as
    // both "new" and "not created yet".
    #[test]
    fn test_version_new() {
        let v = Version::new();
        assert_eq!(v.value(), 0);
        assert!(v.is_new());
        assert!(!v.is_created());
    }

    // `from_value` exposes a non-zero version as both not-new and created.
    #[test]
    fn test_version_from_value() {
        let v = Version::from_value(5);
        assert_eq!(v.value(), 5);
        assert!(!v.is_new());
        assert!(v.is_created());
    }

    // `next` returns a fresh value that is exactly one greater than the
    // original and leaves the original untouched.
    #[test]
    fn test_version_next() {
        let v1 = Version::from_value(10);
        let v2 = v1.next();

        assert_eq!(v1.value(), 10);
        assert_eq!(v2.value(), 11);
    }

    // The derived `PartialOrd` / `Ord` implementations should compare
    // versions by their underlying `usize`.
    #[test]
    fn test_version_ordering() {
        let v0 = Version::from_value(0);
        let v1 = Version::from_value(1);
        let v2 = Version::from_value(2);

        assert!(v1 > v0);
        assert!(v2 > v1);
        assert!(v2 >= v1);
        assert!(v0 < v2);
        assert_eq!(v1, Version::from_value(1));
    }

    // Equality is purely structural, as expected for a value object.
    #[test]
    fn test_version_equality() {
        let v1 = Version::from_value(5);
        let v2 = Version::from_value(5);
        let v3 = Version::from_value(6);

        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
    }

    // The `Display` impl renders versions as `v<n>` for human-readable logs.
    #[test]
    fn test_version_display() {
        let v0 = Version::new();
        let v5 = Version::from_value(5);

        assert_eq!(format!("{}", v0), "v0");
        assert_eq!(format!("{}", v5), "v5");
    }

    // `Default::default` should match `Version::new()` so that the type can
    // be embedded in entities with `#[derive(Default)]`.
    #[test]
    fn test_version_default() {
        let v: Version = Default::default();
        assert_eq!(v, Version::new());
        assert_eq!(v.value(), 0);
    }

    // `From<usize>` provides ergonomic construction via `.into()`.
    #[test]
    fn test_version_from_usize() {
        let v: Version = 42.into();
        assert_eq!(v.value(), 42);
    }

    // `From<Version> for usize` allows lossless extraction of the raw value.
    #[test]
    fn test_version_into_usize() {
        let v = Version::from_value(42);
        let num: usize = v.into();
        assert_eq!(num, 42);
    }

    // `is_created` should match the rule "version > 0", regardless of how
    // the version was created.
    #[test]
    fn test_version_is_created() {
        let v0 = Version::new();
        assert!(!v0.is_created());

        let v1 = v0.next();
        assert!(v1.is_created());

        let v5 = Version::from_value(5);
        assert!(v5.is_created());
    }

    // `next` chains cleanly — useful when constructing fixtures in tests.
    #[test]
    fn test_version_chaining() {
        let v = Version::new().next().next().next();
        assert_eq!(v.value(), 3);
    }

    // `Version` should serialize transparently as its inner `usize` and
    // round-trip back to the same value.
    #[test]
    fn test_version_serde() {
        let v = Version::from_value(42);

        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "42");

        let deserialized: Version = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, v);
    }

    // Because `Version` is `Copy`, assignment performs a bitwise copy and
    // the original remains usable.
    #[test]
    fn test_version_clone() {
        let v1 = Version::from_value(10);
        let v2 = v1;

        assert_eq!(v1, v2);
        assert_eq!(v1.value(), v2.value());
    }
}
