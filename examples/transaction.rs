//! Transaction example demonstrating PreparedQuery with transactions
//!
//! Run with: cargo run --example transaction
//!
//! Uses an in-memory SQLite database; no database server or configuration is needed.

use sqlx::{sqlite::SqlitePoolOptions, AssertSqlSafe, FromRow, Sqlite, SqlitePool, Transaction};
use sqlx_named_bind::{
    SqlitePreparedQuery as PreparedQuery, SqlitePreparedQueryAs as PreparedQueryAs,
};

#[derive(Debug, FromRow)]
struct Account {
    id: i32,
    name: String,
    balance: i32,
}

async fn transfer_money(
    tx: &mut Transaction<'_, Sqlite>,
    from_id: i32,
    to_id: i32,
    amount: i32,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "  Transferring ${} from account {} to account {}",
        amount, from_id, to_id
    );

    // Debit from source account
    let mut debit = PreparedQuery::new(
        "UPDATE accounts SET balance = balance - :amount WHERE id = :id",
        |q, key| match key {
            ":amount" => q.bind(amount),
            ":id" => q.bind(from_id),
            _ => q,
        },
    )?;

    let result = debit.execute(&mut **tx).await?;
    if result.rows_affected() == 0 {
        return Err("Source account not found".into());
    }

    // Check for negative balance
    let mut check_balance = PreparedQueryAs::<(i32,), _>::new(
        "SELECT balance FROM accounts WHERE id = :id",
        |q, key| match key {
            ":id" => q.bind(from_id),
            _ => q,
        },
    )?;

    let (balance,) = check_balance.fetch_one(&mut **tx).await?;
    if balance < 0 {
        return Err(format!("Insufficient funds (balance: ${})", balance).into());
    }

    // Credit to destination account
    let mut credit = PreparedQuery::new(
        "UPDATE accounts SET balance = balance + :amount WHERE id = :id",
        |q, key| match key {
            ":amount" => q.bind(amount),
            ":id" => q.bind(to_id),
            _ => q,
        },
    )?;

    let result = credit.execute(&mut **tx).await?;
    if result.rows_affected() == 0 {
        return Err("Destination account not found".into());
    }

    println!("  ✓ Transfer completed successfully");
    Ok(())
}

async fn show_accounts(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
    let mut query = PreparedQueryAs::<Account, _>::new(
        "SELECT id, name, balance FROM accounts ORDER BY id",
        |q, _key| q,
    )?;

    let accounts = query.fetch_all(pool).await?;
    println!("\nCurrent account balances:");
    for account in &accounts {
        println!(
            "  {} (id={}): ${}",
            account.name, account.id, account.balance
        );
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to database...");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;

    // Setup: Create accounts table
    println!("\nSetting up accounts table...");
    sqlx::query(AssertSqlSafe("DROP TABLE IF EXISTS accounts"))
        .execute(&pool)
        .await?;
    sqlx::query(AssertSqlSafe(
        "CREATE TABLE accounts (
            id INTEGER PRIMARY KEY,
            name VARCHAR(100) NOT NULL,
            balance INT NOT NULL DEFAULT 0
        )",
    ))
    .execute(&pool)
    .await?;

    // Insert initial accounts
    println!("\nCreating test accounts...");
    let accounts = vec![("Alice", 1000), ("Bob", 500), ("Charlie", 750)];

    for (name, balance) in accounts {
        let mut query = PreparedQuery::new(
            "INSERT INTO accounts (name, balance) VALUES (:name, :balance)",
            |q, key| match key {
                ":name" => q.bind(name),
                ":balance" => q.bind(balance),
                _ => q,
            },
        )?;
        query.execute(&pool).await?;
    }

    show_accounts(&pool).await?;

    // Example 1: Successful transaction
    println!("\n--- Example 1: Successful transfer ---");
    let mut tx = pool.begin().await?;
    match transfer_money(&mut tx, 1, 2, 200).await {
        Ok(_) => {
            tx.commit().await?;
            println!("  ✓ Transaction committed");
        }
        Err(e) => {
            tx.rollback().await?;
            println!("  ✗ Transaction rolled back: {}", e);
        }
    }
    show_accounts(&pool).await?;

    // Example 2: Failed transaction (insufficient funds)
    println!("\n--- Example 2: Failed transfer (insufficient funds) ---");
    let mut tx = pool.begin().await?;
    match transfer_money(&mut tx, 2, 1, 1000).await {
        Ok(_) => {
            tx.commit().await?;
            println!("  ✓ Transaction committed");
        }
        Err(e) => {
            tx.rollback().await?;
            println!("  ✗ Transaction rolled back: {}", e);
        }
    }
    show_accounts(&pool).await?;

    // Example 3: Multiple transfers in one transaction
    println!("\n--- Example 3: Multiple transfers in one transaction ---");
    let mut tx = pool.begin().await?;

    let transfers = vec![
        (1, 3, 100), // Alice -> Charlie
        (3, 2, 50),  // Charlie -> Bob
    ];

    let mut success = true;
    for (from, to, amount) in transfers {
        if let Err(e) = transfer_money(&mut tx, from, to, amount).await {
            println!("  ✗ Transfer failed: {}", e);
            success = false;
            break;
        }
    }

    if success {
        tx.commit().await?;
        println!("  ✓ All transfers committed");
    } else {
        tx.rollback().await?;
        println!("  ✗ All transfers rolled back");
    }
    show_accounts(&pool).await?;

    // Cleanup
    println!("\nCleaning up...");
    sqlx::query(AssertSqlSafe("DROP TABLE IF EXISTS accounts"))
        .execute(&pool)
        .await?;

    println!("\nExample completed successfully!");
    Ok(())
}
