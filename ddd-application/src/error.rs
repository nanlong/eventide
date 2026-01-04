//! # 应用层统一错误定义
//!
//! 本模块提供应用层的错误处理机制，与 `ddd-domain` 的 [`ErrorCode`] trait 无缝集成。
//!
//! ## 架构设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  ddd-domain                                                     │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
//! │  │ ErrorKind   │  │ ErrorCode   │  │ DomainError             │ │
//! │  │ (分类枚举)  │  │ (trait)     │  │ impl ErrorCode ✓        │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  ddd-application (本模块)                                       │
//! │  ┌───────────────────────────────────────────────────────────┐ │
//! │  │ AppError                                                  │ │
//! │  │ impl ErrorCode ✓                                          │ │
//! │  │ impl From<DomainError> ✓                                  │ │
//! │  └───────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 快速开始
//!
//! ### 使用 AppError
//!
//! ```rust
//! use ddd_application::error::{AppError, AppResult};
//! use ddd_domain::error::DomainError;
//!
//! fn process_command() -> AppResult<String> {
//!     // 领域错误会自动转换为 AppError
//!     let domain_result: Result<String, DomainError> = Err(DomainError::not_found("user 123"));
//!     domain_result?;
//!
//!     Ok("success".to_string())
//! }
//!
//! fn validate_input(input: &str) -> AppResult<()> {
//!     if input.is_empty() {
//!         return Err(AppError::validation("input cannot be empty"));
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ### API 层转换
//!
//! ```rust,ignore
//! use axum::response::{IntoResponse, Response};
//! use axum::http::StatusCode;
//! use axum::Json;
//! use ddd_domain::error::ErrorCode;
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! pub struct ApiErrorResponse {
//!     pub code: String,
//!     pub message: String,
//! }
//!
//! pub struct ApiError<E>(pub E);
//!
//! impl<E: ErrorCode> IntoResponse for ApiError<E> {
//!     fn into_response(self) -> Response {
//!         let status = StatusCode::from_u16(self.0.http_status())
//!             .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
//!
//!         let body = ApiErrorResponse {
//!             code: self.0.code().to_string(),
//!             message: self.0.to_string(),
//!         };
//!
//!         (status, Json(body)).into_response()
//!     }
//! }
//! ```

use std::{error::Error as StdError, fmt};

use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};

/// 应用层统一错误类型
///
/// 提供应用层特有的错误类型，同时能够包装领域层错误。
///
/// # 特点
///
/// - 实现 [`ErrorCode`] trait，可直接转换为 API 响应
/// - 通过 `From<DomainError>` 自动转换领域错误
/// - 提供应用层特有的错误类型（验证、授权、Handler 等）
///
/// # 示例
///
/// ```rust
/// use ddd_application::error::AppError;
/// use ddd_domain::error::{ErrorCode, ErrorKind};
///
/// let err = AppError::validation("email format invalid");
/// assert_eq!(err.kind(), ErrorKind::InvalidValue);
/// assert_eq!(err.code(), "VALIDATION_ERROR");
///
/// let err = AppError::handler_not_found("CreateUserHandler");
/// assert_eq!(err.kind(), ErrorKind::Internal);
/// assert_eq!(err.code(), "HANDLER_NOT_FOUND");
/// ```
pub struct AppError {
    kind: ErrorKind,
    code: &'static str,
    message: Box<str>,
    source: Option<Source>,
}

enum Source {
    Domain(DomainError),
    Other(Box<dyn StdError + Send + Sync>),
}

