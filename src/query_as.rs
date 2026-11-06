use std::marker::PhantomData;

use crate::{builder::parse_query, Platform};
use sqlx::{query::QueryAs, AssertSqlSafe, Database, Executor, IntoArguments};

/// Type alias for SQLx QueryAs with generic database arguments.
pub type QA<'q, R, DB, ARG> = QueryAs<'q, DB, R, ARG>;

/// A prepared query builder that returns typed results from named placeholders.
///
/// `PreparedQueryAs` is similar to `PreparedQuery` but returns strongly-typed results
/// using SQLx's `FromRow` trait. It supports `fetch_all`, `fetch_one`, and `fetch_optional`.
///
/// # Type Parameters
///
/// * `R` - The result type that implements `FromRow`
/// * `F` - A binder function that binds values to placeholders
///
/// # Examples
///
/// ```rust,no_run
/// use sqlx::{SqlitePool, FromRow};
/// use sqlx_named_bind::PreparedQueryAs;
///
/// #[derive(FromRow)]
/// struct User {
///     id: i32,
///     name: String,
/// }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let pool = SqlitePool::connect("sqlite::memory:").await?;
/// let user_id = 42;
///
/// let mut query = PreparedQueryAs::<User, _, sqlx::Sqlite, _, _>::new(
///     "SELECT id, name FROM users WHERE id = :id",
///     |q, key| match key {
///         ":id" => q.bind(user_id),
///         _ => q,
///     }
/// )?;
///
/// let user: User = query.fetch_one(&pool).await?;
/// println!("User: {} ({})", user.name, user.id);
/// # Ok(())
/// # }
/// ```
pub struct PreparedQueryAs<'q, R, F, DB, ARG, ROW>
where
    R: for<'r> sqlx::FromRow<'r, ROW> + Send + Unpin,
    F: FnMut(QA<'q, R, DB, ARG>, &str) -> QA<'q, R, DB, ARG>,
    DB: Database<Arguments = ARG>,
    ARG: IntoArguments<DB>,
    ROW: sqlx::Row<Database = DB>,
{
    sql: String,
    order: Vec<String>,
    binder: F,

    _pd: std::marker::PhantomData<R>,
    _pd_lifetime: std::marker::PhantomData<&'q ()>,
    _pd_db: std::marker::PhantomData<DB>,
    _pd_arg: std::marker::PhantomData<ARG>,
    _pd_row: std::marker::PhantomData<ROW>,
}

// Diagnostics:
// the trait bound `for<'r> R: sqlx::FromRow<'r, <DB as sqlx::Database>::Row>` is not satisfied
// the trait `for<'r> sqlx::FromRow<'r, <DB as sqlx::Database>::Row>` is not implemented for `R` [E0277]

// Diagnostics:
// the method `fetch_all` exists for struct `sqlx::query::QueryAs<'_, DB, R, ARG>`, but its trait bounds were not satisfied
// the following trait bounds were not satisfied:
// `R: sqlx::FromRow<'r, ROW>` [E0599]
impl<'q, R, F, DB, ARG, ROW> PreparedQueryAs<'q, R, F, DB, ARG, ROW>
where
    R: for<'r> sqlx::FromRow<'r, ROW> + Send + Unpin,
    F: FnMut(QA<'q, R, DB, ARG>, &str) -> QA<'q, R, DB, ARG>,
    DB: Database<Arguments = ARG, Row = ROW>,
    ARG: IntoArguments<DB>,
    ROW: sqlx::Row<Database = DB>,
{
    /// Creates a new `PreparedQueryAs` from an SQL template and binder function.
    ///
    /// # Arguments
    ///
    /// * `template` - SQL query template with named placeholders
    /// * `binder` - Function that binds values to placeholders
    ///
    /// # Errors
    ///
    /// Returns an error if the SQL template cannot be parsed.
    /// SQLx `Any` and unknown drivers require [`Self::new_with_platform`] instead.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use sqlx::FromRow;
    /// use sqlx_named_bind::PreparedQueryAs;
    ///
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i32,
    ///     name: String,
    /// }
    ///
    /// let query = PreparedQueryAs::<User, _, sqlx::Sqlite, _, _>::new(
    ///     "SELECT id, name FROM users WHERE id = :id",
    ///     |q, key| match key {
    ///         ":id" => q.bind(42),
    ///         _ => q,
    ///     }
    /// )?;
    /// # Ok::<(), sqlx_named_bind::Error>(())
    /// ```
    pub fn new<T>(template: T, binder: F) -> crate::Result<Self>
    where
        T: Into<String>,
    {
        Self::new_with_platform(template, binder, Platform::for_database::<DB>()?)
    }

    /// Creates a typed query using an explicit placeholder format.
    ///
    /// Use this with SQLx `Any`, choosing the platform of the connection or pool
    /// on which the query will execute.
    pub fn new_with_platform<T>(template: T, binder: F, platform: Platform) -> crate::Result<Self>
    where
        T: Into<String>,
    {
        let (sql, order) = parse_query(&template.into(), platform)?;

        Ok(Self {
            sql,
            order,
            binder,

            _pd: PhantomData,
            _pd_lifetime: PhantomData,
            _pd_db: PhantomData,
            _pd_arg: PhantomData,
            _pd_row: PhantomData,
        })
    }

    /// Executes the query and returns all matching rows.
    ///
    /// # Arguments
    ///
    /// * `executor` - Any SQLx executor (pool, transaction, etc.)
    ///
    /// # Returns
    ///
    /// Returns a vector of all rows matching the query.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails or if any row cannot be converted to type `R`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use sqlx::{SqlitePool, FromRow};
    /// use sqlx_named_bind::PreparedQueryAs;
    ///
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = SqlitePool::connect("sqlite::memory:").await?;
    /// let mut query = PreparedQueryAs::<User, _, sqlx::Sqlite, _, _>::new(
    ///     "SELECT id, name FROM users WHERE age > :min_age",
    ///     |q, key| match key {
    ///         ":min_age" => q.bind(18),
    ///         _ => q,
    ///     }
    /// )?;
    ///
    /// let users: Vec<User> = query.fetch_all(&pool).await?;
    /// println!("Found {} users", users.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fetch_all<'e, E>(&mut self, executor: E) -> crate::Result<Vec<R>>
    where
        E: Executor<'e, Database = DB>,
    {
        let Self {
            ref sql,
            ref order,
            ref mut binder,
            ..
        } = self;

        let mut q = sqlx::query_as(AssertSqlSafe(sql.as_str()));
        for key in order.iter() {
            q = binder(q, key);
        }
        Ok(q.fetch_all(executor).await?)
    }

    /// Executes the query and returns exactly one row.
    ///
    /// # Arguments
    ///
    /// * `executor` - Any SQLx executor (pool, transaction, etc.)
    ///
    /// # Returns
    ///
    /// Returns the single row matching the query.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No rows are found
    /// - More than one row is found
    /// - The query fails
    /// - The row cannot be converted to type `R`
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use sqlx::{SqlitePool, FromRow};
    /// use sqlx_named_bind::PreparedQueryAs;
    ///
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = SqlitePool::connect("sqlite::memory:").await?;
    /// let mut query = PreparedQueryAs::<User, _, sqlx::Sqlite, _, _>::new(
    ///     "SELECT id, name FROM users WHERE id = :id",
    ///     |q, key| match key {
    ///         ":id" => q.bind(42),
    ///         _ => q,
    ///     }
    /// )?;
    ///
    /// let user: User = query.fetch_one(&pool).await?;
    /// println!("Found user: {}", user.name);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fetch_one<'e, E>(&mut self, executor: E) -> crate::Result<R>
    where
        E: Executor<'e, Database = DB>,
    {
        let Self {
            ref sql,
            ref order,
            ref mut binder,
            ..
        } = self;

        let mut q = sqlx::query_as(AssertSqlSafe(sql.as_str()));
        for key in order.iter() {
            q = binder(q, key);
        }
        Ok(q.fetch_one(executor).await?)
    }

    /// Executes the query and returns at most one row.
    ///
    /// # Arguments
    ///
    /// * `executor` - Any SQLx executor (pool, transaction, etc.)
    ///
    /// # Returns
    ///
    /// Returns `Some(row)` if exactly one row matches, `None` if no rows match.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - More than one row is found
    /// - The query fails
    /// - The row cannot be converted to type `R`
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use sqlx::{SqlitePool, FromRow};
    /// use sqlx_named_bind::PreparedQueryAs;
    ///
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = SqlitePool::connect("sqlite::memory:").await?;
    /// let mut query = PreparedQueryAs::<User, _, sqlx::Sqlite, _, _>::new(
    ///     "SELECT id, name FROM users WHERE email = :email",
    ///     |q, key| match key {
    ///         ":email" => q.bind("user@example.com"),
    ///         _ => q,
    ///     }
    /// )?;
    ///
    /// match query.fetch_optional(&pool).await? {
    ///     Some(user) => println!("Found user: {}", user.name),
    ///     None => println!("User not found"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fetch_optional<'e, E>(&mut self, executor: E) -> crate::Result<Option<R>>
    where
        E: Executor<'e, Database = DB>,
    {
        let Self {
            ref sql,
            ref order,
            ref mut binder,
            ..
        } = self;

        let mut q = sqlx::query_as(AssertSqlSafe(sql.as_str()));
        for key in order.iter() {
            q = binder(q, key);
        }
        Ok(q.fetch_optional(executor).await?)
    }
}

#[cfg(all(
    test,
    any(
        feature = "sqlite",
        feature = "mysql",
        feature = "postgres",
        feature = "mssql"
    )
))]
mod platform_tests {
    use super::*;

