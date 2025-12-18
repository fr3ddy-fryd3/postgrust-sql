/// System Functions for `PostgreSQL` compatibility (v2.0.0)
///
/// Implements special functions that `PostgreSQL` clients expect:
/// - `version()` - Database version string
/// - `current_database()` - Current database name
/// - `pg_table_size(table_name)` - Table size in bytes
/// - `current_user` - Current user name
/// - `current_schema()` - Current schema name
///
/// These functions are intercepted in SELECT queries and evaluated specially.
use crate::core::{Database, DatabaseError};

pub struct SystemFunctions;

impl SystemFunctions {
    /// Check if function name is a system function
    #[must_use] 
    pub fn is_system_function(name: &str) -> bool {
        matches!(
            name.to_lowercase().as_str(),
            "version"
                | "current_database"
                | "pg_table_size"
                | "current_user"
                | "current_schema"
                | "pg_backend_pid"
                | "pg_encoding_to_char"
        )
    }

    /// Evaluate system function
    ///
    /// Returns a string value representing the function result
    pub fn evaluate(
        name: &str,
        args: &[String],
        db: &Database,
        database_storage: Option<&crate::storage::DatabaseStorage>,
    ) -> Result<String, DatabaseError> {
        match name.to_lowercase().as_str() {
            "version" => Ok(Self::version()),
            "current_database" => Ok(db.name.clone()),
            "current_schema" => Ok("public".to_string()),
            "current_user" => Ok("rustdb".to_string()),
            "pg_backend_pid" => Ok(std::process::id().to_string()),
            "pg_encoding_to_char" => Ok("UTF8".to_string()),
            "pg_table_size" => {
                if args.is_empty() {
                    return Err(DatabaseError::ParseError(
                        "pg_table_size() requires table name argument".to_string(),
                    ));
                }
                Self::pg_table_size(&args[0], db, database_storage)
            }
            _ => Err(DatabaseError::ParseError(format!(
                "Unknown system function: {name}"
            ))),
        }
    }

    /// `version()` - Return database version string
    ///
    /// Format: `RustDB` 2.0.0 on <platform>
    fn version() -> String {
        let platform = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        format!(
            "RustDB 2.0.0 on {platform}-{arch}, Rust/LLVM"
        )
    }

    /// `pg_table_size(table_name)` - Return table size in bytes
    ///
    /// Returns approximate size based on row count and average row size
    fn pg_table_size(
        table_name: &str,
        db: &Database,
        database_storage: Option<&crate::storage::DatabaseStorage>,
    ) -> Result<String, DatabaseError> {
        // Remove quotes if present
        let table_name = table_name.trim_matches('\'').trim_matches('"');

        // Check if table exists
        let table = db
            .get_table(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        // Calculate size based on storage type
        let size_bytes = if let Some(db_storage) = database_storage {
            // Page-based storage: count pages
            if let Some(paged_table) = db_storage.get_paged_table(table_name) {
                // Each page is 8KB
                let stats = paged_table.stats();
                stats.page_count as usize * 8192
            } else {
                // Fallback: estimate from schema
                Self::estimate_size_from_schema(table)
            }
        } else {
            // Legacy storage or empty table: estimate from in-memory rows or schema
            #[allow(deprecated)]
            let row_count = table.rows.len();
            if row_count > 0 {
                let avg_row_size = Self::estimate_row_size(table);
                row_count * avg_row_size
            } else {
                // Empty table - estimate from schema
                Self::estimate_size_from_schema(table)
            }
        };

        Ok(size_bytes.to_string())
    }

    /// Estimate table size from schema (when no actual data available)
    fn estimate_size_from_schema(table: &crate::core::Table) -> usize {
        // Assume average of 100 rows per table
        let estimated_rows = 100;
        let avg_row_size = Self::estimate_row_size(table);
        estimated_rows * avg_row_size
    }

    /// Estimate average row size based on column types
    fn estimate_row_size(table: &crate::core::Table) -> usize {
        let mut size = 0;
        for col in &table.columns {
            size += match &col.data_type {
                crate::core::DataType::Boolean => 1,
                crate::core::DataType::SmallInt => 2,
                crate::core::DataType::Integer => 4,
                crate::core::DataType::Serial => 4,
                crate::core::DataType::BigSerial => 8,
                crate::core::DataType::Real => 4,
                crate::core::DataType::Numeric { .. } => 16,
                crate::core::DataType::Text => 50, // Assume average text length
                crate::core::DataType::Varchar { max_length } => *max_length,
                crate::core::DataType::Char { length } => *length,
                crate::core::DataType::Date => 4,
                crate::core::DataType::Timestamp => 8,
                crate::core::DataType::TimestampTz => 8,
                crate::core::DataType::Uuid => 16,
                crate::core::DataType::Json => 100, // Assume average JSON size
                crate::core::DataType::Jsonb => 100,
                crate::core::DataType::Bytea => 50,
                crate::core::DataType::Enum { .. } => 4,
            };
        }
        size + 24 // Add overhead for xmin/xmax and row header
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Column, DataType, Table};

    #[test]
    fn test_is_system_function() {
        assert!(SystemFunctions::is_system_function("version"));
        assert!(SystemFunctions::is_system_function("VERSION"));
        assert!(SystemFunctions::is_system_function("current_database"));
        assert!(SystemFunctions::is_system_function("pg_table_size"));
        assert!(!SystemFunctions::is_system_function("foobar"));
    }

    #[test]
    fn test_version() {
        let version = SystemFunctions::version();
        assert!(version.contains("RustDB 2.0.0"));
        assert!(version.contains("Rust/LLVM"));
    }

    #[test]
    fn test_current_database() {
        let db = Database::new("test_db".to_string());
        let result = SystemFunctions::evaluate("current_database", &[], &db, None).unwrap();
        assert_eq!(result, "test_db");
    }

    #[test]
    fn test_current_schema() {
        let db = Database::new("test".to_string());
        let result = SystemFunctions::evaluate("current_schema", &[], &db, None).unwrap();
        assert_eq!(result, "public");
    }

    #[test]
    fn test_current_user() {
        let db = Database::new("test".to_string());
        let result = SystemFunctions::evaluate("current_user", &[], &db, None).unwrap();
        assert_eq!(result, "rustdb");
    }

    #[test]
    fn test_pg_backend_pid() {
        let db = Database::new("test".to_string());
        let result = SystemFunctions::evaluate("pg_backend_pid", &[], &db, None).unwrap();
        let pid: u32 = result.parse().unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn test_pg_table_size() {
        let mut db = Database::new("test".to_string());
        let table = Table::new(
            "users".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "name".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    foreign_key: None,
                },
            ],
        );
        db.create_table(table).unwrap();

        let result =
            SystemFunctions::evaluate("pg_table_size", &["users".to_string()], &db, None).unwrap();
        let size: usize = result.parse().unwrap();
        assert!(size > 0); // Should return non-zero size
    }

    #[test]
    fn test_pg_table_size_unknown_table() {
        let db = Database::new("test".to_string());
        let result =
            SystemFunctions::evaluate("pg_table_size", &["nonexistent".to_string()], &db, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_estimate_row_size() {
        let table = Table::new(
            "test".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: true,
                    unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "name".to_string(),
                    data_type: DataType::Varchar { max_length: 100 },
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    foreign_key: None,
                },
            ],
        );

        let size = SystemFunctions::estimate_row_size(&table);
        assert_eq!(size, 4 + 100 + 24); // int + varchar(100) + overhead
    }
}
