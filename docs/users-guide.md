# User guide

This guide explains how to compose Common Table Expressions (CTEs) with
`diesel-cte-ext`, configure features, and exercise the crate inside automated
suites. Follow the documentation style guide in
`docs/documentation-style-guide.md` when extending this file.

## Overview

`diesel-cte-ext` adds two ergonomic layers on top of Diesel:

- the `RecursiveCTEExt` trait, which exposes `with_cte` and `with_recursive`
  constructors on synchronous and async connection types, plus
  `with_recursive_not_all` for recursive `UNION` behaviour;
- the `Columns` utilities that keep runtime column names aligned with Diesel's
  compile-time type metadata.

The crate works with SQLite and PostgreSQL backends out of the box. Enable the
`async` feature when you need `diesel_async` connections.

## Feature flags

| Feature    | Purpose                                          |
| ---------- | ------------------------------------------------ |
| `sqlite`   | Enables Diesel's SQLite backend integration.     |
| `postgres` | Enables Diesel's PostgreSQL backend integration. |
| `async`    | Adds `diesel_async` support for both backends.   |

All examples in this document assume the default feature set (`sqlite` +
`postgres`). Enable `async` when compiling the async snippets or running the
integration tests.

## Building non-recursive CTEs

Use `with_cte` to create a single `WITH` block without a recursive step. Bundle
the CTE body and the consuming query using `CteParts::new` before passing them
to the helper.

```rust,no_run
use diesel::{dsl::sql, sqlite::SqliteConnection, sql_types::Text, RunQueryDsl};
use diesel_cte_ext::{CteParts, RecursiveCTEExt};

fn names() -> diesel::QueryResult<Vec<String>> {
    let mut conn = SqliteConnection::establish(":memory:")?;
    conn.with_cte(
        "names",
        &["label"],
        CteParts::new(
            sql::<Text>("SELECT 'root' AS label UNION ALL SELECT 'child'"),
            sql::<Text>("SELECT label FROM names ORDER BY label"),
        ),
    )
    .load(&mut conn)
}
```

## Building recursive CTEs

Recursive queries delegate the three constituent fragments (seed, recursive
step, and final body) to a `RecursiveParts` struct. The builder enforces the
shape of each fragment, so Diesel can validate the AST at compile time.

```rust,no_run
use diesel::{dsl::sql, pg::PgConnection, sql_types::Integer, RunQueryDsl};
use diesel_cte_ext::{RecursiveCTEExt, RecursiveParts};

fn up_to_five(conn: &mut PgConnection) -> diesel::QueryResult<Vec<i32>> {
    conn.with_recursive(
        "series",
        &["n"],
        RecursiveParts::new(
            sql::<Integer>("SELECT 1"),
            sql::<Integer>("SELECT n + 1 FROM series WHERE n < 5"),
            sql::<Integer>("SELECT n FROM series"),
        ),
    )
    .load(conn)
}
```

`with_recursive` renders a recursive `UNION ALL`. Use `with_recursive_not_all`
when the recursive term should deduplicate at each iteration using `UNION`.

```rust,no_run
use diesel::{dsl::sql, pg::PgConnection, sql_types::Integer, RunQueryDsl};
use diesel_cte_ext::{RecursiveCTEExt, RecursiveParts};

fn reachable_nodes(conn: &mut PgConnection) -> diesel::QueryResult<Vec<i32>> {
    conn.with_recursive_not_all(
        "graph",
        &["node_id"],
        RecursiveParts::new(
            sql::<Integer>("SELECT 1"),
            sql::<Integer>(concat!(
                "SELECT edges.target_id FROM edges ",
                "INNER JOIN graph ON edges.source_id = graph.node_id"
            )),
            sql::<Integer>("SELECT node_id FROM graph ORDER BY node_id"),
        ),
    )
    .load(conn)
}
```

Async connections receive the same helpers once the `async` feature is enabled:

