//! Diesel extension crate providing support for recursive CTEs.
//!
//! The [`RecursiveCTEExt::with_recursive`] and
//! [`RecursiveCTEExt::with_recursive_not_all`] methods build Diesel queries
//! representing `WITH RECURSIVE` blocks that can be executed like any other
//! query.

pub mod builders;
pub mod columns;
pub mod connection_ext;
pub mod cte;
pub mod macros;
#[cfg(test)]
pub(crate) mod test_support;

/// Bundles the CTE and body fragments handed to `with_cte`.
pub use builders::CteParts;
/// Bundles the seed, step, and body fragments handed to `with_recursive`.
pub use builders::RecursiveParts;
/// Builds a simple `WITH` block without the recursive union step.
pub use builders::with_cte;
#[doc = "Legacy helper kept for backwards compatibility with 0.1.0 previews."]
#[deprecated(note = "Use `RecursiveCTEExt::with_recursive` instead")]
pub use builders::with_recursive;
/// Builds a recursive `WITH RECURSIVE` block using `UNION`.
pub use builders::with_recursive_not_all;
/// Runtime column names paired with compile-time schema metadata.
pub use columns::Columns;
/// Extension trait exposing the `with_recursive` helper on Diesel connections.
pub use connection_ext::RecursiveCTEExt;
/// Marker trait implemented by Diesel backends that can run recursive CTEs.
pub use cte::RecursiveBackend;
/// The type of search style to use for recursive CTEs.
pub use cte::SearchStyle;
/// Wrapper for embedding Diesel fragments inside macro-driven queries.
pub use macros::QueryPart;
