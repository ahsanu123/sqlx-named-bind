use std::marker::PhantomData;

use crate::builder::build_query;
use regex::Regex;
use sqlx::{query::QueryAs, AssertSqlSafe, Database, Executor, IntoArguments};

/// Type alias for SQLx QueryAs with MySQL arguments
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
/// use sqlx::{MySqlPool, FromRow};
/// use sqlx_named_bind::PreparedQueryAs;
///
/// #[derive(FromRow)]
/// struct User {
///     id: i32,
///     name: String,
/// }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let pool = MySqlPool::connect("mysql://localhost/test").await?;
/// let user_id = 42;
///
/// let mut query = PreparedQueryAs::<User, _>::new(
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
    /// let query = PreparedQueryAs::<User, _>::new(
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
    /// use sqlx::{MySqlPool, FromRow};
    /// use sqlx_named_bind::PreparedQueryAs;
    ///
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = MySqlPool::connect("mysql://localhost/test").await?;
    /// let mut query = PreparedQueryAs::<User, _>::new(
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
    /// use sqlx::{MySqlPool, FromRow};
    /// use sqlx_named_bind::PreparedQueryAs;
    ///
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = MySqlPool::connect("mysql://localhost/test").await?;
    /// let mut query = PreparedQueryAs::<User, _>::new(
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
    /// use sqlx::{MySqlPool, FromRow};
    /// use sqlx_named_bind::PreparedQueryAs;
    ///
    /// #[derive(FromRow)]
    /// struct User {
    ///     id: i32,
    ///     name: String,
    /// }
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let pool = MySqlPool::connect("mysql://localhost/test").await?;
    /// let mut query = PreparedQueryAs::<User, _>::new(
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

#[cfg(test)]
mod tests {
    use super::*;

    // Mock struct for testing (requires sqlx::FromRow)
    // In real tests, this would use a real database connection

    // #[test]
    // fn test_prepared_query_as_new() {
    //     #[derive(sqlx::FromRow)]
    //     struct TestRow {
    //         #[allow(dead_code)]
    //         id: i32,
    //     }
    //
    //     let result =
    //         PreparedQueryAs::<TestRow, _>::new("SELECT id FROM users WHERE id = :id", |q, _| q);
    //     assert!(result.is_ok());
    // }
    //
    // #[test]
    // fn test_prepared_query_as_placeholder_order() {
    //     #[derive(sqlx::FromRow)]
    //     struct TestRow {
    //         #[allow(dead_code)]
    //         id: i32,
    //     }
    //
    //     let query = PreparedQueryAs::<TestRow, _>::new(
    //         "SELECT id FROM users WHERE id = :id AND name = :name",
    //         |q, _| q,
    //     )
    //     .unwrap();
    //
    //     assert_eq!(query.order, vec![":id", ":name"]);
    //     assert_eq!(query.sql, "SELECT id FROM users WHERE id = ? AND name = ?");
    // }
}
