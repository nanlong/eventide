//! # Unified domain-layer error definitions.
//!
//! This module provides a flexible, extensible error-handling toolkit
//! for DDD projects. It is built around four ideas:
//!
//! - **Unified error contract** — every error type opts in via the
//!   [`ErrorCode`] trait, which gives a consistent API regardless of the
//!   concrete type.
//! - **Semantic categorisation** — the [`ErrorKind`] enum classifies every
//!   error so cross-cutting concerns (HTTP mapping, retry decisions,
//!   logging) can act on the category instead of pattern-matching every
//!   variant.
//! - **Drop-in middle layer** — [`DomainError`] is a ready-to-use error
//!   type for the domain layer, modelled after [`std::io::Error`] so it
//!   can carry a category, an optional code, and either a message or a
//!   wrapped custom error without losing type information.
//! - **Seamless conversion** — any error implementing [`ErrorCode`] can be
//!   turned into an HTTP response or higher-layer error type with no
//!   bespoke glue.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  eventide-domain                                                 │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
//! │  │ ErrorKind   │  │ ErrorCode   │  │ DomainError             │ │
//! │  │ (categories)│  │ (trait)     │  │ impl ErrorCode ✓        │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  eventide-application                                            │
//! │  ┌───────────────────────────────────────────────────────────┐ │
//! │  │ AppError                                                  │ │
//! │  │ impl ErrorCode ✓                                          │ │
//! │  │ impl From<DomainError> ✓                                  │ │
//! │  └───────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  user business layer                                            │
//! │  ┌───────────────────────────────────────────────────────────┐ │
//! │  │ PayrollError / OrderError / ...                           │ │
//! │  │ impl ErrorCode ✓                                          │ │
//! │  └───────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  API layer                                                      │
//! │  ┌───────────────────────────────────────────────────────────┐ │
//! │  │ impl<E: ErrorCode> IntoResponse for ApiError<E>           │ │
//! │  │ any layer's error becomes an HTTP response                │ │
//! │  └───────────────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick start
//!
//! ### 1. Use the built-in `DomainError`
//!
//! ```rust
//! use eventide_domain::error::{DomainError, DomainResult};
//!
//! fn validate_amount(amount: i64) -> DomainResult<()> {
//!     if amount < 0 {
//!         return Err(DomainError::invalid_value("amount must be non-negative"));
//!     }
//!     Ok(())
//! }
//!
//! fn find_user(id: &str) -> DomainResult<String> {
//!     // simulate a lookup
//!     if id == "not_found" {
//!         return Err(DomainError::not_found(format!("user {id}")));
//!     }
//!     Ok(format!("User: {id}"))
//! }
//! ```
//!
//! ### 2. Define your own business error
//!
//! ```rust
//! use eventide_domain::error::{ErrorCode, ErrorKind, DomainError};
//! use thiserror::Error;
//!
//! #[derive(Debug, Error)]
//! pub enum PayrollError {
//!     #[error("employee not found: {0}")]
//!     EmployeeNotFound(String),
//!
//!     #[error("payslip is locked")]
//!     PayslipLocked,
//!
//!     #[error("invalid amount: {0}")]
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
//! // Optional: convert into a `DomainError`.
//! impl From<PayrollError> for DomainError {
//!     fn from(e: PayrollError) -> Self {
//!         DomainError::custom(e.kind(), e)
//!     }
//! }
//! ```
//!
//! ### 3. Convert into an HTTP response (Axum example)
//!
//! ```rust,ignore
//! use axum::response::{IntoResponse, Response};
//! use axum::http::StatusCode;
//! use axum::Json;
//! use eventide_domain::error::ErrorCode;
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! pub struct ApiErrorResponse {
//!     pub code: String,
//!     pub message: String,
//! }
//!
//! /// Wrapper turning any `ErrorCode` into an HTTP response.
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
//! ## Design principles
//!
//! 1. **Begin with the end in mind** — every error eventually becomes an
//!    API response, so the design is shaped around that target.
//! 2. **One contract** — [`ErrorCode`] is the single interface every error
//!    type implements.
//! 3. **Open for extension** — users can define any error type they like as
//!    long as it implements [`ErrorCode`].
//! 4. **A useful middle layer** — [`DomainError`] removes boilerplate by
//!    providing an out-of-the-box error for the domain layer.
//! 5. **Type-safe** — the original error type can be retrieved with
//!    `downcast_ref` whenever it is needed.

use std::error::Error as StdError;
use std::fmt;

// ==================== Error categories ====================

/// Error category enum.
///
/// Used for unified handling: mapping to HTTP status codes, deciding
/// whether to retry, choosing log levels, and so on.
///
/// # Examples
///
/// ```rust
/// use eventide_domain::error::ErrorKind;
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
    /// Value-object validation failed (for example: a negative amount, a
    /// malformed email).
    InvalidValue,
    /// The aggregate's current state forbids the operation (for example:
    /// trying to modify a closed order).
    InvalidState,
    /// Command parameters or preconditions are not satisfied (for example:
    /// insufficient stock).
    InvalidCommand,
    /// The requested resource does not exist (for example: user / order
    /// not found).
    NotFound,
    /// Optimistic-locking / version conflict (callers may retry).
    Conflict,
    /// Unauthorised access.
    Unauthorized,
    /// Internal error (database, serialization, and other infrastructure
    /// failures).
    Internal,
}

