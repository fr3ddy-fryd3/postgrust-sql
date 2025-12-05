/// Index management executor
///
/// Handles CREATE INDEX, DROP INDEX operations

use crate::types::{Database, DatabaseError};
use crate::executor::QueryResult;
use crate::index::{Index, IndexType, BTreeIndex, HashIndex};

pub struct IndexExecutor;

impl IndexExecutor {
    /// Execute CREATE INDEX
    ///
    /// Creates a B-tree or Hash index on specified column.
    /// Populates index with existing data from table.
    pub fn create_index(
        db: &mut Database,
        name: String,
        table_name: String,
        column_name: String,
        unique: bool,
        index_type: IndexType,
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

        // Create index based on type
        let mut index = match index_type {
            IndexType::BTree => {
                Index::BTree(BTreeIndex::new(
                    name.clone(),
                    table_name.clone(),
                    column_name.clone(),
                    unique,
                ))
            }
            IndexType::Hash => {
                Index::Hash(HashIndex::new(
                    name.clone(),
                    table_name.clone(),
                    column_name.clone(),
                    unique,
                ))
            }
        };

        // Populate index with existing data
        for (row_idx, row) in table.rows.iter().enumerate() {
            let value = &row.values[column_idx];
            index.insert(value, row_idx)?;
        }

        // Store index
        db.indexes.insert(name.clone(), index);

        Ok(QueryResult::Success(format!(
            "Index '{}' created on {}.{} using {}",
            name, table_name, column_name, index_type.as_str()
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
    fn test_create_btree_index() {
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
        ];
        db.create_table(table).unwrap();

        let result = IndexExecutor::create_index(
            &mut db,
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            false,
            IndexType::BTree,
        );

        assert!(result.is_ok());
        assert!(db.indexes.contains_key("idx_id"));

        let index = &db.indexes["idx_id"];
        assert_eq!(index.search(&Value::Integer(1)), vec![0]);
        assert_eq!(index.search(&Value::Integer(2)), vec![1]);
    }

    #[test]
    fn test_create_hash_index() {
        let mut db = Database::new("test".to_string());

        let columns = vec![
            Column {
                name: "category".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];
        let mut table = Table::new("products".to_string(), columns);
        table.rows = vec![
            Row::new(vec![Value::Text("Electronics".to_string())]),
            Row::new(vec![Value::Text("Books".to_string())]),
        ];
        db.create_table(table).unwrap();

        let result = IndexExecutor::create_index(
            &mut db,
            "idx_category".to_string(),
            "products".to_string(),
            "category".to_string(),
            false,
            IndexType::Hash,
        );

        assert!(result.is_ok());
        assert!(db.indexes.contains_key("idx_category"));

        let index = &db.indexes["idx_category"];
        assert_eq!(index.search(&Value::Text("Electronics".to_string())), vec![0]);
        assert_eq!(index.search(&Value::Text("Books".to_string())), vec![1]);
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
            false,
            IndexType::BTree,
        )
        .unwrap();

        let result = IndexExecutor::create_index(
            &mut db,
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            false,
            IndexType::BTree,
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
            false,
            IndexType::BTree,
        )
        .unwrap();

        let result = IndexExecutor::drop_index(&mut db, "idx_id".to_string());
        assert!(result.is_ok());
        assert!(!db.indexes.contains_key("idx_id"));
    }
}
