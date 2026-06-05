//! Core types modelling CTE queries.
//!
//! [`WithRecursive`] represents recursive CTEs and [`WithCte`] represents
//! non-recursive CTEs, both as raw query fragments. [`RecursiveBackend`] marks
//! Diesel backends that support recursive queries.

use std::collections::BTreeSet;

use diesel::{
    backend::Backend,
    query_builder::{AstPass, Query, QueryFragment, QueryId},
    result::{Error, QueryResult},
};

use crate::columns::Columns;

macro_rules! impl_cte_traits {
    ($name:ident<$($gen:ident),*>, $body_ty:ident) => {
        impl<DB, Cols, $($gen),*> Query for $name<DB, Cols, $($gen),*>
        where
            DB: Backend,
            $body_ty: Query,
        {
            type SqlType = <$body_ty as Query>::SqlType;
        }

        impl<DB, Cols, $($gen),*, Conn> diesel::query_dsl::RunQueryDsl<Conn>
            for $name<DB, Cols, $($gen),*>
        where
            DB: Backend,
            Conn: diesel::connection::Connection<Backend = DB>,
            Self: QueryFragment<DB> + QueryId + Query,
        {}
    };
}

macro_rules! impl_static_query_id {
    ($name:ident<$($gen:ident),*>) => {
        impl<DB, Cols, $($gen),*> QueryId for $name<DB, Cols, $($gen),*>
        where
            DB: Backend + 'static,
            Cols: 'static,
            $($gen: 'static),*
        {
            type QueryId = Self;
            const HAS_STATIC_QUERY_ID: bool = true;
        }
    };
}

fn push_identifiers<DB, Cols>(
    out: &mut AstPass<'_, '_, DB>,
    cols: &Columns<Cols>,
) -> QueryResult<()>
where
    DB: Backend,
{
    let ids = cols.names;
    if ids.is_empty() {
        return Ok(());
    }
    ensure_unique_columns(ids)?;
    out.push_sql(" (");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            out.push_sql(", ");
        }
        out.push_identifier(id)?;
    }
    out.push_sql(")");
    Ok(())
}

fn ensure_unique_columns(names: &[&str]) -> QueryResult<()> {
    let mut seen = BTreeSet::new();
    for name in names {
        if !seen.insert(name) {
            return Err(Error::QueryBuilderError(
                format!("duplicate column name '{name}' in CTE").into(),
            ));
        }
    }
    Ok(())
}

/// Marker trait for backends that support `WITH RECURSIVE`.
pub trait RecursiveBackend: Backend {}

#[cfg(feature = "sqlite")]
impl RecursiveBackend for diesel::sqlite::Sqlite {}

#[cfg(feature = "postgres")]
impl RecursiveBackend for diesel::pg::Pg {}

/// Specifies how the seed and recursive step queries are combined in a
/// recursive CTE.
///
/// - `All` corresponds to `UNION ALL`, preserving duplicate rows between the
///   seed and step results.
/// - `Distinct` corresponds to `UNION`, which removes duplicate rows.
#[derive(Debug, Clone, Copy)]
pub enum UnionKind {
    /// Use `UNION ALL` between the seed and step queries.
    All,
    /// Use `UNION` between the seed and step queries (duplicates removed).
    Distinct,
}
impl UnionKind {
    /// Returns the SQL fragment used to combine the seed and recursive step of a
    /// recursive CTE.
    ///
    /// The returned string includes leading and trailing spaces so it can be
    /// concatenated directly into the generated SQL without adding extra
    /// separators.
    ///
    /// - `UnionKind::All` -> `" UNION ALL "` (preserves duplicate rows)
    /// - `UnionKind::Distinct` -> `" UNION "` (removes duplicate rows)
    const fn as_sql(self) -> &'static str {
        match self {
            Self::All => " UNION ALL ",
            Self::Distinct => " UNION ",
        }
    }
}

/// Define the search mode for the CTE
#[derive(Debug, Clone)]
pub struct SearchConfig {
    pub(crate) style: SearchStyle,
    pub(crate) search_column: &'static str,
    pub(crate) output_column: &'static str,
}

