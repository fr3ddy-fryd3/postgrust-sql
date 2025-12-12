/// System Catalogs for `PostgreSQL` compatibility (v2.0.0)
///
/// Implements virtual tables:
/// - `pg_catalog.pg_class` (tables, indexes, views)
/// - `pg_catalog.pg_attribute` (columns)
/// - `pg_catalog.pg_index` (index definitions)
/// - `pg_catalog.pg_type` (data types)
/// - `pg_catalog.pg_namespace` (schemas)
/// - `information_schema.tables`
/// - `information_schema.columns`
///
/// These are read-only metadata tables queried by psql, `pg_dump`, etc.

use crate::core::{Database, DatabaseError, DataType};
use super::dispatcher_executor::QueryResult;

pub struct SystemCatalog;

impl SystemCatalog {
    /// Check if table name is a system catalog
    #[must_use] 
    pub fn is_system_catalog(table_name: &str) -> bool {
        matches!(
            table_name,
            "pg_catalog.pg_class"
                | "pg_catalog.pg_attribute"
                | "pg_catalog.pg_index"
                | "pg_catalog.pg_type"
                | "pg_catalog.pg_namespace"
                | "information_schema.tables"
                | "information_schema.columns"
        )
    }

    /// Query system catalog
    pub fn query(
        table_name: &str,
        db: &Database,
    ) -> Result<QueryResult, DatabaseError> {
        match table_name {
            "pg_catalog.pg_class" => Self::pg_class(db),
            "pg_catalog.pg_attribute" => Self::pg_attribute(db),
            "pg_catalog.pg_index" => Self::pg_index(db),
            "pg_catalog.pg_type" => Self::pg_type(),
            "pg_catalog.pg_namespace" => Self::pg_namespace(),
            "information_schema.tables" => Self::information_schema_tables(db),
            "information_schema.columns" => Self::information_schema_columns(db),
            _ => Err(DatabaseError::TableNotFound(table_name.to_string())),
        }
    }

    /// `pg_catalog.pg_class` - Tables, indexes, views
    ///
    /// Simplified schema:
    /// - oid: Object ID (fake)
    /// - relname: Relation name
    /// - relnamespace: Namespace OID (always 2200 = public)
    /// - relkind: 'r' = table, 'i' = index, 'v' = view
    fn pg_class(db: &Database) -> Result<QueryResult, DatabaseError> {
        let columns = vec![
            "oid".to_string(),
            "relname".to_string(),
            "relnamespace".to_string(),
            "relkind".to_string(),
        ];

        let mut rows = Vec::new();
        let mut oid = 16384; // PostgreSQL user object OIDs start at 16384

        // Tables
        for table_name in db.tables.keys() {
            rows.push(vec![
                oid.to_string(),
                table_name.clone(),
                "2200".to_string(), // public schema
                "r".to_string(),    // table
            ]);
            oid += 1;
        }

        // Views
        for view_name in db.views.keys() {
            rows.push(vec![
                oid.to_string(),
                view_name.clone(),
                "2200".to_string(),
                "v".to_string(), // view
            ]);
            oid += 1;
        }

        // Indexes
        for index_name in db.indexes.keys() {
            rows.push(vec![
                oid.to_string(),
                index_name.clone(),
                "2200".to_string(),
                "i".to_string(), // index
            ]);
            oid += 1;
        }

        Ok(QueryResult::Rows(rows, columns))
    }

    /// `pg_catalog.pg_attribute` - Columns
    ///
    /// Schema:
    /// - attrelid: Table OID
    /// - attname: Column name
    /// - atttypid: Data type OID
    /// - attnum: Column number (1-indexed)
    /// - attnotnull: NOT NULL constraint
    fn pg_attribute(db: &Database) -> Result<QueryResult, DatabaseError> {
        let columns = vec![
            "attrelid".to_string(),
            "attname".to_string(),
            "atttypid".to_string(),
            "attnum".to_string(),
            "attnotnull".to_string(),
        ];

        let mut rows = Vec::new();
        let mut oid = 16384;

        for table in db.tables.values() {
            for (col_idx, col) in table.columns.iter().enumerate() {
                let type_oid = Self::data_type_to_oid(&col.data_type);
                rows.push(vec![
                    oid.to_string(),
                    col.name.clone(),
                    type_oid.to_string(),
                    (col_idx + 1).to_string(), // 1-indexed
                    (!col.nullable).to_string(),
                ]);
            }
            oid += 1;
        }

        Ok(QueryResult::Rows(rows, columns))
    }

    /// `pg_catalog.pg_index` - Index definitions
    ///
    /// Schema:
    /// - indexrelid: Index OID
    /// - indrelid: Table OID
    /// - indkey: Column numbers (space-separated)
    /// - indisunique: Unique index?
    fn pg_index(db: &Database) -> Result<QueryResult, DatabaseError> {
        let columns = vec![
            "indexrelid".to_string(),
            "indrelid".to_string(),
            "indkey".to_string(),
            "indisunique".to_string(),
        ];

        let mut rows = Vec::new();
        let mut index_oid = 17000; // Arbitrary offset
        let table_oid = 16384; // Match pg_class

        for index in db.indexes.values() {
            // Get column names and represent as column numbers
            let column_names = index.column_names();
            // For now, just use sequential numbers (proper impl would look up actual positions)
            let indkey = (1..=column_names.len())
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(" ");

            rows.push(vec![
                index_oid.to_string(),
                table_oid.to_string(),
                indkey,
                index.is_unique().to_string(),
            ]);
            index_oid += 1;
        }

        Ok(QueryResult::Rows(rows, columns))
    }

