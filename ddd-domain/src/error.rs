//! # 领域层统一错误定义
//!
//! 本模块提供了一套灵活、可扩展的错误处理机制，支持：
//!
//! - **统一错误协议**：通过 [`ErrorCode`] trait 定义错误的标准接口
//! - **错误分类**：通过 [`ErrorKind`] 枚举对错误进行语义分类
//! - **中间层错误**：[`DomainError`] 作为领域层的标准错误类型
//! - **无缝转换**：任何实现 [`ErrorCode`] 的错误都能方便地转换为 API 响应
//!
//! ## 架构设计
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  ddd-domain                                                     │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
//! │  │ ErrorKind   │  │ ErrorCode   │  │ DomainError             │ │
//! │  │ (分类枚举)   │  │ (trait)     │  │ impl ErrorCode ✓        │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  ddd-application                                                │
//! │  ┌───────────────────────────────────────────────────────────┐ │
//! │  │ AppError                                                  │ │
//! │  │ impl ErrorCode ✓                                          │ │
//! │  │ impl From<DomainError> ✓                                  │ │
//! │  └───────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  用户业务层                                                      │
//! │  ┌───────────────────────────────────────────────────────────┐ │
//! │  │ PayrollError / OrderError / ...                           │ │
//! │  │ impl ErrorCode ✓                                          │ │
//! │  └───────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  API 层                                                         │
//! │  ┌───────────────────────────────────────────────────────────┐ │
//! │  │ impl<E: ErrorCode> IntoResponse for ApiError<E>           │ │
//! │  │ 任意一层的错误都能直接转 HTTP 响应                         │ │
//! │  └───────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## 快速开始
//!
//! ### 1. 使用内置的 DomainError
//!
//! ```rust
//! use ddd_domain::error::{DomainError, DomainResult};
//!
//! fn validate_amount(amount: i64) -> DomainResult<()> {
//!     if amount < 0 {
//!         return Err(DomainError::invalid_value("amount must be non-negative"));
//!     }
//!     Ok(())
//! }
//!
//! fn find_user(id: &str) -> DomainResult<String> {
//!     // 模拟查询
//!     if id == "not_found" {
//!         return Err(DomainError::not_found(format!("user {id}")));
//!     }
//!     Ok(format!("User: {id}"))
//! }
//! ```
//!
//! ### 2. 自定义业务错误
//!
//! ```rust
//! use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
//! use thiserror::Error;
//!
//! #[derive(Debug, Error)]
//! pub enum PayrollError {
//!     #[error("员工不存在: {0}")]
//!     EmployeeNotFound(String),
//!
//!     #[error("工资单已锁定")]
//!     PayslipLocked,
//!
//!     #[error("金额无效: {0}")]
//!     InvalidAmount(String),
//! }
//!
//! impl ErrorCode for PayrollError {
//!     fn kind(&self) -> ErrorKind {
//!         match self {
//!             Self::EmployeeNotFound(_) => ErrorKind::NotFound,
//!             Self::PayslipLocked => ErrorKind::InvalidState,
//!             Self::InvalidAmount(_) => ErrorKind::InvalidValue,
//!         }
//!     }
//!
//!     fn code(&self) -> &str {
//!         match self {
//!             Self::EmployeeNotFound(_) => "EMPLOYEE_NOT_FOUND",
//!             Self::PayslipLocked => "PAYSLIP_LOCKED",
//!             Self::InvalidAmount(_) => "INVALID_AMOUNT",
//!         }
//!     }
//! }
//!
//! // 可选：转换为 DomainError
//! impl From<PayrollError> for DomainError {
//!     fn from(e: PayrollError) -> Self {
//!         DomainError::custom(e.kind(), e)
//!     }
//! }
//! ```
//!
//! ### 3. API 层转换（以 Axum 为例）
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
//! /// 包装器：任何实现 ErrorCode 的错误 -> HTTP 响应
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
//!
//! ## 设计原则
//!
//! 1. **以终为始**：所有错误最终都要转为 API 响应，设计围绕这个目标展开
//! 2. **统一协议**：[`ErrorCode`] trait 是唯一的「接口契约」
//! 3. **灵活扩展**：用户可以定义任意错误类型，只需实现 [`ErrorCode`]
//! 4. **中间层价值**：[`DomainError`] 提供开箱即用的领域错误，减少样板代码
//! 5. **类型安全**：通过 `downcast_ref` 可在需要时取回原始错误类型