/// Define the search mode to tell the DB to use when scanning the recursive CTE.
#[derive(Debug, Clone, Copy)]
pub enum SearchStyle {
    /// Tells the DB to perform a breadth first scan of the recursive CTE
    BreadthFirst,
    /// Tells the DB to perform a depth first scan of the recursive CTE.
    DepthFirst,
}
impl std::fmt::Display for SearchStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BreadthFirst => write!(f, "BREADTH FIRST"),
            Self::DepthFirst => write!(f, "DEPTH FIRST"),
        }
    }
}

/// Representation of a recursive CTE query.
#[derive(Debug, Clone)]
pub struct WithRecursive<DB: Backend, Cols, Seed, Step, Body> {
    pub(crate) cte_name: &'static str,
    pub(crate) columns: Columns<Cols>,
    pub(crate) seed: Seed,
    pub(crate) step: Step,
    pub(crate) body: Body,
    pub(crate) union_kind: UnionKind,
    pub(crate) search_config: Option<SearchConfig>,
    pub(crate) _marker: std::marker::PhantomData<DB>,
}

impl<DB: Backend, Cols, Seed, Step, Body> WithRecursive<DB, Cols, Seed, Step, Body> {
    /// Specify the search mode for the recursive CTE.
    pub fn with_search(
        self,
        style: SearchStyle,
        search_column: &'static str,
        output_column: &'static str,
    ) -> Self {
        Self {
            search_config: Some(SearchConfig {
                style,
                search_column,
                output_column,
            }),
            ..self
        }
    }
}

impl<DB, Cols, Seed, Step, Body> QueryFragment<DB> for WithRecursive<DB, Cols, Seed, Step, Body>
where
    DB: Backend,
    Seed: QueryFragment<DB>,
    Step: QueryFragment<DB>,
    Body: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql("WITH RECURSIVE ");
        out.push_identifier(self.cte_name)?;
        push_identifiers(&mut out, &self.columns)?;
        out.push_sql(" AS (");
        self.seed.walk_ast(out.reborrow())?;
        out.push_sql(self.union_kind.as_sql());
        self.step.walk_ast(out.reborrow())?;
        out.push_sql(") ");
        if let Some(SearchConfig {
            style,
            search_column,
            output_column,
        }) = &self.search_config
        {
            out.push_sql("SEARCH ");
            out.push_sql(style.to_string().as_str());
            out.push_sql(" BY ");
            out.push_identifier(search_column)?;
            out.push_sql(" SET ");
            out.push_identifier(output_column)?;
            out.push_sql(" ");
        }
        self.body.walk_ast(out.reborrow())
    }
}

/// Representation of a non-recursive CTE query.
#[derive(Debug, Clone)]
pub struct WithCte<DB: Backend, Cols, Cte, Body> {
    pub(crate) cte_name: &'static str,
    pub(crate) columns: Columns<Cols>,
    pub(crate) cte: Cte,
    pub(crate) body: Body,
    pub(crate) _marker: std::marker::PhantomData<DB>,
}

impl<DB, Cols, Cte, Body> QueryFragment<DB> for WithCte<DB, Cols, Cte, Body>
where
    DB: Backend,
    Cte: QueryFragment<DB>,
    Body: QueryFragment<DB>,
{
    fn walk_ast<'b>(&'b self, mut out: AstPass<'_, 'b, DB>) -> QueryResult<()> {
        out.push_sql("WITH ");
        out.push_identifier(self.cte_name)?;
        push_identifiers(&mut out, &self.columns)?;
        out.push_sql(" AS (");
        self.cte.walk_ast(out.reborrow())?;
        out.push_sql(") ");
        self.body.walk_ast(out.reborrow())
    }
}

impl_static_query_id!(WithCte<Cte, Body>);

impl<DB, Cols, Seed, Step, Body> QueryId for WithRecursive<DB, Cols, Seed, Step, Body>
where
    DB: Backend + 'static,
    Cols: 'static,
    Seed: 'static,
    Step: 'static,
    Body: 'static,
{
    type QueryId = ();
    const HAS_STATIC_QUERY_ID: bool = false;
}

