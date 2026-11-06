use crate::builder::build_query;
use regex::Regex;
use sqlx::query::Query;
use sqlx::Executor;
use sqlx::{AssertSqlSafe, Database, IntoArguments};
use std::marker::PhantomData;

/// Type alias for SQLx Query with generic database arguments.
pub type Q<'q, DB, ARGS> = Query<'q, DB, ARGS>;

/// A prepared query builder that supports named placeholders.
///
/// `PreparedQuery` allows you to use named placeholders (`:name`) in your SQL templates
/// instead of positional placeholders (`?`). It avoids self-referential lifetime issues
/// by storing the SQL template, placeholder order, and binder function separately,
/// and constructing the actual `Query` on each execution.
///
/// # Type Parameters
///
/// * `F` - A binder function that binds values to placeholders. Must work with any lifetime `'q`.
///
/// # Examples
///
/// ```rust,no_run
/// use sqlx::SqlitePool;
/// use sqlx_named_bind::PreparedQuery;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let pool = SqlitePool::connect("sqlite::memory:").await?;
/// let user_id = 42;
/// let name = "John Doe";
///
/// let mut query = PreparedQuery::new(
///     "INSERT INTO users (user_id, name) VALUES (:user_id, :name)",
///     |q, key| match key {
///         ":user_id" => q.bind(user_id),
///         ":name" => q.bind(name),
///         _ => q,
///     }
/// )?;
///
/// let result = query.execute(&pool).await?;
/// println!("Inserted {} rows", result.rows_affected());
/// # Ok(())
/// # }
/// ```
///
/// # Using with Transactions
///
/// ```rust,no_run
/// use sqlx::{SqlitePool, Transaction, Sqlite};
/// use sqlx_named_bind::PreparedQuery;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let pool = SqlitePool::connect("sqlite::memory:").await?;
/// let mut tx: Transaction<Sqlite> = pool.begin().await?;
///
/// let mut query = PreparedQuery::new(
///     "UPDATE users SET name = :name WHERE user_id = :user_id",
///     |q, key| match key {
///         ":user_id" => q.bind(vec![1, 2, 3]),
///         ":name" => q.bind("Jane Doe"),
///         _ => q,
///     }
/// )?;
///
/// query.execute(&mut *tx).await?;
/// tx.commit().await?;
/// # Ok(())
/// # }
/// ```
pub struct PreparedQuery<F, DB, ARG> {
    sql: String,
    order: Vec<String>,
    binder: F,

    db_ty: PhantomData<DB>,
    db_arg: PhantomData<ARG>,
}