use std::{error::Error as StdError, fmt};

// ==================== 错误分类 ====================

/// 错误分类枚举
///
/// 用于统一处理错误、映射 HTTP 状态码、决定是否重试等。
///
/// # 示例
///
/// ```rust
/// use ddd_domain::error::ErrorKind;
///
/// let kind = ErrorKind::NotFound;
/// assert_eq!(kind.http_status(), 404);
/// assert_eq!(kind.default_code(), "NOT_FOUND");
/// assert!(!kind.is_retryable());
///
/// let conflict = ErrorKind::Conflict;
/// assert!(conflict.is_retryable());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// 值对象验证失败（如：金额为负数、邮箱格式错误）
    InvalidValue,
    /// 聚合状态不允许执行该操作（如：已关闭的订单不能修改）
    InvalidState,
    /// 命令参数或前置条件不满足（如：库存不足）
    InvalidCommand,
    /// 资源不存在（如：用户不存在、订单不存在）
    NotFound,
    /// 乐观锁/版本冲突（可重试）
    Conflict,
    /// 未授权访问
    Unauthorized,
    /// 内部错误（数据库、序列化等基础设施错误）
    Internal,
}

impl ErrorKind {
    /// 映射到 HTTP 状态码
    ///
    /// | ErrorKind       | HTTP Status |
    /// |-----------------|-------------|
    /// | InvalidValue    | 400         |
    /// | InvalidCommand  | 400         |
    /// | Unauthorized    | 401         |
    /// | NotFound        | 404         |
    /// | Conflict        | 409         |
    /// | InvalidState    | 422         |
    /// | Internal        | 500         |
    #[must_use]
    pub const fn http_status(self) -> u16 {
        match self {
            Self::InvalidValue | Self::InvalidCommand => 400,
            Self::Unauthorized => 401,
            Self::NotFound => 404,
            Self::Conflict => 409,
            Self::InvalidState => 422,
            Self::Internal => 500,
        }
    }

    /// 获取默认错误码
    ///
    /// 返回大写下划线格式的错误码，如 `"NOT_FOUND"`、`"INVALID_VALUE"`。
    #[must_use]
    pub const fn default_code(self) -> &'static str {
        match self {
            Self::InvalidValue => "INVALID_VALUE",
            Self::InvalidState => "INVALID_STATE",
            Self::InvalidCommand => "INVALID_COMMAND",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Internal => "INTERNAL_ERROR",
        }
    }

    /// 是否可重试
    ///
    /// 目前只有 [`ErrorKind::Conflict`] 返回 `true`，表示乐观锁冲突可以重试。
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Conflict)
    }

    /// 获取用户友好的错误消息
    ///
    /// 当 [`DomainError`] 没有具体消息时，使用此默认消息。
    #[must_use]
    pub const fn default_message(self) -> &'static str {
        match self {
            Self::InvalidValue => "the provided value is invalid",
            Self::InvalidState => "the current state does not allow this operation",
            Self::InvalidCommand => "the command cannot be executed",
            Self::NotFound => "the requested resource was not found",
            Self::Conflict => "a version conflict occurred, please retry",
            Self::Unauthorized => "access denied",
            Self::Internal => "an internal error occurred",
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.default_message())
    }
}

// ==================== 错误协议 ====================

