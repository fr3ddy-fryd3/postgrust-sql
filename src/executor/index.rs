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
    /// Creates a B-tree or Hash index on specified column(s) - v1.9.0 supports composite
    /// Populates index with existing data from table.
    pub fn create_index(
        db: &mut Database,
        name: String,
        table_name: String,
        column_names: Vec<String>,
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

        // Validate all columns exist and get their indices
        let mut column_indices = Vec::new();
        for col_name in &column_names {
            let col_idx = table.columns.iter()
                .position(|c| &c.name == col_name)
                .ok_or_else(|| DatabaseError::ColumnNotFound(col_name.clone()))?;
            column_indices.push(col_idx);
        }

        let is_composite = column_names.len() > 1;

        // Create index based on type and column count
        let mut index = if is_composite {
            // Composite index
            match index_type {
                IndexType::BTree => {
                    Index::BTree(BTreeIndex::new_composite(
                        name.clone(),
                        table_name.clone(),
                        column_names.clone(),
                        unique,
                    ))
                }
                IndexType::Hash => {
                    Index::Hash(HashIndex::new_composite(
                        name.clone(),
                        table_name.clone(),
                        column_names.clone(),
                        unique,
                    ))
                }
            }
        } else {
            // Single column index
            match index_type {
                IndexType::BTree => {
                    Index::BTree(BTreeIndex::new(
                        name.clone(),
                        table_name.clone(),
                        column_names[0].clone(),
                        unique,
                    ))
                }
                IndexType::Hash => {
                    Index::Hash(HashIndex::new(
                        name.clone(),
                        table_name.clone(),
                        column_names[0].clone(),
                        unique,
                    ))
                }
            }
        };

        // Populate index with existing data
        for (row_idx, row) in table.rows.iter().enumerate() {
            if is_composite {
                // Extract values for all indexed columns
                let values: Vec<_> = column_indices.iter()
                    .map(|&idx| row.values[idx].clone())
                    .collect();
                index.insert_composite(&values, row_idx)?;
            } else {
                // Single column
                let value = &row.values[column_indices[0]];
                index.insert(value, row_idx)?;
            }
        }

        // Store index
        db.indexes.insert(name.clone(), index);

        let columns_str = column_names.join(", ");
        Ok(QueryResult::Success(format!(
            "Index '{}' created on {}.({}) using {}",
            name, table_name, columns_str, index_type.as_str()
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
            vec!["id".to_string()],
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
            vec!["category".to_string()],
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
            vec!["id".to_string()],
            false,
            IndexType::BTree,
        )
        .unwrap();

        let result = IndexExecutor::create_index(
            &mut db,
            "idx_id".to_string(),
            "users".to_string(),
            vec!["id".to_string()],
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
            vec!["id".to_string()],
            false,
            IndexType::BTree,
        )
        .unwrap();

        let result = IndexExecutor::drop_index(&mut db, "idx_id".to_string());
        assert!(result.is_ok());
        assert!(!db.indexes.contains_key("idx_id"));
    }

    #[test]
    fn test_create_composite_btree_index() {
        let mut db = Database::new("test".to_string());

        let columns = vec![
            Column {
                name: "city".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
            Column {
                name: "age".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];
        let mut table = Table::new("users".to_string(), columns);
        table.rows = vec![
            Row::new(vec![Value::Text("NYC".to_string()), Value::Integer(30)]),
            Row::new(vec![Value::Text("LA".to_string()), Value::Integer(25)]),
            Row::new(vec![Value::Text("NYC".to_string()), Value::Integer(25)]),
        ];
        db.create_table(table).unwrap();

        let result = IndexExecutor::create_index(
            &mut db,
            "idx_city_age".to_string(),
            "users".to_string(),
            vec!["city".to_string(), "age".to_string()],
            false,
            IndexType::BTree,
        );

        assert!(result.is_ok());
        assert!(db.indexes.contains_key("idx_city_age"));

        let index = &db.indexes["idx_city_age"];
        assert!(index.is_composite());
        assert_eq!(index.column_names().len(), 2);

        // Test composite search
        let results = index.search_composite(&vec![
            Value::Text("NYC".to_string()),
            Value::Integer(30),
        ]);
        assert_eq!(results, vec![0]);
    }

    #[test]
    fn test_create_composite_hash_index() {
        let mut db = Database::new("test".to_string());

        let columns = vec![
            Column {
                name: "first_name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
            Column {
                name: "last_name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];
        let mut table = Table::new("people".to_string(), columns);
        table.rows = vec![
            Row::new(vec![Value::Text("John".to_string()), Value::Text("Doe".to_string())]),
            Row::new(vec![Value::Text("Jane".to_string()), Value::Text("Smith".to_string())]),
            Row::new(vec![Value::Text("John".to_string()), Value::Text("Smith".to_string())]),
        ];
        db.create_table(table).unwrap();

        let result = IndexExecutor::create_index(
            &mut db,
            "idx_name".to_string(),
            "people".to_string(),
            vec!["first_name".to_string(), "last_name".to_string()],
            false,
            IndexType::Hash,
        );

        assert!(result.is_ok());
        assert!(db.indexes.contains_key("idx_name"));

        let index = &db.indexes["idx_name"];
        assert!(index.is_composite());

        // Test composite hash search
        let results = index.search_composite(&vec![
            Value::Text("John".to_string()),
            Value::Text("Doe".to_string()),
        ]);
        assert_eq!(results, vec![0]);

        let results2 = index.search_composite(&vec![
            Value::Text("John".to_string()),
            Value::Text("Smith".to_string()),
        ]);
        assert_eq!(results2, vec![2]);
    }

    #[test]
    fn test_composite_unique_index() {
        let mut db = Database::new("test".to_string());

        let columns = vec![
            Column {
                name: "email".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
            Column {
                name: "provider".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];
        let mut table = Table::new("accounts".to_string(), columns);
        table.rows = vec![
            Row::new(vec![Value::Text("user@example.com".to_string()), Value::Text("google".to_string())]),
        ];
        db.create_table(table).unwrap();

        let result = IndexExecutor::create_index(
            &mut db,
            "idx_email_provider".to_string(),
            "accounts".to_string(),
            vec!["email".to_string(), "provider".to_string()],
            true, // unique
            IndexType::BTree,
        );

        assert!(result.is_ok());

        let index = &db.indexes["idx_email_provider"];
        assert!(index.is_unique());
        assert!(index.is_composite());
    }
}
