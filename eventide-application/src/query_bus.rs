use crate::{context::AppContext, error::AppError};
use async_trait::async_trait;

/// Query Bus abstraction for the CQRS read side.
///
/// The query bus routes a strongly typed query `Q` to the
/// [`crate::query_handler::QueryHandler`] registered for that `(Q, R)` pair
/// and returns the produced result `R` to the caller. It is the symmetric
/// counterpart of [`crate::command_bus::CommandBus`] and lives on the read
/// path: handlers should consult read models / projections rather than
/// mutate aggregates.
///
/// # Implementation notes
///
/// - Implementations may target in-process dispatch (see
///   [`crate::InMemoryQueryBus`]) or cross-process query routing.
/// - The same query type may produce different result types — the
///   `(query type, result type)` pair forms the lookup key in the in-memory
///   implementation, allowing multiple read models to coexist.
/// - Implementations must be `Send + Sync`.
///
/// # Errors
///
/// Methods return [`AppError`]. Typical failure modes include
/// `HANDLER_NOT_FOUND` when nothing is registered for the requested
/// `(Q, R)` pair, `TYPE_MISMATCH` when the result returned by the handler
/// cannot be downcast to `R`, and any error surfaced by the handler itself.
#[async_trait]
pub trait QueryBus: Send + Sync {
    /// Dispatch a query and return its associated result.
    ///
    /// The bus locates the handler for the concrete `(Q, R)` pair, hands
    /// `q` to it, and forwards the produced `R` value to the caller.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] when no handler is registered for `(Q, R)`,
    /// the handler fails, or the produced value cannot be coerced back
    /// into the requested result type.
    async fn dispatch<Q, R>(&self, ctx: &AppContext, q: Q) -> Result<R, AppError>
    where
        Q: Send + 'static,
        R: Send + 'static;

    /// Dispatch a batch of queries in order, returning a parallel vector of
    /// results.
    ///
    /// The default implementation iterates sequentially and short-circuits
    /// on the first error. Implementations may override this to issue
    /// concurrent reads or coalesce identical queries.
    ///
    /// # Errors
    ///
    /// Returns the first [`AppError`] produced by any inner dispatch.
    async fn dispatch_batch<Q, R>(
        &self,
        ctx: &AppContext,
        queries: Vec<Q>,
    ) -> Result<Vec<R>, AppError>
    where
        Q: Send + 'static,
        R: Send + 'static,
    {
        let mut out = Vec::with_capacity(queries.len());
        for q in queries {
            out.push(self.dispatch::<Q, R>(ctx, q).await?);
        }
        Ok(out)
    }
}