/// 错误协议 trait
///
/// 所有错误类型实现此 trait，即可：
/// - 获取错误分类 ([`ErrorCode::kind`])
/// - 获取错误码 ([`ErrorCode::code`])
/// - 映射到 HTTP 状态码 ([`ErrorCode::http_status`])
/// - 判断是否可重试 ([`ErrorCode::is_retryable`])
///
/// # 示例
///
/// ```rust
/// use ddd_domain::error::{ErrorCode, ErrorKind};
/// use thiserror::Error;
///
/// #[derive(Debug, Error)]
/// #[error("订单已取消")]
/// struct OrderCancelled;
///
/// impl ErrorCode for OrderCancelled {
///     fn kind(&self) -> ErrorKind {
///         ErrorKind::InvalidState
///     }
///
///     fn code(&self) -> &str {
///         "ORDER_CANCELLED"
///     }
/// }
///
/// let err = OrderCancelled;
/// assert_eq!(err.kind(), ErrorKind::InvalidState);
/// assert_eq!(err.code(), "ORDER_CANCELLED");
/// assert_eq!(err.http_status(), 422);
/// ```
pub trait ErrorCode: StdError + Send + Sync + 'static {
    /// 返回错误分类
    fn kind(&self) -> ErrorKind;

    /// 返回错误码（默认使用 [`ErrorKind::default_code`]）
    fn code(&self) -> &str {
        self.kind().default_code()
    }

    /// 返回 HTTP 状态码（默认使用 [`ErrorKind::http_status`]）
    fn http_status(&self) -> u16 {
        self.kind().http_status()
    }

    /// 是否可重试（默认使用 [`ErrorKind::is_retryable`]）
    fn is_retryable(&self) -> bool {
        self.kind().is_retryable()
    }
}

// ==================== DomainError ====================

/// 领域层统一错误类型
///
/// 借鉴 [`std::io::Error`] 的设计，提供：
/// - 简单错误（只有分类）
/// - 带消息的错误
/// - 包装自定义错误（保留类型信息）
///
/// # 构造方式
///
/// ## 便捷方法
///
/// ```rust
/// use ddd_domain::error::DomainError;
///
/// // 值无效
/// let err = DomainError::invalid_value("金额不能为负数");
///
/// // 状态无效
/// let err = DomainError::invalid_state("订单已关闭，不能修改");
///
/// // 资源不存在
/// let err = DomainError::not_found("用户 123");
///
/// // 版本冲突
/// let err = DomainError::conflict(1, 2);
/// ```
///
/// ## 通用方法
///
/// ```rust
/// use ddd_domain::error::{DomainError, ErrorKind};
///
/// // 指定分类和消息
/// let err = DomainError::new(ErrorKind::InvalidCommand, "库存不足");
///
/// // 自定义错误码
/// let err = DomainError::new(ErrorKind::NotFound, "用户不存在").with_code("USER_NOT_FOUND");
/// ```
///
/// ## 包装自定义错误
///
/// ```rust
/// use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
/// use thiserror::Error;
///
/// #[derive(Debug, Error)]
/// #[error("自定义错误")]
/// struct MyError;
///
/// let err = DomainError::custom(ErrorKind::Internal, MyError);
///
/// // 可以取回原始错误
/// assert!(err.downcast_ref::<MyError>().is_some());
/// ```
pub struct DomainError {
    kind: ErrorKind,
    code: Option<&'static str>,
    repr: Repr,
}

enum Repr {
    /// 简单错误：只有分类
    Simple,
    /// 带消息的错误
    Message(Box<str>),
    /// 包装自定义错误
    Custom(Box<dyn StdError + Send + Sync>),
}

impl DomainError {
    // ==================== 基础构造 ====================

    /// 从分类创建简单错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorKind};
    ///
    /// let err = DomainError::from_kind(ErrorKind::NotFound);
    /// assert_eq!(err.kind(), ErrorKind::NotFound);
    /// ```
    #[must_use]
    pub const fn from_kind(kind: ErrorKind) -> Self {
        Self {
            kind,
            code: None,
            repr: Repr::Simple,
        }
    }