impl_cte_traits!(WithCte<Cte, Body>, Body);
impl_cte_traits!(WithRecursive<Seed, Step, Body>, Body);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        builders::{self, RecursiveParts},
        test_support::normalise_debug_sql,
    };
    use diesel::{
        debug_query, dsl::sql, expression::SqlLiteral, sql_types::Integer, sqlite::Sqlite,
    };
    use rstest::{fixture, rstest};

    enum Builder {
        All,
        Distinct,
    }

    #[fixture]
    fn sample_parts()
    -> RecursiveParts<SqlLiteral<Integer>, SqlLiteral<Integer>, SqlLiteral<Integer>> {
        RecursiveParts::new(
            sql::<Integer>("SELECT 1"),
            sql::<Integer>("SELECT n + 1 FROM nums WHERE n < 2"),
            sql::<Integer>("SELECT n FROM nums"),
        )
    }

    #[test]
    fn duplicate_column_names_are_rejected() {
        let names = &["id", "id"];
        match ensure_unique_columns(names) {
            Err(err) => {
                assert!(matches!(err, Error::QueryBuilderError(_)));
                assert!(err.to_string().contains("duplicate column name"));
            }
            Ok(()) => panic!("expected duplicate column error"),
        }
    }

    #[rstest]
    #[case::all(Builder::All, "UNION ALL")]
    #[case::distinct(Builder::Distinct, "UNION")]
    fn with_recursive_renders_expected_sql(
        sample_parts: RecursiveParts<SqlLiteral<Integer>, SqlLiteral<Integer>, SqlLiteral<Integer>>,
        #[case] builder: Builder,
        #[case] union_op: &str,
    ) {
        let query = match builder {
            Builder::All => {
                builders::with_recursive::<Sqlite, _, _, _, _, _>("nums", &["n"], sample_parts)
            }
            Builder::Distinct => builders::with_recursive_not_all::<Sqlite, _, _, _, _, _>(
                "nums",
                &["n"],
                sample_parts,
            ),
        };

        let sql = normalise_debug_sql(&debug_query::<Sqlite, _>(&query).to_string());
        assert_eq!(
            sql,
            format!(
                "WITH RECURSIVE \"nums\" (\"n\") AS (SELECT 1 {union_op} SELECT n + 1 FROM nums WHERE n < 2) SELECT n FROM nums"
            )
        );
    }

    #[test]
    fn with_cte_renders_expected_sql() {
        let query = builders::with_cte::<Sqlite, _, _, _, _>(
            "seed",
            &["value"],
            builders::CteParts::new(
                sql::<Integer>("SELECT 42"),
                sql::<Integer>("SELECT value FROM seed"),
            ),
        );
        let sql = normalise_debug_sql(&debug_query::<Sqlite, _>(&query).to_string());
        assert_eq!(
            sql,
            "WITH \"seed\" (\"value\") AS (SELECT 42) SELECT value FROM seed"
        );
    }

    #[test]
    fn with_recursive_skips_identifier_list_when_empty() {
        let query = builders::with_recursive::<Sqlite, _, _, _, _, _>(
            "nums",
            &[] as &[&str],
            RecursiveParts::new(
                sql::<Integer>("SELECT 1"),
                sql::<Integer>("SELECT n + 1 FROM nums WHERE n < 2"),
                sql::<Integer>("SELECT n FROM nums"),
            ),
        );
        let sql = normalise_debug_sql(&debug_query::<Sqlite, _>(&query).to_string());
        assert_eq!(
            sql,
            "WITH RECURSIVE \"nums\" AS (SELECT 1 UNION ALL SELECT n + 1 FROM nums WHERE n < 2) SELECT n FROM nums"
        );
    }

    #[test]
    fn query_id_reflects_runtime_union_choice() {
        type RecursiveQuery = WithRecursive<
            Sqlite,
            (),
            SqlLiteral<Integer>,
            SqlLiteral<Integer>,
            SqlLiteral<Integer>,
        >;
        type CteQuery = WithCte<Sqlite, (), SqlLiteral<Integer>, SqlLiteral<Integer>>;

        let recursive_has_static =
            std::hint::black_box(<RecursiveQuery as QueryId>::HAS_STATIC_QUERY_ID);
        let cte_has_static = std::hint::black_box(<CteQuery as QueryId>::HAS_STATIC_QUERY_ID);

        assert!(!recursive_has_static);
        assert!(cte_has_static);
    }
}
