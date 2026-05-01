//! # Application-layer unified error type
//!
//! This module defines [`AppError`], the canonical error type returned by
//! command/query handlers and the surrounding application-layer plumbing.
//! It integrates seamlessly with the [`ErrorCode`] trait from
//! `eventide-domain` so that the same `code()` / `kind()` / `http_status()`
//! contract used in the domain layer can be re-exported all the way up to
//! the HTTP/gRPC boundary.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  eventide-domain                                                │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │
//! │  │ ErrorKind   │  │ ErrorCode   │  │ DomainError             │  │
//! │  │ (category)  │  │ (trait)     │  │ impl ErrorCode ✓        │  │
//! │  └─────────────┘  └─────────────┘  └─────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  eventide-application (this module)                             │
//! │  ┌───────────────────────────────────────────────────────────┐  │
//! │  │ AppError                                                  │  │
//! │  │ impl ErrorCode ✓                                          │  │
//! │  │ impl From<DomainError> ✓                                  │  │
//! │  └───────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Quick start
//!
//! ### Using AppError
//!
//! ```rust
//! use eventide_application::error::{AppError, AppResult};
//! use eventide_domain::error::DomainError;
//!
//! fn process_command() -> AppResult<String> {
//!     // Domain errors are converted into AppError automatically.
//!     let domain_result: Result<String, DomainError> =
//!         Err(DomainError::not_found("user 123"));
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
//! ### Mapping to an API response
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

use eventide_domain::error::{DomainError, ErrorCode, ErrorKind};
use std::error::Error as StdError;
use std::fmt;

/// Unified application-layer error type.
///
/// `AppError` is the single error type produced by application-layer
/// handlers. It can wrap a [`DomainError`] (with the domain's `kind` and
/// `code` preserved) or carry an application-specific failure such as
/// validation, authorization, or a handler routing problem.
///
/// # Features
///
/// - Implements [`ErrorCode`], so it can be turned directly into an HTTP /
///   API response without further mapping.
/// - Provides `From<DomainError>` so that `?` lifts domain errors
///   transparently.
/// - Offers application-specific constructors for common failure modes
///   (validation, authorization, handler routing, type coercion).
///
/// # Examples
///
/// ```rust
/// use eventide_application::error::AppError;
/// use eventide_domain::error::{ErrorCode, ErrorKind};
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
    /// Construct a new application error with the given kind, stable code
    /// string, and human-readable message.
    fn new(kind: ErrorKind, code: &'static str, message: impl Into<Box<str>>) -> Self {
        Self {
            kind,
            code,
            message: message.into(),
            source: None,
        }
    }

    // ==================== Convenience constructors ====================

    /// Build a validation error.
    ///
    /// Use this for application-layer input validation failures (shape,
    /// format, length) that occur before reaching domain logic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorCode;
    ///
    /// let err = AppError::validation("email format invalid");
    /// assert_eq!(err.code(), "VALIDATION_ERROR");
    /// assert_eq!(err.http_status(), 400);
    /// ```
    #[must_use]
    pub fn validation(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::InvalidValue, "VALIDATION_ERROR", msg)
    }

    /// Build an unauthorized error.
    ///
    /// Use this when the caller's identity cannot be verified or the
    /// presented credentials are insufficient.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorCode;
    ///
    /// let err = AppError::unauthorized("invalid token");
    /// assert_eq!(err.code(), "UNAUTHORIZED");
    /// assert_eq!(err.http_status(), 401);
    /// ```
    #[must_use]
    pub fn unauthorized(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Unauthorized, "UNAUTHORIZED", msg)
    }

    /// Build a "handler not found" error.
    ///
    /// Returned by buses (such as [`crate::InMemoryCommandBus`] /
    /// [`crate::InMemoryQueryBus`]) when no handler is registered for the
    /// dispatched type. The provided `handler_name` is included in the
    /// error message for diagnostics.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorCode;
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

    /// Build an "aggregate not found" error.
    ///
    /// Use this in command/query handlers that need to load an aggregate
    /// by id and find that the underlying repository returned no record.
    /// The aggregate type and id are interpolated into the message.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorCode;
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

    /// Build a "handler already registered" error.
    ///
    /// Returned by the in-memory buses when the caller attempts to register
    /// a second handler for a key that is already populated. Registration is
    /// intentionally exclusive to keep the dispatch deterministic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorCode;
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

    /// Build a "type mismatch" error.
    ///
    /// Used by the type-erased buses when a dynamically dispatched value
    /// cannot be downcast back to its expected concrete type. This usually
    /// indicates a registry corruption or a programming error rather than a
    /// runtime user-facing condition.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorCode;
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

    /// Build a generic internal error.
    ///
    /// Use this as a last resort when the failure cannot be expressed by a
    /// more specific constructor. Maps to HTTP 500.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorCode;
    ///
    /// let err = AppError::internal("unexpected state");
    /// assert_eq!(err.code(), "INTERNAL_ERROR");
    /// assert_eq!(err.http_status(), 500);
    /// ```
    #[must_use]
    pub fn internal(msg: impl Into<Box<str>>) -> Self {
        Self::new(ErrorKind::Internal, "INTERNAL_ERROR", msg)
    }

    // ==================== Inspection methods ====================

    /// Return the [`ErrorKind`] category of this error.
    #[must_use]
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return a reference to the wrapped [`DomainError`], if this `AppError`
    /// originated from a domain-layer failure via `From<DomainError>`.
    #[must_use]
    pub fn domain_error(&self) -> Option<&DomainError> {
        match &self.source {
            Some(Source::Domain(e)) => Some(e),
            _ => None,
        }
    }

    /// Return a reference to the inner source error, regardless of whether
    /// it originated as a [`DomainError`] or as an arbitrary error wrapped
    /// via [`AppError::wrap`].
    #[must_use]
    pub fn get_ref(&self) -> Option<&(dyn StdError + Send + Sync + 'static)> {
        match &self.source {
            Some(Source::Domain(e)) => Some(e),
            Some(Source::Other(e)) => Some(e.as_ref()),
            None => None,
        }
    }

    /// Attempt to downcast the inner source to a specific error type.
    ///
    /// Supports retrieval of the original concrete error from sources
    /// created with [`DomainError::custom`] or [`AppError::wrap`].
    #[must_use]
    pub fn downcast_ref<E: StdError + 'static>(&self) -> Option<&E> {
        match &self.source {
            Some(Source::Domain(e)) => e.downcast_ref(),
            Some(Source::Other(e)) => e.downcast_ref(),
            None => None,
        }
    }

    /// Wrap an arbitrary error into an `AppError`.
    ///
    /// The wrapped error's type information is preserved and can be
    /// recovered later with [`AppError::downcast_ref`]. The error's
    /// `Display` value is used as the human-readable message.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorKind;
    /// use std::io;
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

    /// Test whether this error matches the given kind/code pair.
    ///
    /// Useful in tests and conditional handling code that needs to react
    /// to a specific error variant without inspecting the message text.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use eventide_application::error::AppError;
    /// use eventide_domain::error::ErrorKind;
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

