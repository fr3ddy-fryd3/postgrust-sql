use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::column::Column;
use super::row::Row;
use super::data_type::DataType;
use super::error::DatabaseError;

/// Storage backend mode for Table
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageMode {
    /// In-memory Vec<Row> (default, for backward compatibility)
    InMemory,
    /// Page-based storage (new, for better performance)
    #[allow(dead_code)]
    PageBased,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    /// v2.0.0: Kept only for serialization compatibility
    /// Actual row storage uses `PagedTable` (managed by `DatabaseStorage`)
    /// This field is synced during checkpoint for persistence
    #[deprecated(since = "2.0.0", note = "Use DatabaseStorage::get_paged_table() instead")]
    pub rows: Vec<Row>,
    /// Sequence counters for SERIAL columns: `column_name` -> `next_value`
    pub sequences: HashMap<String, i64>,
    // Note: PagedTable cannot be stored here because:
    // 1. Arc<Mutex<PageManager>> is not serializable
    // 2. PagedTable is managed externally by Database
    // When using PageBased mode, rows Vec is kept in sync for serialization
}

impl Table {
    #[must_use] 
    pub fn new(name: String, columns: Vec<Column>) -> Self {
        let mut sequences = HashMap::new();

        // Initialize sequences for SERIAL and BIGSERIAL columns
        for col in &columns {
            if matches!(col.data_type, DataType::Serial | DataType::BigSerial) {
                sequences.insert(col.name.clone(), 1);
            }
        }

        Self {
            name,
            columns,
            rows: Vec::new(),
            sequences,
        }
    }

    pub fn insert(&mut self, row: Row) -> Result<(), DatabaseError> {
        if row.values.len() != self.columns.len() {
            return Err(DatabaseError::ColumnCountMismatch);
        }
        self.rows.push(row);
        Ok(())
    }

    #[must_use] 
    pub fn get_column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}
