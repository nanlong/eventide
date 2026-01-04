//! 值对象（Value Object）
//!
//! 无标识、以值相等为准的对象，用于封装不可变的概念性值与校验逻辑。

use std::fmt;

use ddd_macros::value_object;

/// 值对象抽象
pub trait ValueObject {
    /// 业务校验失败时的错误类型
    type Error;

    /// 创建值对象时进行验证
    fn validate(&self) -> Result<(), Self::Error>;
}

/// 版本号（用于乐观锁和并发控制）
///
/// 提供类型安全的版本号操作，避免直接使用 usize 导致的语义不明确问题。
///
/// # 示例
///
/// ```
/// use ddd_domain::value_object::Version;
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
// 使用 value_object 宏提供基础的派生（Debug, Clone, Default, Serialize, Deserialize, PartialEq,
// Eq） 手动添加 Version 特有的派生（Copy, PartialOrd, Ord, Hash）
#[value_object]
#[derive(Copy, PartialOrd, Ord, Hash)]
pub struct Version(usize);

impl Version {
    /// 创建初始版本（版本号为 0）
    ///
    /// # 示例
    ///
    /// ```
    /// use ddd_domain::value_object::Version;
    ///
    /// let v = Version::new();
    /// assert_eq!(v.value(), 0);
    /// ```
    pub const fn new() -> Self {
        Self(0)
    }

    /// 从值创建版本号
    ///
    /// # 示例
    ///
    /// ```
    /// use ddd_domain::value_object::Version;
    ///
    /// let v = Version::from_value(5);
    /// assert_eq!(v.value(), 5);
    /// ```
    pub const fn from_value(value: usize) -> Self {
        Self(value)
    }

    /// 获取下一个版本号
    ///
    /// # 示例
    ///
    /// ```
    /// use ddd_domain::value_object::Version;
    ///
    /// let v1 = Version::from_value(10);
    /// let v2 = v1.next();
    /// assert_eq!(v2.value(), 11);
    /// ```
    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    /// 获取版本号的值
    ///
    /// # 示例
    ///
    /// ```
    /// use ddd_domain::value_object::Version;
    ///
    /// let v = Version::from_value(42);
    /// assert_eq!(v.value(), 42);
    /// ```
    pub const fn value(&self) -> usize {
        self.0
    }

    /// 检查是否为初始版本
    ///
    /// # 示例
    ///
    /// ```
    /// use ddd_domain::value_object::Version;
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

    /// 检查聚合是否已创建（版本大于零）
    ///
    /// # 示例
    ///
    /// ```
    /// use ddd_domain::value_object::Version;
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

    // 测试初始版本创建
    #[test]
    fn test_version_new() {
        let v = Version::new();
        assert_eq!(v.value(), 0);
        assert!(v.is_new());
        assert!(!v.is_created());
    }

    // 测试从值创建版本
    #[test]
    fn test_version_from_value() {
        let v = Version::from_value(5);
        assert_eq!(v.value(), 5);
        assert!(!v.is_new());
        assert!(v.is_created());
    }

    // 测试获取下一个版本
    #[test]
    fn test_version_next() {
        let v1 = Version::from_value(10);
        let v2 = v1.next();

        assert_eq!(v1.value(), 10);
        assert_eq!(v2.value(), 11);
    }

    // 测试版本比较
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

    // 测试版本相等性
    #[test]
    fn test_version_equality() {
        let v1 = Version::from_value(5);
        let v2 = Version::from_value(5);
        let v3 = Version::from_value(6);

        assert_eq!(v1, v2);
        assert_ne!(v1, v3);
    }

    // 测试 Display 实现
    #[test]
    fn test_version_display() {
        let v0 = Version::new();
        let v5 = Version::from_value(5);

        assert_eq!(format!("{}", v0), "v0");
        assert_eq!(format!("{}", v5), "v5");
    }

    // 测试 Default 实现
    #[test]
    fn test_version_default() {
        let v: Version = Default::default();
        assert_eq!(v, Version::new());
        assert_eq!(v.value(), 0);
    }

    // 测试 From<usize> 实现
    #[test]
    fn test_version_from_usize() {
        let v: Version = 42.into();
        assert_eq!(v.value(), 42);
    }

    // 测试 Into<usize> 实现
    #[test]
    fn test_version_into_usize() {
        let v = Version::from_value(42);
        let num: usize = v.into();
        assert_eq!(num, 42);
    }

    // 测试 is_created 方法
    #[test]
    fn test_version_is_created() {
        let v0 = Version::new();
        assert!(!v0.is_created());

        let v1 = v0.next();
        assert!(v1.is_created());

        let v5 = Version::from_value(5);
        assert!(v5.is_created());
    }

    // 测试版本号链式操作
    #[test]
    fn test_version_chaining() {
        let v = Version::new().next().next().next();
        assert_eq!(v.value(), 3);
    }

    // 测试序列化和反序列化
    #[test]
    fn test_version_serde() {
        let v = Version::from_value(42);

        let json = serde_json::to_string(&v).unwrap();
        assert_eq!(json, "42");

        let deserialized: Version = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, v);
    }

    // 测试版本号克隆
    #[test]
    fn test_version_clone() {
        let v1 = Version::from_value(10);
        let v2 = v1;

        assert_eq!(v1, v2);
        assert_eq!(v1.value(), v2.value());
    }
}