    /// 创建带消息的错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorKind};
    ///
    /// let err = DomainError::new(ErrorKind::InvalidValue, "金额必须为正数");
    /// assert_eq!(err.to_string(), "金额必须为正数");
    /// ```
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            code: None,
            repr: Repr::Message(message.into()),
        }
    }

    /// 包装自定义错误
    ///
    /// 保留原始错误的类型信息，可通过 [`DomainError::downcast_ref`] 取回。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ddd_domain::error::{DomainError, ErrorKind};
    ///
    /// let io_err = io::Error::new(io::ErrorKind::NotFound, "文件不存在");
    /// let err = DomainError::custom(ErrorKind::Internal, io_err);
    ///
    /// // 取回原始错误
    /// let inner = err.downcast_ref::<io::Error>().unwrap();
    /// assert_eq!(inner.kind(), io::ErrorKind::NotFound);
    /// ```
    #[must_use]
    pub fn custom<E>(kind: ErrorKind, error: E) -> Self
    where
        E: StdError + Send + Sync + 'static,
    {
        Self {
            kind,
            code: None,
            repr: Repr::Custom(Box::new(error)),
        }
    }

    /// 设置自定义错误码
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
    ///
    /// let err = DomainError::not_found("用户 123").with_code("USER_NOT_FOUND");
    ///
    /// assert_eq!(err.code(), "USER_NOT_FOUND");
    /// ```
    #[must_use]
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    // ==================== 便捷构造 ====================

    /// 创建「值无效」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
    ///
    /// let err = DomainError::invalid_value("金额必须为正数");
    /// assert_eq!(err.kind(), ErrorKind::InvalidValue);
    /// assert_eq!(err.http_status(), 400);
    /// ```
    #[must_use]
    pub fn invalid_value(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidValue, msg)
    }

    /// 创建「状态无效」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
    ///
    /// let err = DomainError::invalid_state("订单已关闭");
    /// assert_eq!(err.kind(), ErrorKind::InvalidState);
    /// assert_eq!(err.http_status(), 422);
    /// ```
    #[must_use]
    pub fn invalid_state(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidState, msg)
    }

    /// 创建「命令无效」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
    ///
    /// let err = DomainError::invalid_command("库存不足");
    /// assert_eq!(err.kind(), ErrorKind::InvalidCommand);
    /// assert_eq!(err.http_status(), 400);
    /// ```
    #[must_use]
    pub fn invalid_command(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidCommand, msg)
    }

    /// 创建「资源不存在」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
    ///
    /// let err = DomainError::not_found("用户 123");
    /// assert_eq!(err.kind(), ErrorKind::NotFound);
    /// assert_eq!(err.http_status(), 404);
    /// ```
    #[must_use]
    pub fn not_found(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::NotFound, msg)
    }

    /// 创建「版本冲突」错误
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::DomainError;
    ///
    /// // 支持任意可 Display 的类型
    /// let err = DomainError::conflict(1_u64, 2_u64);
    /// let err = DomainError::conflict(1_usize, 2_usize);
    /// let err = DomainError::conflict("v1", "v2");
    /// ```
    #[must_use]
    pub fn conflict(expected: impl fmt::Display, actual: impl fmt::Display) -> Self {
        Self::new(
            ErrorKind::Conflict,
            format!("version conflict: expected={expected}, actual={actual}"),
        )
    }

    /// 创建「内部错误」
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorCode, ErrorKind};
    ///
    /// let err = DomainError::internal("数据库连接失败");
    /// assert_eq!(err.kind(), ErrorKind::Internal);
    /// assert_eq!(err.http_status(), 500);
    /// ```
    #[must_use]
    pub fn internal(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Internal, msg)
    }

    /// 创建「事件上抬失败」错误
    #[must_use]
    pub fn upcast_failed(
        event_type: impl Into<Box<str>>,
        from_version: usize,
        stage: Option<&'static str>,
        reason: impl Into<Box<str>>,
    ) -> Self {
        let event_type = event_type.into();
        let reason = reason.into();
        let msg = match stage {
            Some(s) => format!(
                "upcast failed: type={event_type}, from_version={from_version}, stage={s}, \
                 reason={reason}"
            ),
            None => format!(
                "upcast failed: type={event_type}, from_version={from_version}, reason={reason}"
            ),
        };
        Self::new(ErrorKind::Internal, msg).with_code("UPCAST_FAILED")
    }

    /// 创建「类型不匹配」错误
    #[must_use]
    pub fn type_mismatch(expected: impl Into<Box<str>>, found: impl Into<Box<str>>) -> Self {
        let expected = expected.into();
        let found = found.into();
        Self::new(
            ErrorKind::Internal,
            format!("type mismatch: expected={expected}, found={found}"),
        )
        .with_code("TYPE_MISMATCH")
    }

    /// 创建「事件总线」错误
    #[must_use]
    pub fn event_bus(reason: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Internal, reason).with_code("EVENT_BUS_ERROR")
    }

    // ==================== 查询方法 ====================

    /// 获取错误分类
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// 尝试向下转型为具体错误类型
    ///
    /// 仅当错误是通过 [`DomainError::custom`] 创建时有效。
    #[must_use]
    pub fn downcast_ref<E: StdError + 'static>(&self) -> Option<&E> {
        match &self.repr {
            Repr::Custom(error) => error.downcast_ref(),
            _ => None,
        }
    }

    /// 获取内部错误引用
    #[must_use]
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match &self.repr {
            Repr::Custom(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    /// 获取静态生命周期的错误码
    ///
    /// 与 [`ErrorCode::code`] 不同，此方法返回 `&'static str`，
    /// 适用于需要将错误码存储到其他结构中的场景。
    #[must_use]
    pub fn static_code(&self) -> &'static str {
        self.code.unwrap_or_else(|| self.kind.default_code())
    }

    /// 检查错误是否匹配指定的分类和错误码
    ///
    /// 用于测试和条件判断。
    ///
    /// # 示例
    ///
    /// ```rust
    /// use ddd_domain::error::{DomainError, ErrorKind};
    ///
    /// let err = DomainError::not_found("user").with_code("USER_NOT_FOUND");
    ///
    /// assert!(err.matches(ErrorKind::NotFound, "USER_NOT_FOUND"));
    /// assert!(!err.matches(ErrorKind::NotFound, "NOT_FOUND"));
    /// assert!(!err.matches(ErrorKind::Internal, "USER_NOT_FOUND"));
    /// ```
    #[must_use]
    pub fn matches(&self, kind: ErrorKind, code: &str) -> bool {
        self.kind == kind && self.static_code() == code
    }
}