impl AppError {
    /// 创建新的应用错误
    fn new(kind: ErrorKind, code: &'static str, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            source: None,
        }
    }

    // ==================== 便捷构造 ====================

    /// 创建「验证错误」
    ///
    /// 用于应用层输入验证失败的场景。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorCode;
    ///
    /// let err = AppError::validation("email format invalid");
    /// assert_eq!(err.code(), "VALIDATION_ERROR");
    /// assert_eq!(err.http_status(), 400);
    /// ```
    #[must_use]
    pub fn validation(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidValue, "VALIDATION_ERROR", msg)
    }

    /// 创建「未授权错误」
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorCode;
    ///
    /// let err = AppError::unauthorized("invalid token");
    /// assert_eq!(err.code(), "UNAUTHORIZED");
    /// assert_eq!(err.http_status(), 401);
    /// ```
    #[must_use]
    pub fn unauthorized(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Unauthorized, "UNAUTHORIZED", msg)
    }

    /// 创建「Handler 未找到」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorCode;
    ///
    /// let err = AppError::handler_not_found("CreateUserHandler");
    /// assert_eq!(err.code(), "HANDLER_NOT_FOUND");
    /// ```
    #[must_use]
    pub fn handler_not_found(handler_name: &str) -> Self {
        Self::new(
            ErrorKind::Internal,
            "HANDLER_NOT_FOUND",
            format!("handler not found: {handler_name}"),
        )
    }

    /// 创建「聚合不存在」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorCode;
    ///
    /// let err = AppError::aggregate_not_found("User", "user-123");
    /// assert_eq!(err.code(), "AGGREGATE_NOT_FOUND");
    /// assert_eq!(err.http_status(), 404);
    /// ```
    #[must_use]
    pub fn aggregate_not_found(aggregate_type: &str, aggregate_id: &str) -> Self {
        Self::new(
            ErrorKind::NotFound,
            "AGGREGATE_NOT_FOUND",
            format!("{aggregate_type} not found: {aggregate_id}"),
        )
    }

    /// 创建「Handler 已注册」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorCode;
    ///
    /// let err = AppError::handler_already_registered("CreateUserHandler");
    /// assert_eq!(err.code(), "HANDLER_ALREADY_REGISTERED");
    /// ```
    #[must_use]
    pub fn handler_already_registered(handler_name: &str) -> Self {
        Self::new(
            ErrorKind::Internal,
            "HANDLER_ALREADY_REGISTERED",
            format!("handler already registered: {handler_name}"),
        )
    }

    /// 创建「类型不匹配」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorCode;
    ///
    /// let err = AppError::type_mismatch("String", "i32");
    /// assert_eq!(err.code(), "TYPE_MISMATCH");
    /// ```
    #[must_use]
    pub fn type_mismatch(expected: &str, found: &str) -> Self {
        Self::new(
            ErrorKind::Internal,
            "TYPE_MISMATCH",
            format!("type mismatch: expected={expected}, found={found}"),
        )
    }

    /// 创建「内部错误」
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorCode;
    ///
    /// let err = AppError::internal("unexpected state");
    /// assert_eq!(err.code(), "INTERNAL_ERROR");
    /// assert_eq!(err.http_status(), 500);
    /// ```
    #[must_use]
    pub fn internal(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Internal, "INTERNAL_ERROR", msg)
    }

    // ==================== 查询方法 ====================

    /// 获取错误分类
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// 获取领域错误引用（如果是从 DomainError 转换而来）
    #[must_use]
    pub fn domain_error(&self) -> Option<&DomainError> {
        match &self.source {
            Some(Source::Domain(e)) => Some(e),
            _ => None,
        }
    }

    /// 获取内部错误引用
    #[must_use]
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match &self.source {
            Some(Source::Domain(e)) => Some(e),
            Some(Source::Other(e)) => Some(e.as_ref()),
            None => None,
        }
    }

    /// 尝试向下转型为具体错误类型
    ///
    /// 支持从 `DomainError::custom` 或 `AppError::wrap` 创建的错误中取回原始类型。
    #[must_use]
    pub fn downcast_ref<E: StdError + 'static>(&self) -> Option<&E> {
        match &self.source {
            Some(Source::Domain(e)) => e.downcast_ref(),
            Some(Source::Other(e)) => e.downcast_ref(),
            None => None,
        }
    }

    /// 包装任意错误
    ///
    /// 保留原始错误的类型信息，可通过 [`AppError::downcast_ref`] 取回。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorKind;
    ///
    /// let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    /// let err = AppError::wrap(ErrorKind::Internal, "IO_ERROR", io_err);
    ///
    /// assert!(err.downcast_ref::<io::Error>().is_some());
    /// ```
    #[must_use]
    pub fn wrap<E: StdError + Send + Sync + 'static>(
        kind: ErrorKind,
        code: &'static str,
        error: E,
    ) -> Self {
        Self {
            kind,
            code,
            message: error.to_string().into(),
            source: Some(Source::Other(Box::new(error))),
        }
    }

    /// 检查错误是否匹配指定的分类和错误码
    ///
    /// 用于测试和条件判断。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_application::error::AppError;
    /// use ddd_domain::error::ErrorKind;
    ///
    /// let err = AppError::validation("invalid email");
    ///
    /// assert!(err.matches(ErrorKind::InvalidValue, "VALIDATION_ERROR"));
    /// assert!(!err.matches(ErrorKind::NotFound, "VALIDATION_ERROR"));
    /// ```
    #[must_use]
    pub fn matches(&self, kind: ErrorKind, code: &str) -> bool {
        self.kind == kind && self.code == code
    }
}