impl<'q, F, DB, ARG> PreparedQuery<F, DB, ARG>
where
    F: FnMut(Q<'q, DB, ARG>, &str) -> Q<'q, DB, ARG>,
    DB: Database<Arguments = ARG>,
    ARG: IntoArguments<DB>,
{
    /// Creates a new `PreparedQuery` from an SQL template and binder function.
    ///
    /// The SQL template can contain named placeholders in the format `:name`.
    /// The binder function will be called for each placeholder in the order they appear.
    ///
    /// # Arguments
    ///
    /// * `template` - SQL query template with named placeholders (e.g., `:user_id`)
    /// * `binder` - Function that binds values to placeholders based on their names
    ///
    /// # Errors
    ///
    /// Returns an error if the SQL template cannot be parsed (invalid regex pattern).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use sqlx_named_bind::PreparedQuery;
    ///
    /// let query = PreparedQuery::<_, sqlx::Sqlite, _>::new(
    ///     "SELECT * FROM users WHERE id = :id",
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
        let template = template.into();
        let order = Regex::new(r":[a-zA-Z0-9_]+")?
            .find_iter(&template)
            .map(|m| m.as_str().to_owned())
            .collect();
        let sql = build_query(&template)?;
        Ok(Self {
            sql,
            order,
            binder,
            db_ty: PhantomData,
            db_arg: PhantomData,
        })
    }

    /// Executes the prepared query using the provided executor.
    ///
    /// This method constructs a fresh `Query` on each call, avoiding self-referential
    /// lifetime issues. It works with any SQLx `Executor` implementation, including
    /// `SqlitePool`, `Transaction`, and others.
    ///
    /// # Arguments
    ///
    /// * `executor` - Any SQLx executor (pool, transaction, etc.)
    ///
    /// # Returns
    ///
    /// Returns the backend's query result containing information about affected rows,
    /// last insert ID, etc.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use sqlx::SqlitePool;
    /// use sqlx_named_bind::PreparedQuery;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = SqlitePool::connect("sqlite::memory:").await?;
    /// let mut query = PreparedQuery::new(
    ///     "DELETE FROM users WHERE id = :id",
    ///     |q, key| match key {
    ///         ":id" => q.bind(42),
    ///         _ => q,
    ///     }
    /// )?;
    ///
    /// let result = query.execute(&pool).await?;
    /// println!("Deleted {} rows", result.rows_affected());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute<'e, E>(&mut self, executor: E) -> crate::Result<DB::QueryResult>
    where
        E: Executor<'e, Database = DB>,
    {
        let Self {
            sql, order, binder, ..
        } = self;

        let mut q = sqlx::query::<DB>(AssertSqlSafe(sql.as_str()));
        for key in order.iter() {
            q = binder(q, key);
        }
        Ok(q.execute(executor).await?)
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::SqlitePreparedQuery;
    use sqlx::{Connection, SqliteConnection};

    #[test]
    fn test_prepared_query_placeholder_order() {
        let query = SqlitePreparedQuery::new(
            "SELECT * FROM users WHERE id = :id AND name = :name OR user_id = :id",
            |q, _| q,
        )
        .unwrap();

        assert_eq!(query.order, vec![":id", ":name", ":id"]);
        assert_eq!(
            query.sql,
            "SELECT * FROM users WHERE id = ? AND name = ? OR user_id = ?"
        );
    }

    #[tokio::test]
    async fn test_execute_repeated_placeholders_and_reuse() -> crate::Result<()> {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
        sqlx::query(AssertSqlSafe(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, alias TEXT NOT NULL)",
        ))
        .execute(&mut connection)
        .await?;

        let mut id = 0;
        let mut query = SqlitePreparedQuery::new(
            "INSERT INTO users (id, name, alias) VALUES (:id, :name, :name)",
            |q, key| match key {
                ":id" => {
                    id += 1;
                    q.bind(id)
                }
                ":name" => q.bind("Alice"),
                _ => panic!("unexpected placeholder: {key}"),
            },
        )?;

        assert_eq!(query.execute(&mut connection).await?.rows_affected(), 1);
        assert_eq!(query.execute(&mut connection).await?.rows_affected(), 1);

        let rows: Vec<(i32, String, String)> = sqlx::query_as(AssertSqlSafe(
            "SELECT id, name, alias FROM users ORDER BY id",
        ))
        .fetch_all(&mut connection)
        .await?;
        assert_eq!(
            rows,
            vec![
                (1, "Alice".to_owned(), "Alice".to_owned()),
                (2, "Alice".to_owned(), "Alice".to_owned()),
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_transaction_commit_and_rollback() -> crate::Result<()> {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
        sqlx::query(AssertSqlSafe(
            "CREATE TABLE values_to_insert (value INTEGER)",
        ))
        .execute(&mut connection)
        .await?;

        let mut query = SqlitePreparedQuery::new(
            "INSERT INTO values_to_insert (value) VALUES (:value)",
            |q, key| match key {
                ":value" => q.bind(42),
                _ => panic!("unexpected placeholder: {key}"),
            },
        )?;

        let mut tx = connection.begin().await?;
        query.execute(&mut *tx).await?;
        tx.rollback().await?;

        let count: (i64,) = sqlx::query_as(AssertSqlSafe("SELECT COUNT(*) FROM values_to_insert"))
            .fetch_one(&mut connection)
            .await?;
        assert_eq!(count.0, 0);

        let mut tx = connection.begin().await?;
        query.execute(&mut *tx).await?;
        tx.commit().await?;

        let values: Vec<(i32,)> =
            sqlx::query_as(AssertSqlSafe("SELECT value FROM values_to_insert"))
                .fetch_all(&mut connection)
                .await?;
        assert_eq!(values, vec![(42,)]);
        Ok(())
    }

    #[tokio::test]
    async fn test_execute_propagates_database_error() -> crate::Result<()> {
        let mut connection = SqliteConnection::connect("sqlite::memory:").await?;
        let mut query =
            SqlitePreparedQuery::new("INSERT INTO missing_table (id) VALUES (:id)", |q, key| {
                match key {
                    ":id" => q.bind(1),
                    _ => panic!("unexpected placeholder: {key}"),
                }
            })?;

        assert!(matches!(
            query.execute(&mut connection).await,
            Err(crate::Error::Database(_))
        ));
        Ok(())
    }
}
