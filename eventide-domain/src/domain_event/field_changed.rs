use serde::{Deserialize, Serialize};

/// Captures a "before" and "after" pair for a single field of a domain event.
///
/// `FieldChanged<T>` is the canonical way to model field-level updates in an
/// event payload. Storing both the previous and new value alongside each
/// other has two benefits:
///
/// - it keeps events self-describing — projections and read-side handlers do
///   not have to query historical state to know what changed;
/// - it makes "no-op" updates trivial to detect via [`is_changed`](Self::is_changed)
///   when `T: PartialEq`.
///
/// # Examples
///
/// ```ignore
/// // Inside a domain event variant
/// pub struct ShippingAddressUpdated {
///     pub address: FieldChanged<Address>,
/// }
///
/// // Constructing one from old and new values
/// let change = FieldChanged::new(old_address, new_address);
/// if change.is_changed() {
///     // emit the event ...
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldChanged<T> {
    pub old: T,
    pub new: T,
}

impl<T> FieldChanged<T> {
    pub fn new(old: T, new: T) -> Self {
        Self { old, new }
    }

    pub fn new_value(&self) -> &T {
        &self.new
    }

    pub fn old_value(&self) -> &T {
        &self.old
    }
}

impl<T> FieldChanged<T>
where
    T: PartialEq,
{
    pub fn is_changed(&self) -> bool {
        self.old != self.new
    }
}
