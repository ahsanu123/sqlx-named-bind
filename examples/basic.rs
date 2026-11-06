//! Basic example demonstrating PreparedQuery usage
//!
//! Run with: cargo run --example basic
//!
//! Uses an in-memory SQLite database; no database server or configuration is needed.

use sqlx::{sqlite::SqlitePoolOptions, AssertSqlSafe, FromRow};
use sqlx_named_bind::{
    SqlitePreparedQuery as PreparedQuery, SqlitePreparedQueryAs as PreparedQueryAs,
};

#[derive(Debug, FromRow)]
struct User {
    id: i32,
    name: String,
    email: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to database...");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;

    // Create table if it doesn't exist
    println!("\nCreating users table...");
    sqlx::query(AssertSqlSafe(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY,
            name VARCHAR(100) NOT NULL,
            email VARCHAR(100) NOT NULL UNIQUE
        )",
    ))
    .execute(&pool)
    .await?;

    // Example 1: Insert with PreparedQuery
    println!("\n--- Example 1: Inserting users ---");
    let users_to_insert = vec![
        ("Alice", "alice@example.com"),
        ("Bob", "bob@example.com"),
        ("Charlie", "charlie@example.com"),
    ];

    for (name, email) in users_to_insert {
        let mut query = PreparedQuery::new(
            "INSERT INTO users (name, email) VALUES (:name, :email)
             ON CONFLICT(email) DO UPDATE SET name = excluded.name",
            |q, key| match key {
                ":name" => q.bind(name),
                ":email" => q.bind(email),
                _ => q,
            },
        )?;

        let result = query.execute(&pool).await?;
        println!(
            "Inserted user '{}': last_insert_rowid={}",
            name,
            result.last_insert_rowid()
        );
    }

    // Example 2: Query all users with PreparedQueryAs
    println!("\n--- Example 2: Fetching all users ---");
    let mut query_all = PreparedQueryAs::<User, _>::new(
        "SELECT id, name, email FROM users ORDER BY id",
        |q, _key| q, // No parameters needed
    )?;

    let users = query_all.fetch_all(&pool).await?;
    println!("Found {} users:", users.len());
    for user in &users {
        println!("  - {} (id={}, email={})", user.name, user.id, user.email);
    }

    // Example 3: Query single user by email
    println!("\n--- Example 3: Finding user by email ---");
    let search_email = "alice@example.com";
    let mut query_one = PreparedQueryAs::<User, _>::new(
        "SELECT id, name, email FROM users WHERE email = :email",
        |q, key| match key {
            ":email" => q.bind(search_email),
            _ => q,
        },
    )?;

    match query_one.fetch_optional(&pool).await? {
        Some(user) => println!("Found user: {} ({})", user.name, user.email),
        None => println!("User with email '{}' not found", search_email),
    }

    // Example 4: Update user with PreparedQuery
    println!("\n--- Example 4: Updating user ---");
    let update_email = "bob@example.com";
    let new_name = "Robert";

    let mut update_query = PreparedQuery::new(
        "UPDATE users SET name = :name WHERE email = :email",
        |q, key| match key {
            ":name" => q.bind(new_name),
            ":email" => q.bind(update_email),
            _ => q,
        },
    )?;

    let result = update_query.execute(&pool).await?;
    println!("Updated {} row(s)", result.rows_affected());

    // Verify the update
    let mut verify_query = PreparedQueryAs::<User, _>::new(
        "SELECT id, name, email FROM users WHERE email = :email",
        |q, key| match key {
            ":email" => q.bind(update_email),
            _ => q,
        },
    )?;

    if let Some(user) = verify_query.fetch_optional(&pool).await? {
        println!("Updated user is now: {} ({})", user.name, user.email);
    }

    // Example 5: Delete user with PreparedQuery
    println!("\n--- Example 5: Deleting user ---");
    let delete_email = "charlie@example.com";

    let mut delete_query = PreparedQuery::new(
        "DELETE FROM users WHERE email = :email",
        |q, key| match key {
            ":email" => q.bind(delete_email),
            _ => q,
        },
    )?;

    let result = delete_query.execute(&pool).await?;
    println!("Deleted {} row(s)", result.rows_affected());

    // Show final state
    println!("\n--- Final state ---");
    let mut final_query = PreparedQueryAs::<User, _>::new(
        "SELECT id, name, email FROM users ORDER BY id",
        |q, _key| q,
    )?;

    let users = final_query.fetch_all(&pool).await?;
    println!("Remaining {} users:", users.len());
    for user in &users {
        println!("  - {} (id={}, email={})", user.name, user.id, user.email);
    }

    // Cleanup
    println!("\nCleaning up...");
    sqlx::query(AssertSqlSafe("DROP TABLE IF EXISTS users"))
        .execute(&pool)
        .await?;

    println!("\nExample completed successfully!");
    Ok(())
}