impl ErrorKind {
    /// Map the error kind to its HTTP status code.
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

    /// Return the default error code for this category.
    ///
    /// Codes are upper-snake-case strings such as `"NOT_FOUND"` or
    /// `"INVALID_VALUE"`.
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

    /// Whether the error is safe to retry.
    ///
    /// Currently only [`ErrorKind::Conflict`] returns `true`, indicating
    /// that an optimistic-locking conflict can be retried.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        matches!(self, Self::Conflict)
    }

    /// Default user-facing message used by [`DomainError`] when no
    /// explicit message has been attached.
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

// ==================== Error contract ====================

/// Error-contract trait.
///
/// Implement this trait once and any error type gains the ability to:
///
/// - report its category via [`ErrorCode::kind`];
/// - report its error code via [`ErrorCode::code`];
/// - map to an HTTP status via [`ErrorCode::http_status`];
/// - report whether it is retryable via [`ErrorCode::is_retryable`].
///
/// # Examples
///
/// ```rust
/// use eventide_domain::error::{ErrorCode, ErrorKind};
/// use thiserror::Error;
///
/// #[derive(Debug, Error)]
/// #[error("order has been cancelled")]
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
    /// Return the error's category.
    fn kind(&self) -> ErrorKind;

    /// Return the error code (defaults to [`ErrorKind::default_code`]).
    fn code(&self) -> &str {
        self.kind().default_code()
    }

    /// Return the HTTP status code (defaults to [`ErrorKind::http_status`]).
    fn http_status(&self) -> u16 {
        self.kind().http_status()
    }

    /// Whether the error is retryable (defaults to [`ErrorKind::is_retryable`]).
    fn is_retryable(&self) -> bool {
        self.kind().is_retryable()
    }
}

// ==================== DomainError ====================

/// Unified error type for the domain layer.
///
/// Modelled after [`std::io::Error`], [`DomainError`] supports three
/// representations:
///
/// - a simple error that only carries a category;
/// - an error with a custom message;
/// - an error wrapping another error type, preserving the original
///   concrete type so it can be downcast later.
///
/// # Construction
///
/// ## Convenience constructors
///
/// ```rust
/// use eventide_domain::error::DomainError;
///
/// // Invalid value
/// let err = DomainError::invalid_value("amount must be non-negative");
///
/// // Invalid state
/// let err = DomainError::invalid_state("order is closed and cannot be modified");
///
/// // Resource not found
/// let err = DomainError::not_found("user 123");
///
/// // Version conflict
/// let err = DomainError::conflict(1, 2);
/// ```
///
/// ## Generic constructor
///
/// ```rust
/// use eventide_domain::error::{DomainError, ErrorKind};
///
/// // Specify both category and message
/// let err = DomainError::new(ErrorKind::InvalidCommand, "insufficient stock");
///
/// // Custom error code
/// let err = DomainError::new(ErrorKind::NotFound, "user not found")
///     .with_code("USER_NOT_FOUND");
/// ```
///
/// ## Wrapping a custom error
///
/// ```rust
/// use eventide_domain::error::{DomainError, ErrorKind, ErrorCode};
/// use thiserror::Error;
///
/// #[derive(Debug, Error)]
/// #[error("custom error")]
/// struct MyError;
///
/// let err = DomainError::custom(ErrorKind::Internal, MyError);
///
/// // The original error can be retrieved.
/// assert!(err.downcast_ref::<MyError>().is_some());
/// ```
pub struct DomainError {
    kind: ErrorKind,
    code: Option<&'static str>,
    repr: Repr,
}

enum Repr {
    /// Simple error: category only, no extra payload.
    Simple,
    /// Error with an attached human-readable message.
    Message(Box<str>),
    /// Error wrapping a custom error type.
    Custom(Box<dyn StdError + Send + Sync>),
}

impl DomainError {
    // ==================== Basic constructors ====================

    /// Build a simple error from a category alone.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind};
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

