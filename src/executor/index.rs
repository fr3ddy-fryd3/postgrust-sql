/// Index management executor
///
/// Handles CREATE INDEX, DROP INDEX operations

use crate::types::{Database, DatabaseError};
use crate::executor::QueryResult;
use crate::index::BTreeIndex;

pub struct IndexExecutor;

impl IndexExecutor {
    /// Execute CREATE INDEX
    ///
    /// Creates a B-tree index on specified column.
    /// Populates index with existing data from table.
    pub fn create_index(
        db: &mut Database,
        name: String,
        table_name: String,
        column_name: String,
        unique: bool,
    ) -> Result<QueryResult, DatabaseError> {
        // Check if index already exists
        if db.indexes.contains_key(&name) {
            return Err(DatabaseError::ParseError(
                format!("Index '{}' already exists", name)
            ));
        }

        // Check if table exists
        let table = db.get_table(&table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.clone()))?;

        // Find column index
        let column_idx = table.columns.iter()
            .position(|c| c.name == column_name)
            .ok_or_else(|| DatabaseError::ColumnNotFound(column_name.clone()))?;

        // Create index
        let mut index = BTreeIndex::new(
            name.clone(),
            table_name.clone(),
            column_name.clone(),
            unique,
        );

        // Populate index with existing data
        for (row_idx, row) in table.rows.iter().enumerate() {
            let value = &row.values[column_idx];
            index.insert(value, row_idx)?;
        }

        // Store index
        db.indexes.insert(name.clone(), index);

        Ok(QueryResult::Success(format!(
            "Index '{}' created on {}.{}",
            name, table_name, column_name
        )))
    }

    /// Execute DROP INDEX
    pub fn drop_index(
        db: &mut Database,
        name: String,
    ) -> Result<QueryResult, DatabaseError> {
        if db.indexes.remove(&name).is_none() {
            return Err(DatabaseError::ParseError(
                format!("Index '{}' does not exist", name)
            ));
        }

        Ok(QueryResult::Success(format!("Index '{}' dropped", name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Table, Column, DataType, Row, Value};

    #[test]
    fn test_create_index() {
        let mut db = Database::new("test".to_string());

        // Create table with data
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: false,
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
        ];
        let mut table = Table::new("users".to_string(), columns);
        table.rows = vec![
            Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]),
            Row::new(vec![Value::Integer(2), Value::Text("Bob".to_string())]),
            Row::new(vec![Value::Integer(3), Value::Text("Charlie".to_string())]),
        ];
        db.create_table(table).unwrap();

        // Create index
        let result = IndexExecutor::create_index(
            &mut db,
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            true,
        );

        assert!(result.is_ok());
        assert!(db.indexes.contains_key("idx_id"));

        // Verify index is populated
        let index = db.indexes.get("idx_id").unwrap();
        assert_eq!(index.entry_count(), 3);
        assert_eq!(index.search(&Value::Integer(1)), vec![0]);
        assert_eq!(index.search(&Value::Integer(2)), vec![1]);
    }

    #[test]
    fn test_create_duplicate_index() {
        let mut db = Database::new("test".to_string());

        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];
        let table = Table::new("users".to_string(), columns);
        db.create_table(table).unwrap();

        IndexExecutor::create_index(
            &mut db,
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            true,
        ).unwrap();

        // Try to create duplicate
        let result = IndexExecutor::create_index(
            &mut db,
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            true,
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_drop_index() {
        let mut db = Database::new("test".to_string());

        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];
        let table = Table::new("users".to_string(), columns);
        db.create_table(table).unwrap();

        IndexExecutor::create_index(
            &mut db,
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            true,
        ).unwrap();

        // Drop index
        let result = IndexExecutor::drop_index(&mut db, "idx_id".to_string());
        assert!(result.is_ok());
        assert!(!db.indexes.contains_key("idx_id"));
    }
}