    /// `pg_catalog.pg_type` - Data types
    ///
    /// Returns all supported data types
    fn pg_type() -> Result<QueryResult, DatabaseError> {
        let columns = vec![
            "oid".to_string(),
            "typname".to_string(),
            "typlen".to_string(), // -1 = variable length
        ];

        let types = vec![
            (16, "bool", "1"),
            (20, "int8", "8"),
            (21, "int2", "2"),
            (23, "int4", "4"),
            (25, "text", "-1"),
            (700, "float4", "4"),
            (701, "float8", "8"),
            (1043, "varchar", "-1"),
            (1082, "date", "4"),
            (1114, "timestamp", "8"),
            (1184, "timestamptz", "8"),
            (1700, "numeric", "-1"),
            (2950, "uuid", "16"),
            (3802, "jsonb", "-1"),
            (114, "json", "-1"),
            (17, "bytea", "-1"),
        ];

        let rows = types
            .into_iter()
            .map(|(oid, name, len)| {
                vec![oid.to_string(), name.to_string(), len.to_string()]
            })
            .collect();

        Ok(QueryResult::Rows(rows, columns))
    }

    /// `pg_catalog.pg_namespace` - Schemas
    ///
    /// For now, only 'public' schema
    fn pg_namespace() -> Result<QueryResult, DatabaseError> {
        let columns = vec!["oid".to_string(), "nspname".to_string()];
        let rows = vec![
            vec!["11".to_string(), "pg_catalog".to_string()],
            vec!["2200".to_string(), "public".to_string()],
        ];
        Ok(QueryResult::Rows(rows, columns))
    }

    /// `information_schema.tables` - Standard SQL metadata
    fn information_schema_tables(db: &Database) -> Result<QueryResult, DatabaseError> {
        let columns = vec![
            "table_catalog".to_string(),
            "table_schema".to_string(),
            "table_name".to_string(),
            "table_type".to_string(),
        ];

        let mut rows = Vec::new();

        // User tables
        for table_name in db.tables.keys() {
            rows.push(vec![
                "rustdb".to_string(),
                "public".to_string(),
                table_name.clone(),
                "BASE TABLE".to_string(),
            ]);
        }

        // Views
        for view_name in db.views.keys() {
            rows.push(vec![
                "rustdb".to_string(),
                "public".to_string(),
                view_name.clone(),
                "VIEW".to_string(),
            ]);
        }

        Ok(QueryResult::Rows(rows, columns))
    }

    /// `information_schema.columns` - Column metadata
    fn information_schema_columns(db: &Database) -> Result<QueryResult, DatabaseError> {
        let columns = vec![
            "table_catalog".to_string(),
            "table_schema".to_string(),
            "table_name".to_string(),
            "column_name".to_string(),
            "ordinal_position".to_string(),
            "is_nullable".to_string(),
            "data_type".to_string(),
        ];

        let mut rows = Vec::new();

        for table in db.tables.values() {
            for (col_idx, col) in table.columns.iter().enumerate() {
                rows.push(vec![
                    "rustdb".to_string(),
                    "public".to_string(),
                    table.name.clone(),
                    col.name.clone(),
                    (col_idx + 1).to_string(),
                    if col.nullable { "YES" } else { "NO" }.to_string(),
                    Self::data_type_to_sql_name(&col.data_type),
                ]);
            }
        }

        Ok(QueryResult::Rows(rows, columns))
    }

    /// Convert `DataType` to `PostgreSQL` OID
    const fn data_type_to_oid(data_type: &DataType) -> i32 {
        match data_type {
            DataType::Boolean => 16,
            DataType::SmallInt => 21,
            DataType::Integer => 23,
            DataType::Serial => 23,
            DataType::BigSerial => 20,
            DataType::Real => 700,
            DataType::Numeric { .. } => 1700,
            DataType::Text => 25,
            DataType::Varchar { .. } => 1043,
            DataType::Char { .. } => 1042,
            DataType::Date => 1082,
            DataType::Timestamp => 1114,
            DataType::TimestampTz => 1184,
            DataType::Uuid => 2950,
            DataType::Json => 114,
            DataType::Jsonb => 3802,
            DataType::Bytea => 17,
            DataType::Enum { .. } => 25, // Treat as text
        }
    }