// ==================== Trait 实现 ====================

impl ErrorCode for DomainError {
    fn kind(&self) -> ErrorKind {
        self.kind
    }

    fn code(&self) -> &str {
        self.static_code()
    }
}

impl fmt::Debug for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("DomainError");
        d.field("kind", &self.kind);
        if let Some(code) = self.code {
            d.field("code", &code);
        }
        match &self.repr {
            Repr::Simple => {
                d.field("message", &self.kind.default_message());
            }
            Repr::Message(msg) => {
                d.field("message", msg);
            }
            Repr::Custom(err) => {
                d.field("source", err);
            }
        }
        d.finish()
    }
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.repr {
            Repr::Simple => write!(f, "{}", self.kind.default_message()),
            Repr::Message(msg) => write!(f, "{msg}"),
            Repr::Custom(err) => write!(f, "{err}"),
        }
    }
}

impl StdError for DomainError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.repr {
            Repr::Custom(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

impl From<ErrorKind> for DomainError {
    fn from(kind: ErrorKind) -> Self {
        Self::from_kind(kind)
    }
}

// ==================== 常用类型转换 ====================

impl From<serde_json::Error> for DomainError {
    fn from(err: serde_json::Error) -> Self {
        Self::custom(ErrorKind::Internal, err).with_code("SERIALIZATION_ERROR")
    }
}

impl From<uuid::Error> for DomainError {
    fn from(err: uuid::Error) -> Self {
        Self::custom(ErrorKind::InvalidValue, err).with_code("INVALID_UUID")
    }
}

impl From<std::num::ParseIntError> for DomainError {
    fn from(err: std::num::ParseIntError) -> Self {
        Self::custom(ErrorKind::InvalidValue, err).with_code("PARSE_INT_ERROR")
    }
}

impl From<std::num::ParseFloatError> for DomainError {
    fn from(err: std::num::ParseFloatError) -> Self {
        Self::custom(ErrorKind::InvalidValue, err).with_code("PARSE_FLOAT_ERROR")
    }
}

impl From<std::str::ParseBoolError> for DomainError {
    fn from(err: std::str::ParseBoolError) -> Self {
        Self::custom(ErrorKind::InvalidValue, err).with_code("PARSE_BOOL_ERROR")
    }
}

impl From<chrono::ParseError> for DomainError {
    fn from(err: chrono::ParseError) -> Self {
        Self::custom(ErrorKind::InvalidValue, err).with_code("PARSE_DATE_ERROR")
    }
}

impl From<anyhow::Error> for DomainError {
    fn from(err: anyhow::Error) -> Self {
        // 使用 {:#} 格式保留完整错误链
        Self::new(ErrorKind::Internal, format!("{err:#}"))
    }
}

#[cfg(feature = "infra-sqlx")]
impl From<sqlx::Error> for DomainError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => {
                Self::new(ErrorKind::NotFound, "database row not found").with_code("ROW_NOT_FOUND")
            }
            other => Self::custom(ErrorKind::Internal, other).with_code("DATABASE_ERROR"),
        }
    }
}

