use crate::Platform;
use regex::Regex;

/// Converts named placeholders to the platform's positional placeholders.
///
/// Repeated names receive a new position for every occurrence.
///
/// ```
/// use sqlx_named_bind::{builder::build_query_for_platform, Platform};
///
/// let sql = build_query_for_platform(
///     "SELECT :id::int, :name, :id",
///     Platform::Postgres,
/// )?;
/// assert_eq!(sql, "SELECT $1::int, $2, $3");
/// # Ok::<(), sqlx_named_bind::Error>(())
/// ```
pub fn build_query_for_platform(template: &str, platform: Platform) -> crate::Result<String> {
    parse_query(template, platform).map(|(sql, _)| sql)
}

pub(crate) fn parse_query(
    template: &str,
    platform: Platform,
) -> crate::Result<(String, Vec<String>)> {
    let regex = Regex::new(r"::[a-zA-Z0-9_]+|:[a-zA-Z0-9_]+")?;
    let mut order = Vec::new();
    let sql = regex
        .replace_all(template, |captures: &regex::Captures<'_>| {
            let name = &captures[0];
            if name.starts_with("::") {
                return name.to_owned();
            }
            let placeholder = platform.placeholder(order.len());
            order.push(name.to_owned());
            placeholder
        })
        .into_owned();
    Ok((sql, order))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_query_single_param() {
        let result =
            build_query_for_platform("SELECT * FROM users WHERE id = :id", Platform::MySql)
                .unwrap();
        assert_eq!(result, "SELECT * FROM users WHERE id = ?");
    }

    #[test]
    fn test_build_query_multiple_params() {
        let result = build_query_for_platform(
            "SELECT * FROM users WHERE id = :id AND name = :name",
            Platform::MySql,
        )
        .unwrap();
        assert_eq!(result, "SELECT * FROM users WHERE id = ? AND name = ?");
    }

    #[test]
    fn test_build_query_repeated_params() {
        let result = build_query_for_platform(
            "SELECT * FROM users WHERE id = :id OR user_id = :id",
            Platform::MySql,
        )
        .unwrap();
        assert_eq!(result, "SELECT * FROM users WHERE id = ? OR user_id = ?");
    }

    #[test]
    fn test_build_query_no_params() {
        let result = build_query_for_platform("SELECT * FROM users", Platform::MySql).unwrap();
        assert_eq!(result, "SELECT * FROM users");
    }

    #[test]
    fn test_build_query_with_underscores() {
        let result = build_query_for_platform(
            "SELECT * FROM users WHERE user_id = :user_id",
            Platform::MySql,
        )
        .unwrap();
        assert_eq!(result, "SELECT * FROM users WHERE user_id = ?");
    }

    #[test]
    fn test_platform_placeholders_and_binding_order() {
        let template = "SELECT :user_id, :name, :user_id";
        for (platform, expected) in [
            (Platform::MySql, "SELECT ?, ?, ?"),
            (Platform::Sqlite, "SELECT ?, ?, ?"),
            (Platform::Mssql, "SELECT ?, ?, ?"),
            (Platform::Postgres, "SELECT $1, $2, $3"),
        ] {
            let (sql, order) = parse_query(template, platform).unwrap();
            assert_eq!(sql, expected);
            assert_eq!(order, vec![":user_id", ":name", ":user_id"]);
        }
    }

    #[test]
    fn test_postgres_casts_do_not_create_bindings() {
        let (sql, order) = parse_query(
            "SELECT :id::int, NOW()::date, :id::bigint",
            Platform::Postgres,
        )
        .unwrap();
        assert_eq!(sql, "SELECT $1::int, NOW()::date, $2::bigint");
        assert_eq!(order, vec![":id", ":id"]);
    }

    #[test]
    fn test_postgres_numbering_beyond_nine() {
        let template = std::iter::repeat_n(":id", 12)
            .collect::<Vec<_>>()
            .join(", ");
        let sql = build_query_for_platform(&template, Platform::Postgres).unwrap();
        assert_eq!(sql, "$1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12");
    }

    #[test]
    fn test_no_parameters_for_each_platform() {
        for platform in [
            Platform::MySql,
            Platform::Postgres,
            Platform::Sqlite,
            Platform::Mssql,
        ] {
            let (sql, order) = parse_query("SELECT 1", platform).unwrap();
            assert_eq!(sql, "SELECT 1");
            assert!(order.is_empty());
        }
    }
}
