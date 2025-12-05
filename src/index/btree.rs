/// B-tree index for fast equality lookups
///
/// Simplified in-memory B-tree implementation for v1.6.0.
/// Supports INSERT, DELETE, and SEARCH operations.
///
/// Future improvements:
/// - Persistent storage on disk
/// - Range queries (>, <, BETWEEN)
/// - Bulk loading optimization

use crate::types::{Value, DatabaseError};
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

/// B-tree index for single column
///
/// Current implementation uses Rust's BTreeMap as foundation.
/// Maps column values to row indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BTreeIndex {
    /// Name of this index
    pub name: String,

    /// Table this index belongs to
    pub table_name: String,

    /// Column being indexed
    pub column_name: String,

    /// Is this a unique index?
    pub is_unique: bool,

    /// The actual index: Value -> Vec<row_index>
    /// Vec allows multiple rows with same value (non-unique indexes)
    tree: BTreeMap<IndexKey, Vec<usize>>,
}

/// Wrapper for Value to make it sortable in BTreeMap
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct IndexKey(String);

impl From<&Value> for IndexKey {
    fn from(value: &Value) -> Self {
        // Convert Value to sortable string representation
        match value {
            Value::Integer(i) => IndexKey(format!("I{:020}", i)),
            Value::SmallInt(i) => IndexKey(format!("S{:020}", i)),
            Value::Text(s) => IndexKey(format!("T{}", s)),
            Value::Char(s) => IndexKey(format!("C{}", s)),
            Value::Boolean(b) => IndexKey(format!("BOOL{}", b)),
            Value::Real(f) => IndexKey(format!("R{:020.10}", f)),
            Value::Null => IndexKey("NULL".to_string()),
            Value::Uuid(u) => IndexKey(format!("UUID{}", u)),
            // Add more types as needed
            _ => IndexKey(format!("{:?}", value)),
        }
    }
}

impl BTreeIndex {
    /// Create a new B-tree index
    pub fn new(
        name: String,
        table_name: String,
        column_name: String,
        is_unique: bool,
    ) -> Self {
        Self {
            name,
            table_name,
            column_name,
            is_unique,
            tree: BTreeMap::new(),
        }
    }

    /// Insert a value into the index
    ///
    /// For unique indexes, returns error if value already exists.
    /// For non-unique indexes, appends to existing list.
    pub fn insert(&mut self, value: &Value, row_index: usize) -> Result<(), DatabaseError> {
        let key = IndexKey::from(value);

        if self.is_unique && self.tree.contains_key(&key) {
            return Err(DatabaseError::UniqueViolation(
                format!("Duplicate key value violates unique constraint '{}'", self.name)
            ));
        }

        self.tree.entry(key).or_insert_with(Vec::new).push(row_index);
        Ok(())
    }

    /// Remove a value from the index
    pub fn delete(&mut self, value: &Value, row_index: usize) {
        let key = IndexKey::from(value);

        if let Some(indices) = self.tree.get_mut(&key) {
            indices.retain(|&idx| idx != row_index);
            // Remove key if no more rows
            if indices.is_empty() {
                self.tree.remove(&key);
            }
        }
    }

    /// Search for rows with exact value match
    ///
    /// Returns list of row indices that match the value.
    /// Empty vec if not found.
    pub fn search(&self, value: &Value) -> Vec<usize> {
        let key = IndexKey::from(value);
        self.tree.get(&key).cloned().unwrap_or_default()
    }

    /// Check if index contains a value
    pub fn contains(&self, value: &Value) -> bool {
        let key = IndexKey::from(value);
        self.tree.contains_key(&key)
    }

    /// Get number of distinct keys in index
    pub fn key_count(&self) -> usize {
        self.tree.len()
    }

    /// Get total number of entries (including duplicates for non-unique)
    pub fn entry_count(&self) -> usize {
        self.tree.values().map(|v| v.len()).sum()
    }

    /// Clear all entries from index
    pub fn clear(&mut self) {
        self.tree.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_btree_insert_and_search() {
        let mut index = BTreeIndex::new(
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            true,
        );

        // Insert values
        index.insert(&Value::Integer(1), 0).unwrap();
        index.insert(&Value::Integer(2), 1).unwrap();
        index.insert(&Value::Integer(3), 2).unwrap();

        // Search
        assert_eq!(index.search(&Value::Integer(1)), vec![0]);
        assert_eq!(index.search(&Value::Integer(2)), vec![1]);
        assert_eq!(index.search(&Value::Integer(3)), vec![2]);
        assert_eq!(index.search(&Value::Integer(999)), Vec::<usize>::new());
    }

    #[test]
    fn test_btree_unique_constraint() {
        let mut index = BTreeIndex::new(
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            true,
        );

        index.insert(&Value::Integer(1), 0).unwrap();

        // Try to insert duplicate
        let result = index.insert(&Value::Integer(1), 1);
        assert!(result.is_err());
        assert!(matches!(result, Err(DatabaseError::UniqueViolation(_))));
    }

    #[test]
    fn test_btree_non_unique() {
        let mut index = BTreeIndex::new(
            "idx_age".to_string(),
            "users".to_string(),
            "age".to_string(),
            false, // non-unique
        );

        // Multiple rows with same value
        index.insert(&Value::Integer(25), 0).unwrap();
        index.insert(&Value::Integer(25), 1).unwrap();
        index.insert(&Value::Integer(25), 2).unwrap();

        let results = index.search(&Value::Integer(25));
        assert_eq!(results.len(), 3);
        assert!(results.contains(&0));
        assert!(results.contains(&1));
        assert!(results.contains(&2));
    }

    #[test]
    fn test_btree_delete() {
        let mut index = BTreeIndex::new(
            "idx_id".to_string(),
            "users".to_string(),
            "id".to_string(),
            false,
        );

        index.insert(&Value::Integer(1), 0).unwrap();
        index.insert(&Value::Integer(1), 1).unwrap();

        // Delete one entry
        index.delete(&Value::Integer(1), 0);
        assert_eq!(index.search(&Value::Integer(1)), vec![1]);

        // Delete last entry - key should be removed
        index.delete(&Value::Integer(1), 1);
        assert_eq!(index.search(&Value::Integer(1)), Vec::<usize>::new());
        assert_eq!(index.key_count(), 0);
    }

    #[test]
    fn test_btree_text_values() {
        let mut index = BTreeIndex::new(
            "idx_name".to_string(),
            "users".to_string(),
            "name".to_string(),
            false,
        );

        index.insert(&Value::Text("Alice".to_string()), 0).unwrap();
        index.insert(&Value::Text("Bob".to_string()), 1).unwrap();
        index.insert(&Value::Text("Charlie".to_string()), 2).unwrap();

        assert_eq!(index.search(&Value::Text("Alice".to_string())), vec![0]);
        assert_eq!(index.search(&Value::Text("Bob".to_string())), vec![1]);
        assert_eq!(index.search(&Value::Text("Dave".to_string())), Vec::<usize>::new());
    }

    #[test]
    fn test_btree_counts() {
        let mut index = BTreeIndex::new(
            "idx_age".to_string(),
            "users".to_string(),
            "age".to_string(),
            false,
        );

        index.insert(&Value::Integer(25), 0).unwrap();
        index.insert(&Value::Integer(25), 1).unwrap();
        index.insert(&Value::Integer(30), 2).unwrap();

        assert_eq!(index.key_count(), 2); // Two distinct keys: 25, 30
        assert_eq!(index.entry_count(), 3); // Three total entries
    }
}