// ==================== Result 类型别名 ====================

/// 领域层统一 Result 类型
pub type DomainResult<T> = Result<T, DomainError>;

// ==================== 测试 ====================

#[cfg(test)]
mod tests {
    use super::*;

    // 测试 ErrorKind 的 HTTP 状态码映射
    #[test]
    fn test_error_kind_http_status() {
        assert_eq!(ErrorKind::InvalidValue.http_status(), 400);
        assert_eq!(ErrorKind::InvalidCommand.http_status(), 400);
        assert_eq!(ErrorKind::Unauthorized.http_status(), 401);
        assert_eq!(ErrorKind::NotFound.http_status(), 404);
        assert_eq!(ErrorKind::Conflict.http_status(), 409);
        assert_eq!(ErrorKind::InvalidState.http_status(), 422);
        assert_eq!(ErrorKind::Internal.http_status(), 500);
    }

    // 测试 ErrorKind 的默认错误码
    #[test]
    fn test_error_kind_default_code() {
        assert_eq!(ErrorKind::InvalidValue.default_code(), "INVALID_VALUE");
        assert_eq!(ErrorKind::NotFound.default_code(), "NOT_FOUND");
        assert_eq!(ErrorKind::Conflict.default_code(), "CONFLICT");
    }

    // 测试 ErrorKind 的可重试判断
    #[test]
    fn test_error_kind_retryable() {
        assert!(!ErrorKind::InvalidValue.is_retryable());
        assert!(!ErrorKind::NotFound.is_retryable());
        assert!(ErrorKind::Conflict.is_retryable());
    }

