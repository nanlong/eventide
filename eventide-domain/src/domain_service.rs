//! Domain services.
//!
//! A domain service hosts business logic that does not naturally belong on
//! any single aggregate — typically because it coordinates several
//! aggregates or depends on an external collaborator. The trait is defined
//! purely in terms of input, output, and error types so concrete services
//! can run asynchronously and remain testable behind a trait object.
//!
use async_trait::async_trait;

/// Domain-service contract for cross-aggregate or externally collaborating
/// business logic.
///
/// Implementors describe a single operation through three associated types:
///
/// - [`DomainService::Input`] — the data needed to perform the operation.
/// - [`DomainService::Output`] — the value produced on success.
/// - [`DomainService::Error`] — the error returned on failure.
///
/// Services are `Send + Sync` so they can be stored behind `Arc` and shared
/// across handlers in an asynchronous runtime.
#[async_trait]
pub trait DomainService: Send + Sync {
    type Input;
    type Output;
    type Error;

    async fn execute(&self, input: Self::Input) -> Result<Self::Output, Self::Error>;
}