// ==================== Trait 实现 ====================

impl ErrorCode for AppError {
    fn kind(&self) -> ErrorKind {
        self.kind
    }

    fn code(&self) -> &str {
        self.code
    }
}

impl fmt::Debug for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppError")
            .field("kind", &self.kind)
            .field("code", &self.code)
            .field("message", &self.message)
            .field("source", &self.source.as_ref().map(|_| "..."))
            .finish()
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl StdError for AppError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.source {
            Some(Source::Domain(e)) => Some(e),
            Some(Source::Other(e)) => Some(e.as_ref()),
            None => None,
        }
    }
}

impl From<DomainError> for AppError {
    fn from(e: DomainError) -> Self {
        // 使用 static_code() 保留 DomainError 的自定义错误码
        let code = e.static_code();
        Self {
            kind: e.kind(),
            code,
            message: e.to_string().into(),
            source: Some(Source::Domain(e)),
        }
    }
}

// ==================== Result 类型别名 ====================

/// 应用层统一 Result 类型
pub type AppResult<T> = Result<T, AppError>;

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    // 测试 AppError 的便捷构造方法
    #[test]
    fn test_app_error_convenience_methods() {
        let err = AppError::validation("test");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        assert_eq!(err.code(), "VALIDATION_ERROR");
        assert_eq!(err.to_string(), "test");

        let err = AppError::unauthorized("no token");
        assert_eq!(err.kind(), ErrorKind::Unauthorized);
        assert_eq!(err.code(), "UNAUTHORIZED");

        let err = AppError::handler_not_found("TestHandler");
        assert_eq!(err.kind(), ErrorKind::Internal);
        assert_eq!(err.code(), "HANDLER_NOT_FOUND");
    }

    // 测试从 DomainError 转换
    #[test]
    fn test_from_domain_error() {
        let domain_err = DomainError::not_found("user 123");
        let app_err: AppError = domain_err.into();

        assert_eq!(app_err.kind(), ErrorKind::NotFound);
        assert!(app_err.domain_error().is_some());
    }

    // 测试 AppError 的 ErrorCode trait 实现
    #[test]
    fn test_app_error_implements_error_code() {
        let err = AppError::validation("invalid input");

        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        assert_eq!(err.code(), "VALIDATION_ERROR");
        assert_eq!(err.http_status(), 400);
        assert!(!err.is_retryable());
    }

    // 测试 aggregate_not_found
    #[test]
    fn test_aggregate_not_found() {
        let err = AppError::aggregate_not_found("User", "user-123");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.code(), "AGGREGATE_NOT_FOUND");
        assert_eq!(err.http_status(), 404);
        assert!(err.to_string().contains("User"));
        assert!(err.to_string().contains("user-123"));
    }

    // 测试 From<DomainError> 保留自定义 code
    #[test]
    fn test_from_domain_error_preserves_custom_code() {
        let domain_err = DomainError::not_found("user 123").with_code("USER_NOT_FOUND");
        let app_err: AppError = domain_err.into();

        assert_eq!(app_err.kind(), ErrorKind::NotFound);
        assert_eq!(app_err.code(), "USER_NOT_FOUND"); // 保留自定义 code
    }

    // 测试 wrap 方法
    #[test]
    fn test_wrap_preserves_error() {
        use std::io;

        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = AppError::wrap(ErrorKind::Internal, "IO_ERROR", io_err);

        assert_eq!(err.code(), "IO_ERROR");
        assert!(err.downcast_ref::<io::Error>().is_some());
        assert!(err.get_ref().is_some());
    }

    // 测试 downcast_ref 从 DomainError 中取回原始错误
    #[test]
    fn test_downcast_ref_through_domain_error() {
        use std::io;

        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let domain_err = DomainError::custom(ErrorKind::Internal, io_err);
        let app_err: AppError = domain_err.into();

        // 通过 AppError 取回原始 io::Error
        assert!(app_err.downcast_ref::<io::Error>().is_some());
    }

    // 测试 matches() 方法
    #[test]
    fn test_matches() {
        let err = AppError::validation("invalid email");
        assert!(err.matches(ErrorKind::InvalidValue, "VALIDATION_ERROR"));
        assert!(!err.matches(ErrorKind::NotFound, "VALIDATION_ERROR"));
        assert!(!err.matches(ErrorKind::InvalidValue, "WRONG_CODE"));

        let err = AppError::handler_not_found("TestHandler");
        assert!(err.matches(ErrorKind::Internal, "HANDLER_NOT_FOUND"));
    }
}