    // 测试 DomainError 的便捷构造方法
    #[test]
    fn test_domain_error_convenience_methods() {
        let err = DomainError::invalid_value("test");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        assert_eq!(err.to_string(), "test");

        let err = DomainError::not_found("user 123");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.code(), "NOT_FOUND");
    }

    // 测试 DomainError 的自定义错误码
    #[test]
    fn test_domain_error_custom_code() {
        let err = DomainError::not_found("user").with_code("USER_NOT_FOUND");
        assert_eq!(err.code(), "USER_NOT_FOUND");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    // 测试 DomainError 包装自定义错误
    #[test]
    fn test_domain_error_custom_error() {
        use std::io;

        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = DomainError::custom(ErrorKind::Internal, io_err);

        assert!(err.downcast_ref::<io::Error>().is_some());
        assert!(err.source().is_some());
    }

    // 测试 DomainError 的 ErrorCode trait 实现
    #[test]
    fn test_domain_error_implements_error_code() {
        let err = DomainError::invalid_state("order closed");

        // ErrorCode trait 方法
        assert_eq!(err.kind(), ErrorKind::InvalidState);
        assert_eq!(err.code(), "INVALID_STATE");
        assert_eq!(err.http_status(), 422);
        assert!(!err.is_retryable());
    }

    // 测试从 ErrorKind 转换
    #[test]
    fn test_from_error_kind() {
        let err: DomainError = ErrorKind::NotFound.into();
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    // 测试用户自定义错误实现 ErrorCode
    #[test]
    fn test_user_custom_error() {
        #[derive(Debug)]
        struct MyError;

        impl fmt::Display for MyError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "my error")
            }
        }

        impl StdError for MyError {}

        impl ErrorCode for MyError {
            fn kind(&self) -> ErrorKind {
                ErrorKind::InvalidValue
            }

            fn code(&self) -> &str {
                "MY_ERROR"
            }
        }

        let err = MyError;
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        assert_eq!(err.code(), "MY_ERROR");
        assert_eq!(err.http_status(), 400);
    }

    // 测试 ErrorKind 的友好消息
    #[test]
    fn test_error_kind_default_message() {
        assert_eq!(
            ErrorKind::InvalidValue.default_message(),
            "the provided value is invalid"
        );
        assert_eq!(
            ErrorKind::NotFound.default_message(),
            "the requested resource was not found"
        );
        assert_eq!(
            ErrorKind::Conflict.default_message(),
            "a version conflict occurred, please retry"
        );
    }

    // 测试 Repr::Simple 显示友好消息
    #[test]
    fn test_simple_error_display() {
        let err = DomainError::from_kind(ErrorKind::NotFound);
        assert_eq!(err.to_string(), "the requested resource was not found");

        let err = DomainError::from_kind(ErrorKind::Internal);
        assert_eq!(err.to_string(), "an internal error occurred");
    }

    // 测试 matches() 方法
    #[test]
    fn test_matches() {
        let err = DomainError::not_found("user").with_code("USER_NOT_FOUND");
        assert!(err.matches(ErrorKind::NotFound, "USER_NOT_FOUND"));
        assert!(!err.matches(ErrorKind::NotFound, "NOT_FOUND"));
        assert!(!err.matches(ErrorKind::Internal, "USER_NOT_FOUND"));

        // 使用默认 code
        let err = DomainError::invalid_value("bad input");
        assert!(err.matches(ErrorKind::InvalidValue, "INVALID_VALUE"));
    }

    // 测试 From<anyhow::Error> 保留错误链
    #[test]
    fn test_from_anyhow_preserves_error_chain() {
        use std::io;

        let root = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let anyhow_err = anyhow::Error::new(root).context("failed to load config");
        let domain_err: DomainError = anyhow_err.into();

        let msg = domain_err.to_string();
        // {:#} 格式应保留完整错误链
        assert!(msg.contains("failed to load config"), "msg: {msg}");
        assert!(msg.contains("file not found"), "msg: {msg}");
        assert_eq!(domain_err.kind(), ErrorKind::Internal);
    }
}
