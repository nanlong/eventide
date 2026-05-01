//! Specification pattern.
//!
//! Encapsulates business rules as small composable predicates that can be
//! combined via boolean AND / OR / NOT. The pattern is useful when an
//! aggregate's `execute` method would otherwise grow into a tangle of
//! `if` / `else` branches: each rule becomes its own [`Specification`]
//! implementation that can be reused, unit-tested, and combined freely.
//!
/// Core trait of the specification pattern.
///
/// Implementors encapsulate one self-contained business rule that can be
/// evaluated against a candidate `T`. The trait also provides default
/// implementations for the boolean combinators [`Specification::and`],
/// [`Specification::or`] and [`Specification::not`], so combining rules
/// reads close to how the business expresses them.
pub trait Specification<T> {
    /// Returns `true` when `candidate` satisfies this specification.
    fn is_satisfied_by(&self, candidate: &T) -> bool;

    /// Combine this specification with another using a logical AND.
    /// The combined specification is satisfied only when both inputs are.
    fn and<S>(self, other: S) -> AndSpecification<T>
    where
        Self: Sized + 'static,
        S: Specification<T> + 'static,
    {
        AndSpecification::new(Box::new(self), Box::new(other))
    }

    /// Combine this specification with another using a logical OR.
    /// The combined specification is satisfied when either input is.
    fn or<S>(self, other: S) -> OrSpecification<T>
    where
        Self: Sized + 'static,
        S: Specification<T> + 'static,
    {
        OrSpecification::new(Box::new(self), Box::new(other))
    }

    /// Negate this specification with a logical NOT.
    fn not(self) -> NotSpecification<T>
    where
        Self: Sized + 'static,
    {
        NotSpecification::new(Box::new(self))
    }
}

/// Implement [`Specification`] for `Box<dyn Specification<T>>` so that
/// trait objects can be passed wherever a concrete specification would be
/// accepted (for example, as the operands of [`AndSpecification`] /
/// [`OrSpecification`] / [`NotSpecification`]).
impl<T> Specification<T> for Box<dyn Specification<T>> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        self.as_ref().is_satisfied_by(candidate)
    }
}

/// AND combinator.
///
/// Satisfied only when both inner specifications are satisfied.
pub struct AndSpecification<T> {
    left: Box<dyn Specification<T>>,
    right: Box<dyn Specification<T>>,
}

impl<T> AndSpecification<T> {
    pub fn new(left: Box<dyn Specification<T>>, right: Box<dyn Specification<T>>) -> Self {
        Self { left, right }
    }
}

impl<T> Specification<T> for AndSpecification<T> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        self.left.is_satisfied_by(candidate) && self.right.is_satisfied_by(candidate)
    }
}

/// OR combinator.
///
/// Satisfied when either of the inner specifications is satisfied.
pub struct OrSpecification<T> {
    left: Box<dyn Specification<T>>,
    right: Box<dyn Specification<T>>,
}

impl<T> OrSpecification<T> {
    pub fn new(left: Box<dyn Specification<T>>, right: Box<dyn Specification<T>>) -> Self {
        Self { left, right }
    }
}

impl<T> Specification<T> for OrSpecification<T> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        self.left.is_satisfied_by(candidate) || self.right.is_satisfied_by(candidate)
    }
}

/// NOT combinator.
///
/// Satisfied exactly when the inner specification is *not* satisfied.
pub struct NotSpecification<T> {
    inner: Box<dyn Specification<T>>,
}

impl<T> NotSpecification<T> {
    pub fn new(inner: Box<dyn Specification<T>>) -> Self {
        Self { inner }
    }
}

impl<T> Specification<T> for NotSpecification<T> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        !self.inner.is_satisfied_by(candidate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysTrueSpec;
    impl Specification<i32> for AlwaysTrueSpec {
        fn is_satisfied_by(&self, _: &i32) -> bool {
            true
        }
    }

    struct AlwaysFalseSpec;
    impl Specification<i32> for AlwaysFalseSpec {
        fn is_satisfied_by(&self, _: &i32) -> bool {
            false
        }
    }

    #[test]
    fn test_and_specification() {
        let spec = AlwaysTrueSpec.and(AlwaysTrueSpec);
        assert!(spec.is_satisfied_by(&42));

        let spec = AlwaysTrueSpec.and(AlwaysFalseSpec);
        assert!(!spec.is_satisfied_by(&42));

        let spec = AlwaysFalseSpec.and(AlwaysFalseSpec);
        assert!(!spec.is_satisfied_by(&42));
    }

    #[test]
    fn test_or_specification() {
        let spec = AlwaysTrueSpec.or(AlwaysTrueSpec);
        assert!(spec.is_satisfied_by(&42));

        let spec = AlwaysTrueSpec.or(AlwaysFalseSpec);
        assert!(spec.is_satisfied_by(&42));

        let spec = AlwaysFalseSpec.or(AlwaysFalseSpec);
        assert!(!spec.is_satisfied_by(&42));
    }

    #[test]
    fn test_not_specification() {
        let spec = AlwaysTrueSpec.not();
        assert!(!spec.is_satisfied_by(&42));

        let spec = AlwaysFalseSpec.not();
        assert!(spec.is_satisfied_by(&42));
    }

    #[test]
    fn test_complex_combination() {
        // (TRUE AND FALSE) OR (NOT FALSE) = FALSE OR TRUE = TRUE
        let spec = AlwaysTrueSpec
            .and(AlwaysFalseSpec)
            .or(AlwaysFalseSpec.not());
        assert!(spec.is_satisfied_by(&42));
    }
}