    /// Build an error with a human-readable message.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind};
    ///
    /// let err = DomainError::new(ErrorKind::InvalidValue, "amount must be positive");
    /// assert_eq!(err.to_string(), "amount must be positive");
    /// ```
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            code: None,
            repr: Repr::Message(message.into()),
        }
    }

    /// Wrap an existing custom error.
    ///
    /// The original error's concrete type is preserved and can be
    /// retrieved via [`DomainError::downcast_ref`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind};
    /// use std::io;
    ///
    /// let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
    /// let err = DomainError::custom(ErrorKind::Internal, io_err);
    ///
    /// // Retrieve the original error.
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

    /// Attach a custom error code.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind, ErrorCode};
    ///
    /// let err = DomainError::not_found("user 123")
    ///     .with_code("USER_NOT_FOUND");
    ///
    /// assert_eq!(err.code(), "USER_NOT_FOUND");
    /// ```
    #[must_use]
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    // ==================== Convenience constructors ====================

    /// Create an [`ErrorKind::InvalidValue`] error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind, ErrorCode};
    ///
    /// let err = DomainError::invalid_value("amount must be positive");
    /// assert_eq!(err.kind(), ErrorKind::InvalidValue);
    /// assert_eq!(err.http_status(), 400);
    /// ```
    #[must_use]
    pub fn invalid_value(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidValue, msg)
    }

    /// Create an [`ErrorKind::InvalidState`] error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind, ErrorCode};
    ///
    /// let err = DomainError::invalid_state("order is closed");
    /// assert_eq!(err.kind(), ErrorKind::InvalidState);
    /// assert_eq!(err.http_status(), 422);
    /// ```
    #[must_use]
    pub fn invalid_state(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidState, msg)
    }

    /// Create an [`ErrorKind::InvalidCommand`] error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind, ErrorCode};
    ///
    /// let err = DomainError::invalid_command("insufficient stock");
    /// assert_eq!(err.kind(), ErrorKind::InvalidCommand);
    /// assert_eq!(err.http_status(), 400);
    /// ```
    #[must_use]
    pub fn invalid_command(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidCommand, msg)
    }

    /// Create an [`ErrorKind::NotFound`] error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind, ErrorCode};
    ///
    /// let err = DomainError::not_found("user 123");
    /// assert_eq!(err.kind(), ErrorKind::NotFound);
    /// assert_eq!(err.http_status(), 404);
    /// ```
    #[must_use]
    pub fn not_found(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::NotFound, msg)
    }

    /// Create an [`ErrorKind::Conflict`] error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::DomainError;
    ///
    /// // Accepts any `Display` types.
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

    /// Create an [`ErrorKind::Internal`] error.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind, ErrorCode};
    ///
    /// let err = DomainError::internal("database connection failed");
    /// assert_eq!(err.kind(), ErrorKind::Internal);
    /// assert_eq!(err.http_status(), 500);
    /// ```
    #[must_use]
    pub fn internal(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Internal, msg)
    }

    /// Create an "event upcast failed" error.
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
                "upcast failed: type={event_type}, from_version={from_version}, stage={s}, reason={reason}"
            ),
            None => format!(
                "upcast failed: type={event_type}, from_version={from_version}, reason={reason}"
            ),
        };
        Self::new(ErrorKind::Internal, msg).with_code("UPCAST_FAILED")
    }

    /// Create a "type mismatch" error.
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

    /// Create an "event bus" error.
    #[must_use]
    pub fn event_bus(reason: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Internal, reason).with_code("EVENT_BUS_ERROR")
    }

    // ==================== Inspection helpers ====================

    /// Return the error category.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Try to downcast this error into a concrete type.
    ///
    /// Only succeeds when the error was constructed via
    /// [`DomainError::custom`].
    #[must_use]
    pub fn downcast_ref<E: StdError + 'static>(&self) -> Option<&E> {
        match &self.repr {
            Repr::Custom(error) => error.downcast_ref(),
            _ => None,
        }
    }

    /// Return a reference to the wrapped inner error, if any.
    #[must_use]
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match &self.repr {
            Repr::Custom(error) => Some(error.as_ref()),
            _ => None,
        }
    }

    /// Return the error code as a `&'static str`.
    ///
    /// Unlike [`ErrorCode::code`] this method returns `&'static str`,
    /// which is convenient when the code needs to be stored elsewhere
    /// without lifetime gymnastics.
    #[must_use]
    pub fn static_code(&self) -> &'static str {
        self.code.unwrap_or_else(|| self.kind.default_code())
    }

    /// Check whether this error matches the given category and code.
    ///
    /// Useful in tests and conditional logic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_domain::error::{DomainError, ErrorKind};
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

