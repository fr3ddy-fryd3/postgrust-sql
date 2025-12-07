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

/// B-tree index for single or multiple columns (v1.9.0)
///
/// Current implementation uses Rust's BTreeMap as foundation.
/// Maps column value(s) to row indices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BTreeIndex {
    /// Name of this index
    pub name: String,

    /// Table this index belongs to
    pub table_name: String,

    /// Column(s) being indexed (single or composite)
    pub column_names: Vec<String>,

    /// Is this a unique index?
    pub is_unique: bool,

    /// The actual index: Value(s) -> Vec<row_index>
    /// Vec allows multiple rows with same value (non-unique indexes)
    tree: BTreeMap<IndexKey, Vec<usize>>,
}

// Keep backward compatibility property
impl BTreeIndex {
    /// Get first column name (for backward compatibility with single-column APIs)
    pub fn column_name(&self) -> &str {
        &self.column_names[0]
    }

    /// Check if this is a composite index
    pub fn is_composite(&self) -> bool {
        self.column_names.len() > 1
    }
}

/// Wrapper for Value(s) to make it sortable in BTreeMap
/// Supports both single and composite keys (v1.9.0)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct IndexKey(String);

impl IndexKey {
    /// Create key from single value
    fn from_value(value: &Value) -> Self {
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

    /// Create composite key from multiple values (v1.9.0)
    fn from_values(values: &[Value]) -> Self {
        let parts: Vec<String> = values.iter().map(|v| {
            Self::from_value(v).0
        }).collect();
        IndexKey(parts.join("||"))  // Use || as separator
    }
}

// Keep backward compatibility
impl From<&Value> for IndexKey {
    fn from(value: &Value) -> Self {
        Self::from_value(value)
    }
}

impl BTreeIndex {
    /// Create a new B-tree index (single column)
    pub fn new(
        name: String,
        table_name: String,
        column_name: String,
        is_unique: bool,
    ) -> Self {
        Self {
            name,
            table_name,
            column_names: vec![column_name],
            is_unique,
            tree: BTreeMap::new(),
        }
    }

    /// Create a new composite B-tree index (v1.9.0)
    pub fn new_composite(
        name: String,
        table_name: String,
        column_names: Vec<String>,
        is_unique: bool,
    ) -> Self {
        Self {
            name,
            table_name,
            column_names,
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

    // === Composite index methods (v1.9.0) ===

    /// Insert composite key into index
    pub fn insert_composite(&mut self, values: &[Value], row_index: usize) -> Result<(), DatabaseError> {
        if values.len() != self.column_names.len() {
            return Err(DatabaseError::ParseError(
                format!("Expected {} values for composite index, got {}",
                    self.column_names.len(), values.len())
            ));
        }

        let key = IndexKey::from_values(values);

        if self.is_unique && self.tree.contains_key(&key) {
            return Err(DatabaseError::UniqueViolation(
                format!("Duplicate key value violates unique constraint '{}'", self.name)
            ));
        }

        self.tree.entry(key).or_insert_with(Vec::new).push(row_index);
        Ok(())
    }

    /// Delete composite key from index
    pub fn delete_composite(&mut self, values: &[Value], row_index: usize) {
        if values.len() != self.column_names.len() {
            return; // Ignore mismatched values
        }

        let key = IndexKey::from_values(values);

        if let Some(indices) = self.tree.get_mut(&key) {
            indices.retain(|&idx| idx != row_index);
            if indices.is_empty() {
                self.tree.remove(&key);
            }
        }
    }

    /// Search for rows with composite key match
    pub fn search_composite(&self, values: &[Value]) -> Vec<usize> {
        if values.len() != self.column_names.len() {
            return Vec::new();
        }

        let key = IndexKey::from_values(values);
        self.tree.get(&key).cloned().unwrap_or_default()
    }

    /// Search with prefix match (for composite indexes)
    /// E.g., for index on (city, age), can search just by city
    pub fn search_prefix(&self, values: &[Value]) -> Vec<usize> {
        if values.is_empty() || values.len() > self.column_names.len() {
            return Vec::new();
        }

        let prefix_key = IndexKey::from_values(values);
        let prefix_str = &prefix_key.0;

        // Find all keys that start with this prefix
        let mut result = Vec::new();
        for (key, indices) in self.tree.iter() {
            if key.0.starts_with(prefix_str) {
                result.extend_from_slice(indices);
            }
        }
        result
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
