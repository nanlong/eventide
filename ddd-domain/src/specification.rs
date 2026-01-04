//! 规约（Specification）模式
//!
//! 用于封装业务规则并支持 AND/OR/NOT 组合，便于复用与测试。
/// 规约模式的核心 trait
///
/// 用于封装业务规则，使其可复用、可组合和可测试
pub trait Specification<T> {
    /// 检查候选对象是否满足规约
    fn is_satisfied_by(&self, candidate: &T) -> bool;

    /// 与另一个规约进行 AND 组合
    fn and<S>(self, other: S) -> AndSpecification<T>
    where
        Self: Sized + 'static,
        S: Specification<T> + 'static,
    {
        AndSpecification::new(Box::new(self), Box::new(other))
    }

    /// 与另一个规约进行 OR 组合
    fn or<S>(self, other: S) -> OrSpecification<T>
    where
        Self: Sized + 'static,
        S: Specification<T> + 'static,
    {
        OrSpecification::new(Box::new(self), Box::new(other))
    }

    /// 对规约进行 NOT 操作
    fn not(self) -> NotSpecification<T>
    where
        Self: Sized + 'static,
    {
        NotSpecification::new(Box::new(self))
    }
}

/// 为 `Box<dyn Specification<T>>` 实现 Specification trait
/// 使得可以直接使用 Box 类型的规约
impl<T> Specification<T> for Box<dyn Specification<T>> {
    fn is_satisfied_by(&self, candidate: &T) -> bool {
        self.as_ref().is_satisfied_by(candidate)
    }
}

/// AND 组合规约
///
/// 当两个规约都满足时，组合规约才满足
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

/// OR 组合规约
///
/// 当任意一个规约满足时，组合规约就满足
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

/// NOT 规约
///
/// 当内部规约不满足时，NOT 规约才满足
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