// ==================== Trait implementations ====================

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

// ==================== Common type conversions ====================

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
        // Use the `{:#}` format specifier to preserve the full error chain.
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

// ==================== Result type alias ====================

/// Unified `Result` alias for the domain layer.
pub type DomainResult<T> = Result<T, DomainError>;

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    // Verify the HTTP-status mapping for every `ErrorKind` variant.
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

    // Verify the default error-code strings.
    #[test]
    fn test_error_kind_default_code() {
        assert_eq!(ErrorKind::InvalidValue.default_code(), "INVALID_VALUE");
        assert_eq!(ErrorKind::NotFound.default_code(), "NOT_FOUND");
        assert_eq!(ErrorKind::Conflict.default_code(), "CONFLICT");
    }

    // Verify the retryable flag — only `Conflict` should be retryable.
    #[test]
    fn test_error_kind_retryable() {
        assert!(!ErrorKind::InvalidValue.is_retryable());
        assert!(!ErrorKind::NotFound.is_retryable());
        assert!(ErrorKind::Conflict.is_retryable());
    }

    // Verify the convenience constructors set the right kind and message.
    #[test]
    fn test_domain_error_convenience_methods() {
        let err = DomainError::invalid_value("test");
        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        assert_eq!(err.to_string(), "test");

        let err = DomainError::not_found("user 123");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.code(), "NOT_FOUND");
    }

    // Verify that `with_code` overrides the default code.
    #[test]
    fn test_domain_error_custom_code() {
        let err = DomainError::not_found("user").with_code("USER_NOT_FOUND");
        assert_eq!(err.code(), "USER_NOT_FOUND");
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    // Verify that wrapping a custom error preserves both the inner type
    // (for downcasting) and the standard `Error::source` chain.
    #[test]
    fn test_domain_error_custom_error() {
        use std::io;

        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = DomainError::custom(ErrorKind::Internal, io_err);

        assert!(err.downcast_ref::<io::Error>().is_some());
        assert!(err.source().is_some());
    }

    // Verify that `DomainError` itself implements `ErrorCode` and that the
    // default trait methods delegate correctly.
    #[test]
    fn test_domain_error_implements_error_code() {
        let err = DomainError::invalid_state("order closed");

        // Methods coming from the `ErrorCode` trait.
        assert_eq!(err.kind(), ErrorKind::InvalidState);
        assert_eq!(err.code(), "INVALID_STATE");
        assert_eq!(err.http_status(), 422);
        assert!(!err.is_retryable());
    }

    // Verify the `From<ErrorKind>` conversion produces a simple error.
    #[test]
    fn test_from_error_kind() {
        let err: DomainError = ErrorKind::NotFound.into();
        assert_eq!(err.kind(), ErrorKind::NotFound);
    }

    // Verify that user-defined error types can implement `ErrorCode` and
    // automatically gain the default trait methods.
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

    // Verify the user-friendly default messages exposed by `ErrorKind`.
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

    // A `Repr::Simple` error should render the kind's default message.
    #[test]
    fn test_simple_error_display() {
        let err = DomainError::from_kind(ErrorKind::NotFound);
        assert_eq!(err.to_string(), "the requested resource was not found");

        let err = DomainError::from_kind(ErrorKind::Internal);
        assert_eq!(err.to_string(), "an internal error occurred");
    }

    // Verify the `matches` helper distinguishes between kind and code.
    #[test]
    fn test_matches() {
        let err = DomainError::not_found("user").with_code("USER_NOT_FOUND");
        assert!(err.matches(ErrorKind::NotFound, "USER_NOT_FOUND"));
        assert!(!err.matches(ErrorKind::NotFound, "NOT_FOUND"));
        assert!(!err.matches(ErrorKind::Internal, "USER_NOT_FOUND"));

        // When no custom code is set, the default kind code is used.
        let err = DomainError::invalid_value("bad input");
        assert!(err.matches(ErrorKind::InvalidValue, "INVALID_VALUE"));
    }

    // Verify that converting `anyhow::Error` keeps the full error chain in
    // the rendered message.
    #[test]
    fn test_from_anyhow_preserves_error_chain() {
        use std::io;

        let root = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let anyhow_err = anyhow::Error::new(root).context("failed to load config");
        let domain_err: DomainError = anyhow_err.into();

        let msg = domain_err.to_string();
        // `{:#}` should render the full chain.
        assert!(msg.contains("failed to load config"), "msg: {msg}");
        assert!(msg.contains("file not found"), "msg: {msg}");
        assert_eq!(domain_err.kind(), ErrorKind::Internal);
    }
}