// ==================== Trait implementations ====================

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
        // Use static_code() so that any custom code attached to the
        // DomainError is propagated verbatim into the AppError.
        let code = e.static_code();
        Self {
            kind: e.kind(),
            code,
            message: e.to_string().into(),
            source: Some(Source::Domain(e)),
        }
    }
}

// ==================== Result alias ====================

/// Application-layer `Result` alias using [`AppError`] as the error type.
pub type AppResult<T> = Result<T, AppError>;

// ==================== Tests ====================

#[cfg(test)]
mod tests {
    use super::*;

    // Verify that the convenience constructors produce errors with the
    // expected kind/code/message tuple.
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

    // Verify that `From<DomainError>` preserves both the kind and the
    // ability to recover the original DomainError reference.
    #[test]
    fn test_from_domain_error() {
        let domain_err = DomainError::not_found("user 123");
        let app_err: AppError = domain_err.into();

        assert_eq!(app_err.kind(), ErrorKind::NotFound);
        assert!(app_err.domain_error().is_some());
    }

    // Verify that AppError implements ErrorCode and exposes the expected
    // HTTP status / retryability semantics.
    #[test]
    fn test_app_error_implements_error_code() {
        let err = AppError::validation("invalid input");

        assert_eq!(err.kind(), ErrorKind::InvalidValue);
        assert_eq!(err.code(), "VALIDATION_ERROR");
        assert_eq!(err.http_status(), 400);
        assert!(!err.is_retryable());
    }

    // Verify the `aggregate_not_found` convenience constructor formats the
    // message correctly and exposes the NotFound / 404 mapping.
    #[test]
    fn test_aggregate_not_found() {
        let err = AppError::aggregate_not_found("User", "user-123");
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert_eq!(err.code(), "AGGREGATE_NOT_FOUND");
        assert_eq!(err.http_status(), 404);
        assert!(err.to_string().contains("User"));
        assert!(err.to_string().contains("user-123"));
    }

    // Verify that `From<DomainError>` keeps a custom code attached via
    // `with_code`, instead of overriding it with a generic kind-based code.
    #[test]
    fn test_from_domain_error_preserves_custom_code() {
        let domain_err = DomainError::not_found("user 123").with_code("USER_NOT_FOUND");
        let app_err: AppError = domain_err.into();

        assert_eq!(app_err.kind(), ErrorKind::NotFound);
        assert_eq!(app_err.code(), "USER_NOT_FOUND"); // custom code is preserved
    }

    // Verify `wrap` preserves the underlying error type so it can be
    // recovered later through downcast_ref / get_ref.
    #[test]
    fn test_wrap_preserves_error() {
        use std::io;

        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err = AppError::wrap(ErrorKind::Internal, "IO_ERROR", io_err);

        assert_eq!(err.code(), "IO_ERROR");
        assert!(err.downcast_ref::<io::Error>().is_some());
        assert!(err.get_ref().is_some());
    }

    // Verify that errors smuggled through DomainError::custom into an
    // AppError can still be downcast back to their original concrete type.
    #[test]
    fn test_downcast_ref_through_domain_error() {
        use std::io;

        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let domain_err = DomainError::custom(ErrorKind::Internal, io_err);
        let app_err: AppError = domain_err.into();

        // Recover the original io::Error through the AppError surface.
        assert!(app_err.downcast_ref::<io::Error>().is_some());
    }

    // Verify the `matches` helper performs strict (kind, code) equality.
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
