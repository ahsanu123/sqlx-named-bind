# sqlx-named-bind

[![Crates.io](https://img.shields.io/crates/v/sqlx-named-bind.svg)](https://crates.io/crates/sqlx-named-bind)
[![Documentation](https://docs.rs/sqlx-named-bind/badge.svg)](https://docs.rs/sqlx-named-bind)
[![License](https://img.shields.io/crates/l/sqlx-named-bind.svg)](https://github.com/ahsanu123/sqlx-named-bind#license)

A SQLx extension that provides named parameter binding with HRTB (Higher-Rank Trait Bounds) pattern, avoiding self-referential lifetime issues.

## Features

- ✨ **Named Placeholders**: Use `:param_name` instead of `?` in your SQL queries
- 🔒 **Type-Safe**: Full compile-time type checking with SQLx
- 🚀 **Zero Overhead**: Placeholder conversion happens at construction time
- 🔄 **Generic Executor**: `Transaction`, and any SQLx `Executor`
- 📦 **Strongly-Typed Results**: `PreparedQueryAs` provides type-safe query results via `FromRow`
- 🛡️ **Lifetime Safe**: HRTB pattern avoids self-referential lifetime issues

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
sqlx = { version = "0.9", default-features = false, features = ["sqlite", "runtime-tokio", "macros"] }
sqlx-named-bind = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

SQLite is enabled by default. Each platform feature exposes database-specific type
aliases in the crate root and `prelude`. Typed backends automatically select the
appropriate placeholder format using the `Platform` enum.

| Feature | Query alias | Typed query alias |
|---------|-------------|-------------------|
| `sqlite` (default) | `SqlitePreparedQuery<F>` | `SqlitePreparedQueryAs<'q, R, F>` |
| `mysql` | `MySqlPreparedQuery<F>` | `MySqlPreparedQueryAs<'q, R, F>` |
| `postgres` | `PgPreparedQuery<F>` | `PgPreparedQueryAs<'q, R, F>` |
| `any` | `AnyPreparedQuery<F>` | `AnyPreparedQueryAs<'q, R, F>` |
| `mssql` | `MssqlPreparedQuery<F>` | `MssqlPreparedQueryAs<'q, R, F>` |

For example, select MySQL instead of the default SQLite backend:

```toml
[dependencies]
sqlx = { version = "0.9", default-features = false, features = ["mysql", "runtime-tokio"] }
sqlx-named-bind = { version = "0.1", default-features = false, features = ["mysql"] }
```

Enable `mssql` to use the optional
[`sqlx-mssql-rs`](https://crates.io/crates/sqlx-mssql-rs) driver:

```toml
[dependencies]
sqlx-named-bind = { version = "0.1", default-features = false, features = ["mssql"] }
sqlx-mssql-rs = { version = "0.1.1", default-features = false }
```

## Platform Placeholders

| Platform | Bound placeholders |
|----------|--------------------|
| `Platform::Sqlite` | `?`, `?`, ... |
| `Platform::MySql` | `?`, `?`, ... |
| `Platform::Postgres` | `$1`, `$2`, ... |
| `Platform::Mssql` | `?`, `?`, ... (rewritten to `@P1`, `@P2`, ... by `sqlx-mssql-rs`) |

`new(template, binder)` selects the platform from the SQLx database type, not
from which Cargo features happen to be enabled. Repeated names are bound once
per occurrence, so PostgreSQL's `:id + :id` becomes `$1 + $2`.
PostgreSQL casts such as `:id::int` are preserved as `$1::int`.

SQLx `Any` does not identify a concrete backend at construction time. Use
`new_with_platform(template, binder, platform)` with the platform of the target
connection or pool; `new` returns `Error::PlatformRequired` for `Any`:

```rust
use sqlx_named_bind::{AnyPreparedQueryAs, Platform};

let query = AnyPreparedQueryAs::<(i32,), _>::new_with_platform(
    "SELECT :id + :id",
    |q, key| match key {
        ":id" => q.bind(42),
        _ => q,
    },
    Platform::Postgres,
)?;
```

Enable the `any` feature and the concrete SQLx backend, and register drivers with
`sqlx::any::install_default_drivers()` for built-in drivers before connecting.
The low-level `builder::build_query` keeps its existing `?` output;
`builder::build_query_for_platform(template, platform)` selects a format explicitly.

## Quick Start

```rust
use sqlx::{sqlite::SqlitePoolOptions, AssertSqlSafe};
use sqlx_named_bind::SqlitePreparedQuery;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    sqlx::query(AssertSqlSafe(
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)"
    ))
    .execute(&pool)
    .await?;

    let user_id = 42;
    let name = "John Doe";

    let mut query = SqlitePreparedQuery::new(
        "INSERT INTO users (id, name) VALUES (:id, :name)",
        |q, key| match key {
            ":id" => q.bind(user_id),
            ":name" => q.bind(name),
            _ => q,
        }
    )?;

    let result = query.execute(&pool).await?;
    println!("Inserted {} rows", result.rows_affected());

    Ok(())
}
```

## Examples

### Typed Query Results

```rust
use sqlx::{SqlitePool, FromRow};
use sqlx_named_bind::SqlitePreparedQueryAs;

#[derive(FromRow)]
struct User {
    id: i32,
    name: String,
    email: String,
}

async fn find_users(pool: &SqlitePool, min_age: i32) -> Result<Vec<User>, Box<dyn std::error::Error>> {
    let mut query = SqlitePreparedQueryAs::<User, _>::new(
        "SELECT id, name, email FROM users WHERE age >= :min_age",
        |q, key| match key {
            ":min_age" => q.bind(min_age),
            _ => q,
        }
    )?;

    Ok(query.fetch_all(pool).await?)
}
```

### Using with Transactions

```rust
use sqlx::{SqlitePool, Transaction, Sqlite};
use sqlx_named_bind::SqlitePreparedQuery;

async fn transfer_money(
    pool: &SqlitePool,
    from_id: i32,
    to_id: i32,
    amount: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tx: Transaction<Sqlite> = pool.begin().await?;

    let mut debit = SqlitePreparedQuery::new(
        "UPDATE accounts SET balance = balance - :amount WHERE id = :id",
        |q, key| match key {
            ":amount" => q.bind(amount),
            ":id" => q.bind(from_id),
            _ => q,
        }
    )?;

    let mut credit = SqlitePreparedQuery::new(
        "UPDATE accounts SET balance = balance + :amount WHERE id = :id",
        |q, key| match key {
            ":amount" => q.bind(amount),
            ":id" => q.bind(to_id),
            _ => q,
        }
    )?;

    debit.execute(&mut *tx).await?;
    credit.execute(&mut *tx).await?;

    tx.commit().await?;
    Ok(())
}
```

### Optional Results

```rust
use sqlx::{SqlitePool, FromRow};
use sqlx_named_bind::SqlitePreparedQueryAs;

#[derive(FromRow)]
struct User {
    id: i32,
    name: String,
}

async fn find_user_by_email(
    pool: &SqlitePool,
    email: &str,
) -> Result<Option<User>, Box<dyn std::error::Error>> {
    let mut query = SqlitePreparedQueryAs::<User, _>::new(
        "SELECT id, name FROM users WHERE email = :email",
        |q, key| match key {
            ":email" => q.bind(email),
            _ => q,
        }
    )?;

    Ok(query.fetch_optional(pool).await?)
}
```

## How It Works

The library uses a three-step approach to avoid self-referential lifetime issues:

1. **Parse**: Extract named placeholders (`:name`) and convert them to the backend's positional placeholders
2. **Store**: Keep the converted SQL, placeholder order, and binder function separately
3. **Execute**: Construct a fresh SQLx `Query` on each execution with the correct lifetime

This approach leverages HRTB (Higher-Rank Trait Bounds) to ensure the binder function works with any lifetime, making the API both safe and flexible.

### Why HRTB?

Without HRTB, you'd encounter self-referential lifetime issues:

```rust
// ❌ This doesn't work - self-referential lifetime
struct BadQuery<'a> {
    query: Query<'a, Sqlite>,  // 'a refers to data inside BadQuery
}
```

With HRTB, we defer the lifetime decision to call-site:

```rust
// ✅ This works - lifetime chosen at each call
where F: for<'q> FnMut(Query<'q, Sqlite>, &str) -> Query<'q, Sqlite>
```

## API Documentation

### Generic types and platform aliases

The original generic types remain available:

```rust
use sqlx_named_bind::{PreparedQuery, PreparedQueryAs};

// Specify the backend; SQLx argument and row types can be inferred.
let query = PreparedQuery::<_, sqlx::Sqlite, _>::new(
    "DELETE FROM users WHERE id = :id",
    |q, key| match key {
        ":id" => q.bind(42),
        _ => q,
    },
)?;

let typed = PreparedQueryAs::<(i32,), _, sqlx::Sqlite, _, _>::new(
    "SELECT :id",
    |q, key| match key {
        ":id" => q.bind(42),
        _ => q,
    },
)?;
```

`PreparedQuery<F, DB, ARG>` and `PreparedQueryAs<'q, R, F, DB, ARG, ROW>` expose all
generic parameters. Platform aliases fix `DB`, `ARG`, and `ROW`, leaving only the
binder type `F` (usually `_`) and result type `R` for typed queries.

### `SqlitePreparedQuery<F>`

For queries that execute but don't return rows (`INSERT`, `UPDATE`, `DELETE`).

**Methods:**
- `new(template, binder)` - Create a new prepared query
- `new_with_platform(template, binder, platform)` - Explicitly select a placeholder format (required for `Any`)
- `execute(executor)` - Execute the query and return `SqliteQueryResult`

### `SqlitePreparedQueryAs<'q, R, F>`

For queries that return typed rows (`SELECT`).

**Methods:**
- `new(template, binder)` - Create a new prepared query
- `new_with_platform(template, binder, platform)` - Explicitly select a placeholder format (required for `Any`)
- `fetch_all(executor)` - Fetch all matching rows
- `fetch_one(executor)` - Fetch the first row (error if no rows match)
- `fetch_optional(executor)` - Fetch the first row if present (returns `Option<R>`)

## Limitations

- SQLx `Any` requires an explicit platform matching its target connection or pool
- Placeholder names must match `[a-zA-Z0-9_]+`
- All placeholders in the SQL must be handled by the binder function

## Comparison with Alternatives

| Feature | sqlx-named-bind | SQLx native | Other libraries |
|---------|----------------|-------------|-----------------|
| Named parameters | ✅ `:name` | ❌ `?` only | ✅ Varies |
| Type safety | ✅ Full | ✅ Full | ⚠️ Varies |
| Lifetime safety | ✅ HRTB | ✅ Native | ⚠️ Varies |
| Generic executor | ✅ Yes | ✅ Yes | ❌ Usually pool-only |
| Runtime overhead | ✅ Zero | ✅ Zero | ⚠️ Some have overhead |

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Acknowledgments

This library was inspired by the need for named parameter binding in SQLx while maintaining the same level of type safety and performance. Special thanks to the SQLx team for creating an excellent async SQL toolkit.
