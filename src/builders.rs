//! Helper types for constructing recursive CTE queries.
//!
//! [`with_recursive`] builds a [`WithRecursive`] query from a name, column list
//! and the [`RecursiveParts`] struct bundling the seed, step and body fragments.
//! These helpers are used indirectly via
//! [`crate::connection_ext::RecursiveCTEExt::with_recursive`].

use diesel::{backend::Backend, query_builder::QueryFragment};

use crate::{
    columns::Columns,
    cte::{RecursiveBackend, UnionKind, WithCte, WithRecursive},
};

macro_rules! impl_recursive_builder {
    ($fn_name:ident, $union_kind:expr, $doc:expr) => {
        #[doc = $doc]
        pub fn $fn_name<DB, Cols, Seed, Step, Body, ColSpec>(
            cte_name: &'static str,
            columns: ColSpec,
            parts: RecursiveParts<Seed, Step, Body>,
        ) -> WithRecursive<DB, Cols, Seed, Step, Body>
        where
            DB: RecursiveBackend,
            Seed: QueryFragment<DB>,
            Step: QueryFragment<DB>,
            Body: QueryFragment<DB>,
            ColSpec: Into<Columns<Cols>>,
        {
            WithRecursive {
                cte_name,
                columns: columns.into(),
                seed: parts.seed,
                step: parts.step,
                body: parts.body,
                union_kind: $union_kind,
                search_config: None,
                _marker: std::marker::PhantomData,
            }
        }
    };
}

/// Query fragments used by a recursive CTE.
#[derive(Debug, Clone)]
pub struct RecursiveParts<Seed, Step, Body> {
    /// Seed query producing the first row(s) of the CTE.
    pub seed: Seed,
    /// Step query referencing the previous iteration's result.
    pub step: Step,
    /// Query consuming the CTE.
    pub body: Body,
}

impl<Seed, Step, Body> RecursiveParts<Seed, Step, Body> {
    /// Bundle the seed, step and body queries together.
    pub const fn new(seed: Seed, step: Step, body: Body) -> Self {
        Self { seed, step, body }
    }
}

/// Query fragments used by a non-recursive CTE.
#[derive(Debug, Clone)]
pub struct CteParts<Cte, Body> {
    /// Query producing the CTE rows.
    pub cte: Cte,
    /// Query consuming the CTE.
    pub body: Body,
}

impl<Cte, Body> CteParts<Cte, Body> {
    /// Bundle the CTE and body queries together.
    pub const fn new(cte: Cte, body: Body) -> Self {
        Self { cte, body }
    }
}

impl_recursive_builder!(
    with_recursive,
    UnionKind::All,
    "Build a recursive CTE query using `WITH RECURSIVE` and `UNION ALL`."
);

impl_recursive_builder!(
    with_recursive_not_all,
    UnionKind::Distinct,
    "Build a recursive CTE query using `WITH RECURSIVE` and `UNION` (not `ALL`)."
);

/// Build a non-recursive CTE query.
pub fn with_cte<DB, Cols, Cte, Body, ColSpec>(
    cte_name: &'static str,
    columns: ColSpec,
    parts: CteParts<Cte, Body>,
) -> WithCte<DB, Cols, Cte, Body>
where
    DB: Backend,
    Cte: QueryFragment<DB>,
    Body: QueryFragment<DB>,
    ColSpec: Into<Columns<Cols>>,
{
    WithCte {
        cte_name,
        columns: columns.into(),
        cte: parts.cte,
        body: parts.body,
        _marker: std::marker::PhantomData,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::normalise_debug_sql;
    use diesel::{
        debug_query, dsl::sql, expression::SqlLiteral, sql_types::Integer, sqlite::Sqlite,
    };
    use rstest::{fixture, rstest};

    type TestRecursiveParts =
        RecursiveParts<SqlLiteral<Integer>, SqlLiteral<Integer>, SqlLiteral<Integer>>;

    enum Builder {
        All,
        Distinct,
    }

    #[fixture]
    fn recursive_parts() -> TestRecursiveParts {
        RecursiveParts::new(
            sql::<Integer>("SELECT 1"),
            sql::<Integer>("SELECT n + 1 FROM nums"),
            sql::<Integer>("SELECT n FROM nums"),
        )
    }

    #[rstest]
    #[case::all(Builder::All, "UNION ALL")]
    #[case::distinct(Builder::Distinct, "UNION")]
    fn recursive_builder_composes_fragments(
        recursive_parts: TestRecursiveParts,
        #[case] builder: Builder,
        #[case] union_op: &str,
    ) {
        let query = match builder {
            Builder::All => {
                with_recursive::<Sqlite, _, _, _, _, _>("nums", &["n"], recursive_parts)
            }
            Builder::Distinct => {
                with_recursive_not_all::<Sqlite, _, _, _, _, _>("nums", &["n"], recursive_parts)
            }
        };
        let sql = normalise_debug_sql(&debug_query::<Sqlite, _>(&query).to_string());
        assert_eq!(
            sql,
            format!(
                "WITH RECURSIVE \"nums\" (\"n\") AS (SELECT 1 {union_op} SELECT n + 1 FROM nums) SELECT n FROM nums"
            )
        );
    }

    #[test]
    fn non_recursive_builder_composes_fragments() {
        let query = with_cte::<Sqlite, _, _, _, _>(
            "nums",
            &["n"],
            CteParts::new(
                sql::<Integer>("SELECT 1"),
                sql::<Integer>("SELECT n FROM nums"),
            ),
        );
        let sql = normalise_debug_sql(&debug_query::<Sqlite, _>(&query).to_string());
        assert_eq!(
            sql,
            "WITH \"nums\" (\"n\") AS (SELECT 1) SELECT n FROM nums"
        );
    }
}
