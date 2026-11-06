use crate::builder::build_query;
use regex::Regex;
use sqlx::query::Query;
use sqlx::Executor;
use sqlx::{AssertSqlSafe, Database, IntoArguments};
use std::marker::PhantomData;

/// Type alias for SQLx Query with MySQL arguments
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
/// use sqlx::MySqlPool;
/// use sqlx_named_bind::PreparedQuery;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let pool = MySqlPool::connect("mysql://localhost/test").await?;
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
/// use sqlx::{MySqlPool, Transaction, MySql};
/// use sqlx_named_bind::PreparedQuery;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let pool = MySqlPool::connect("mysql://localhost/test").await?;
/// let mut tx: Transaction<MySql> = pool.begin().await?;
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
    /// let query = PreparedQuery::new(
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
    /// `MySqlPool`, `Transaction`, and others.
    ///
    /// # Arguments
    ///
    /// * `executor` - Any SQLx executor (pool, transaction, etc.)
    ///
    /// # Returns
    ///
    /// Returns the MySQL query result containing information about affected rows,
    /// last insert ID, etc.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use sqlx::MySqlPool;
    /// use sqlx_named_bind::PreparedQuery;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = MySqlPool::connect("mysql://localhost/test").await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_prepared_query_new() {
    //     let result = PreparedQuery::new("SELECT * FROM users WHERE id = :id", |q, _| q);
    //     assert!(result.is_ok());
    // }
    //
    // #[test]
    // fn test_prepared_query_placeholder_order() {
    //     let query = PreparedQuery::new(
    //         "SELECT * FROM users WHERE id = :id AND name = :name",
    //         |q, _| q,
    //     )
    //     .unwrap();
    //
    //     assert_eq!(query.order, vec![":id", ":name"]);
    //     assert_eq!(query.sql, "SELECT * FROM users WHERE id = ? AND name = ?");
    // }
    //
    // #[test]
    // fn test_prepared_query_repeated_placeholders() {
    //     let query = PreparedQuery::new(
    //         "SELECT * FROM users WHERE id = :id OR user_id = :id",
    //         |q, _| q,
    //     )
    //     .unwrap();
    //
    //     // Both occurrences should be captured
    //     assert_eq!(query.order, vec![":id", ":id"]);
    //     assert_eq!(query.sql, "SELECT * FROM users WHERE id = ? OR user_id = ?");
    // }
}