    /// Convert `DataType` to SQL type name
    fn data_type_to_sql_name(data_type: &DataType) -> String {
        match data_type {
            DataType::Boolean => "boolean".to_string(),
            DataType::SmallInt => "smallint".to_string(),
            DataType::Integer => "integer".to_string(),
            DataType::Serial => "serial".to_string(),
            DataType::BigSerial => "bigserial".to_string(),
            DataType::Real => "real".to_string(),
            DataType::Numeric { precision, scale } => {
                format!("numeric({precision},{scale})")
            }
            DataType::Text => "text".to_string(),
            DataType::Varchar { max_length } => format!("varchar({max_length})"),
            DataType::Char { length } => format!("char({length})"),
            DataType::Date => "date".to_string(),
            DataType::Timestamp => "timestamp".to_string(),
            DataType::TimestampTz => "timestamptz".to_string(),
            DataType::Uuid => "uuid".to_string(),
            DataType::Json => "json".to_string(),
            DataType::Jsonb => "jsonb".to_string(),
            DataType::Bytea => "bytea".to_string(),
            DataType::Enum { name, .. } => name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Table, Column};

    #[test]
    fn test_is_system_catalog() {
        assert!(SystemCatalog::is_system_catalog("pg_catalog.pg_class"));
        assert!(SystemCatalog::is_system_catalog("information_schema.tables"));
        assert!(!SystemCatalog::is_system_catalog("users"));
    }

    #[test]
    fn test_pg_namespace() {
        let result = SystemCatalog::pg_namespace().unwrap();
        match result {
            QueryResult::Rows(rows, cols) => {
                assert_eq!(cols, vec!["oid", "nspname"]);
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0], vec!["11", "pg_catalog"]);
                assert_eq!(rows[1], vec!["2200", "public"]);
            }
            _ => panic!("Expected Rows"),
        }
    }

    #[test]
    fn test_pg_type() {
        let result = SystemCatalog::pg_type().unwrap();
        match result {
            QueryResult::Rows(rows, cols) => {
                assert_eq!(cols, vec!["oid", "typname", "typlen"]);
                assert!(rows.len() >= 16); // At least 16 types
                // Check bool type
                assert!(rows.iter().any(|r| r[1] == "bool" && r[0] == "16"));
                // Check text type
                assert!(rows.iter().any(|r| r[1] == "text" && r[0] == "25"));
            }
            _ => panic!("Expected Rows"),
        }
    }

    #[test]
    fn test_pg_class() {
        let mut db = Database::new("test".to_string());
        let table = Table::new(
            "users".to_string(),
            vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            }],
        );
        db.create_table(table).unwrap();

        let result = SystemCatalog::pg_class(&db).unwrap();

        match result {
            QueryResult::Rows(rows, cols) => {
                assert_eq!(cols, vec!["oid", "relname", "relnamespace", "relkind"]);
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][1], "users");
                assert_eq!(rows[0][2], "2200"); // public schema
                assert_eq!(rows[0][3], "r"); // table
            }
            _ => panic!("Expected Rows"),
        }
    }

    #[test]
    fn test_pg_attribute() {
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
                    nullable: true,
                    primary_key: false,
                    unique: false,
                    foreign_key: None,
                },
            ],
        );
        db.create_table(table).unwrap();

        let result = SystemCatalog::pg_attribute(&db).unwrap();

        match result {
            QueryResult::Rows(rows, cols) => {
                assert_eq!(
                    cols,
                    vec!["attrelid", "attname", "atttypid", "attnum", "attnotnull"]
                );
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0][1], "id");
                assert_eq!(rows[0][2], "23"); // INTEGER OID
                assert_eq!(rows[0][3], "1"); // First column
                assert_eq!(rows[0][4], "true"); // NOT NULL
                assert_eq!(rows[1][1], "name");
                assert_eq!(rows[1][2], "25"); // TEXT OID
                assert_eq!(rows[1][4], "false"); // NULLABLE
            }
            _ => panic!("Expected Rows"),
        }
    }

    #[test]
    fn test_information_schema_tables() {
        let mut db = Database::new("test".to_string());
        let table = Table::new("users".to_string(), vec![]);
        db.create_table(table).unwrap();

        let result = SystemCatalog::information_schema_tables(&db).unwrap();

        match result {
            QueryResult::Rows(rows, cols) => {
                assert_eq!(
                    cols,
                    vec!["table_catalog", "table_schema", "table_name", "table_type"]
                );
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][2], "users");
                assert_eq!(rows[0][3], "BASE TABLE");
            }
            _ => panic!("Expected Rows"),
        }
    }

    #[test]
    fn test_information_schema_columns() {
        let mut db = Database::new("test".to_string());
        let table = Table::new(
            "users".to_string(),
            vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            }],
        );
        db.create_table(table).unwrap();

        let result = SystemCatalog::information_schema_columns(&db).unwrap();

        match result {
            QueryResult::Rows(rows, cols) => {
                assert_eq!(
                    cols,
                    vec![
                        "table_catalog",
                        "table_schema",
                        "table_name",
                        "column_name",
                        "ordinal_position",
                        "is_nullable",
                        "data_type"
                    ]
                );
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][2], "users");
                assert_eq!(rows[0][3], "id");
                assert_eq!(rows[0][4], "1");
                assert_eq!(rows[0][5], "NO");
                assert_eq!(rows[0][6], "integer");
            }
            _ => panic!("Expected Rows"),
        }
    }
}