    fn check_platform<DB: Database>(expected: &str)
    where
        DB::Arguments: IntoArguments<DB>,
        (i32,): for<'r> sqlx::FromRow<'r, DB::Row>,
    {
        let query =
            PreparedQueryAs::<(i32,), _, DB, _, _>::new("SELECT :id + :other + :id", |q, _| q)
                .unwrap();
        assert_eq!(query.sql, expected);
        assert_eq!(query.order, vec![":id", ":other", ":id"]);
    }

    #[test]
    fn test_automatic_platform_placeholders() {
        #[cfg(feature = "sqlite")]
        check_platform::<sqlx::Sqlite>("SELECT ? + ? + ?");
        #[cfg(feature = "mysql")]
        check_platform::<sqlx::MySql>("SELECT ? + ? + ?");
        #[cfg(feature = "postgres")]
        check_platform::<sqlx::Postgres>("SELECT $1 + $2 + $3");
        #[cfg(feature = "mssql")]
        check_platform::<sqlx_mssql_rs::Mssql>("SELECT ? + ? + ?");
    }

    #[cfg(feature = "postgres")]
    #[test]
    fn test_postgres_casts_preserve_binding_order() {
        let query =
            crate::PgPreparedQueryAs::<(i32,), _>::new("SELECT :id::int + :id::int", |q, key| {
                match key {
                    ":id" => q.bind(42),
                    _ => panic!("unexpected placeholder: {key}"),
                }
            })
            .unwrap();
        assert_eq!(query.sql, "SELECT $1::int + $2::int");
        assert_eq!(query.order, vec![":id", ":id"]);
    }
}