```rust,no_run
use diesel::{dsl::sql, sql_types::Integer};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use diesel_cte_ext::{RecursiveCTEExt, RecursiveParts};

async fn up_to_five_async() -> diesel::QueryResult<Vec<i32>> {
    let mut conn = AsyncPgConnection::establish("postgresql://localhost/postgres").await?;
    conn.with_recursive(
        "series",
        &["n"],
        RecursiveParts::new(
            sql::<Integer>("SELECT 1"),
            sql::<Integer>("SELECT n + 1 FROM series WHERE n < 5"),
            sql::<Integer>("SELECT n FROM series"),
        ),
    )
    .load(&mut conn)
    .await
}
```

## Using diesel DSL builders instead of raw SQL

The full diesel DSL can be used to build the CTE instead of raw SQL queries.

```rust,no_run
use diesel::prelude::*;
use diesel_cte_ext::{RecursiveCTEExt, RecursiveParts};

table! {
    categories (id) {
        id -> Integer,
        parent_id -> Nullable<Integer>,
    }
}

fn sample_recursive(conn: &mut PgConnection, cat_id: i32) -> diesel::QueryResult<Vec<i32>> {
    table! {
        parents (id) {
            id -> Nullable<Integer>,
        }
    }
    allow_tables_to_appear_in_same_query!(categories, parents);

    conn.with_recursive(
        "parents",
        &["id"],
        RecursiveParts::new(
            // seed query
            categories::table.select(categories::parent_id)
                .filter(categories::id.eq(cat_id)),
            // recursive step
            categories::table.select(categories::parent_id)
                .inner_join(
                    parents::table.on(parents::id.assume_not_null().eq(categories::id))
                ),
            // final body
            parents::table.select(parents::id)
        ),
    )
        .load(conn)
}
```

## Column helpers

Manual column lists are easy to mistype, especially when a recursive step spans
multiple tables. The `columns!` macro accepts individual column paths and emits
both the runtime names and the tuple of Diesel column types. Use
`table_columns!` to refer to a Diesel table definition and capture every column
in declaration order.

```rust
use diesel::{prelude::*, sql_types::Integer};
use diesel_cte_ext::{columns, table_columns, Columns};

diesel::table! {
    employees (id) {
        id -> Integer,
        manager_id -> Integer,
    }
}

const MANAGER_COLUMNS: Columns<(employees::id, employees::manager_id)> =
    columns!(employees::id, employees::manager_id);
const FULL_TABLE: Columns<employees::table> = table_columns!(employees::table);
```

## Macro helpers for inline fragments

Use `cte_query!`, `seed_query!`, and `step_query!` to wrap ad-hoc Diesel
expressions before passing them into `RecursiveParts::new`. The macros keep the
fragments strongly typed whilst avoiding manual `QueryPart` construction and
make the exported helpers more visible when developers scan the module surface.

```rust,no_run
use diesel::{dsl::sql, sql_types::Integer};
use diesel_cte_ext::{RecursiveParts, cte_query, seed_query, step_query};

let parts = RecursiveParts::new(
    seed_query!(sql::<Integer>("SELECT 1")),
    step_query!(sql::<Integer>("SELECT n + 1 FROM series")),
    cte_query!(sql::<Integer>("SELECT n FROM series")),
);
```

## Testing with `pg_embedded_setup_unpriv`

The integration tests under `tests/` rely on
`pg_embedded_setup_unpriv::TestCluster` to provision PostgreSQL without manual
privileges. Running `make test` triggers the helper to:

1. Stage PostgreSQL binaries and a writable data directory under the current
   user's home.
2. Launch the server before each test module executes.
3. Configure `PGPASSFILE`, `TZ`, and related environment variables so Diesel and
   libpq clients authenticate automatically.
4. Shut the cluster down once the `TestCluster` guard drops, preventing leaked
   `postmaster` processes between tests.

The `tests/postgres_recursive.rs` module demonstrates how to wrap the guard in
an `rstest` fixture and propagate failures through the test signature instead
of calling `unwrap`. This pattern should be reused when authoring new tests so
the harness can skip gracefully on machines that cannot start PostgreSQL (for
example, when `tzdata` is missing).
