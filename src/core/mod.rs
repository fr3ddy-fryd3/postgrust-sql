// Module declarations
pub mod error;
pub mod value;
pub mod data_type;
pub mod constraints;
pub mod column;
pub mod row;
pub mod table;
pub mod database;
pub mod privilege;
pub mod user;
pub mod database_metadata;
pub mod server_instance;

// Re-exports for convenience
pub use error::DatabaseError;
pub use value::Value;
pub use data_type::DataType;
pub use constraints::ForeignKey;
pub use column::Column;
pub use row::Row;
pub use table::Table;
pub use database::Database;
pub use privilege::Privilege;
pub use user::User;
pub use database_metadata::DatabaseMetadata;
pub use server_instance::ServerInstance;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_display() {
        assert_eq!(Value::Null.to_string(), "NULL");
        assert_eq!(Value::Integer(42).to_string(), "42");
        assert_eq!(Value::Real(3.14).to_string(), "3.14");
        assert_eq!(Value::Text("hello".to_string()).to_string(), "hello");
        assert_eq!(Value::Boolean(true).to_string(), "true");
    }

    #[test]
    fn test_value_as_int() {
        assert_eq!(Value::Integer(42).as_int(), Some(42));
        assert_eq!(Value::Text("hello".to_string()).as_int(), None);
        assert_eq!(Value::Null.as_int(), None);
    }

    #[test]
    fn test_value_as_text() {
        assert_eq!(Value::Text("hello".to_string()).as_text(), Some("hello"));
        assert_eq!(Value::Integer(42).as_text(), None);
    }

    #[test]
    fn test_value_as_bool() {
        assert_eq!(Value::Boolean(true).as_bool(), Some(true));
        assert_eq!(Value::Boolean(false).as_bool(), Some(false));
        assert_eq!(Value::Integer(1).as_bool(), None);
    }

    #[test]
    fn test_table_creation() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
                unique: false,
            },
        ];

        let table = Table::new("users".to_string(), columns.clone());
        assert_eq!(table.name, "users");
        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.rows.len(), 0);
    }

    #[test]
    fn test_table_insert() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
                unique: false,
            },
        ];

        let mut table = Table::new("users".to_string(), columns);
        let row = Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]);

        assert!(table.insert(row).is_ok());
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn test_table_insert_wrong_column_count() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            },
        ];

        let mut table = Table::new("users".to_string(), columns);
        let row = Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]);

        assert!(matches!(
            table.insert(row),
            Err(DatabaseError::ColumnCountMismatch)
        ));
    }

    #[test]
    fn test_table_get_column_index() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
                unique: false,
            },
        ];

        let table = Table::new("users".to_string(), columns);
        assert_eq!(table.get_column_index("id"), Some(0));
        assert_eq!(table.get_column_index("name"), Some(1));
        assert_eq!(table.get_column_index("age"), None);
    }

    #[test]
    fn test_database_creation() {
        let db = Database::new("test_db".to_string());
        assert_eq!(db.name, "test_db");
        assert_eq!(db.tables.len(), 0);
    }

    #[test]
    fn test_database_create_table() {
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            },
        ];

        let table = Table::new("users".to_string(), columns);
        assert!(db.create_table(table).is_ok());
        assert_eq!(db.tables.len(), 1);
        assert!(db.get_table("users").is_some());
    }

    #[test]
    fn test_database_create_duplicate_table() {
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            },
        ];

        let table1 = Table::new("users".to_string(), columns.clone());
        let table2 = Table::new("users".to_string(), columns);

        assert!(db.create_table(table1).is_ok());
        assert!(matches!(
            db.create_table(table2),
            Err(DatabaseError::TableAlreadyExists(_))
        ));
    }

    #[test]
    fn test_database_drop_table() {
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            },
        ];

        let table = Table::new("users".to_string(), columns);
        db.create_table(table).unwrap();

        assert!(db.drop_table("users").is_ok());
        assert_eq!(db.tables.len(), 0);
    }

    #[test]
    fn test_database_drop_nonexistent_table() {
        let mut db = Database::new("test_db".to_string());
        assert!(matches!(
            db.drop_table("users"),
            Err(DatabaseError::TableNotFound(_))
        ));
    }

    #[test]
    fn test_value_equality() {
        assert_eq!(Value::Integer(42), Value::Integer(42));
        assert_ne!(Value::Integer(42), Value::Integer(43));
        assert_eq!(Value::Text("hello".to_string()), Value::Text("hello".to_string()));
        assert_eq!(Value::Boolean(true), Value::Boolean(true));
    }
}