#[cfg(all(test, feature = "any"))]
mod any_tests {
    use crate::{AnyPreparedQueryAs, Error, Platform};

    #[test]
    fn test_any_requires_explicit_platform() {
        let query = AnyPreparedQueryAs::<(i32,), _>::new("SELECT :id", |q, _| q);
        assert!(matches!(query, Err(Error::PlatformRequired("Any"))));

        let query = AnyPreparedQueryAs::<(i32,), _>::new_with_platform(
            "SELECT :id + :id",
            |q, key| match key {
                ":id" => q.bind(42),
                _ => panic!("unexpected placeholder: {key}"),
            },
            Platform::Postgres,
        )
        .unwrap();
        assert_eq!(query.sql, "SELECT $1 + $2");
        assert_eq!(query.order, vec![":id", ":id"]);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn test_any_executes_with_explicit_platform() -> crate::Result<()> {
        use sqlx::{AssertSqlSafe, Connection};

        sqlx::any::install_default_drivers();
        let mut connection = sqlx::AnyConnection::connect("sqlite::memory:").await?;
        sqlx::query(AssertSqlSafe("CREATE TABLE numbers (value INTEGER)"))
            .execute(&mut connection)
            .await?;

        let mut insert = crate::AnyPreparedQuery::new_with_platform(
            "INSERT INTO numbers (value) VALUES (:value)",
            |q, key| match key {
                ":value" => q.bind(42),
                _ => panic!("unexpected placeholder: {key}"),
            },
            Platform::Sqlite,
        )?;
        assert_eq!(insert.execute(&mut connection).await?.rows_affected(), 1);

        let mut select = AnyPreparedQueryAs::<(i32,), _>::new_with_platform(
            "SELECT value FROM numbers WHERE value = :value OR value = :value",
            |q, key| match key {
                ":value" => q.bind(42),
                _ => panic!("unexpected placeholder: {key}"),
            },
            Platform::Sqlite,
        )?;
        assert_eq!(select.fetch_one(&mut connection).await?, (42,));
        assert_eq!(select.fetch_all(&mut connection).await?, vec![(42,)]);
        assert_eq!(select.fetch_optional(&mut connection).await?, Some((42,)));
        Ok(())
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use crate::SqlitePreparedQueryAs;
    use sqlx::{Connection, SqliteConnection};

    #[derive(Debug, PartialEq, sqlx::FromRow)]
    struct TestRow {
        id: i32,
        name: String,
    }

    #[test]
    fn test_prepared_query_as_placeholder_order() {
        let query = SqlitePreparedQueryAs::<TestRow, _>::new(
            "SELECT id, name FROM users WHERE id = :id AND name = :name OR id = :id",
            |q, _| q,
        )
        .unwrap();

        assert_eq!(query.order, vec![":id", ":name", ":id"]);
        assert_eq!(
            query.sql,
            "SELECT id, name FROM users WHERE id = ? AND name = ? OR id = ?"
        );
    }

    #[tokio::test]
    async fn test_fetch_methods_and_query_reuse() -> crate::Result<()> {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
        let mut query = SqlitePreparedQueryAs::<TestRow, _>::new(
            "SELECT :id AS id, :name AS name",
            |q, key| match key {
                ":id" => q.bind(42),
                ":name" => q.bind("Alice"),
                _ => panic!("unexpected placeholder: {key}"),
            },
        )?;
        let expected = TestRow {
            id: 42,
            name: "Alice".to_owned(),
        };

        assert_eq!(
            query.fetch_all(&mut connection).await?,
            vec![TestRow {
                id: 42,
                name: "Alice".to_owned(),
            }]
        );
        assert_eq!(query.fetch_one(&mut connection).await?, expected);
        assert_eq!(query.fetch_optional(&mut connection).await?, Some(expected));
        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_missing_rows() -> crate::Result<()> {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
        let mut query = SqlitePreparedQueryAs::<TestRow, _>::new(
            "SELECT 1 AS id, 'Alice' AS name WHERE 1 = 0",
            |q, _| q,
        )?;

        assert!(query.fetch_all(&mut connection).await?.is_empty());
        assert_eq!(query.fetch_optional(&mut connection).await?, None);
        assert!(matches!(
            query.fetch_one(&mut connection).await,
            Err(crate::Error::Database(sqlx::Error::RowNotFound))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_all_repeated_placeholders_in_transaction() -> crate::Result<()> {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
        let mut tx = connection.begin().await?;
        let mut query = SqlitePreparedQueryAs::<(i32,), _>::new(
            "SELECT :id UNION ALL SELECT :id",
            |q, key| match key {
                ":id" => q.bind(42),
                _ => panic!("unexpected placeholder: {key}"),
            },
        )?;

        assert_eq!(query.fetch_all(&mut *tx).await?, vec![(42,), (42,)]);
        tx.commit().await?;
        Ok(())
    }
}
