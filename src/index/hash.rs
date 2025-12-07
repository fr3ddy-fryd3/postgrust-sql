/// Hash index implementation for O(1) equality lookups
///
/// Hash indexes are optimized for equality (=) conditions only.
/// They provide O(1) average-case performance vs O(log n) for B-tree.
///
/// Limitations:
/// - No range queries (>, <, BETWEEN)
/// - No ordering (cannot be used for ORDER BY)
/// - Higher memory usage than B-tree for small datasets

use crate::types::{DatabaseError, Value};
use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Hash index using HashMap for O(1) lookups
/// Supports single and composite keys (v1.9.0)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HashIndex {
    pub name: String,
    pub table_name: String,
    pub column_names: Vec<String>,  // v1.9.0: supports composite
    pub is_unique: bool,
    /// Maps value hash → row indices
    /// For non-unique: multiple rows can have same value
    #[serde(skip)]
    map: HashMap<IndexKey, Vec<usize>>,
}

// Backward compatibility
impl HashIndex {
    /// Get first column name (for backward compatibility)
    pub fn column_name(&self) -> &str {
        &self.column_names[0]
    }

    /// Check if this is a composite index
    pub fn is_composite(&self) -> bool {
        self.column_names.len() > 1
    }
}

/// Wrapper for Value to implement Hash + Eq
/// Supports composite keys (v1.9.0)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IndexKey(String);

impl IndexKey {
    /// Create key from single value
    fn from_value(value: &Value) -> Self {
        IndexKey(value.to_string())
    }

    /// Create composite key from multiple values (v1.9.0)
    fn from_values(values: &[Value]) -> Self {
        let parts: Vec<String> = values.iter().map(|v| v.to_string()).collect();
        IndexKey(parts.join("||"))
    }
}

// Backward compatibility
impl From<&Value> for IndexKey {
    fn from(value: &Value) -> Self {
        Self::from_value(value)
    }
}

impl HashIndex {
    /// Create a new hash index (single column)
    pub fn new(name: String, table_name: String, column_name: String, is_unique: bool) -> Self {
        Self {
            name,
            table_name,
            column_names: vec![column_name],
            is_unique,
            map: HashMap::new(),
        }
    }

    /// Create a new composite hash index (v1.9.0)
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
            map: HashMap::new(),
        }
    }

    /// Insert a value into the index
    ///
    /// Returns error if UNIQUE constraint violated
    pub fn insert(&mut self, value: &Value, row_index: usize) -> Result<(), DatabaseError> {
        let key = IndexKey::from(value);

        // Check unique constraint
        if self.is_unique && self.map.contains_key(&key) {
            return Err(DatabaseError::UniqueViolation(format!(
                "Duplicate key value violates unique constraint \"{}\"",
                self.name
            )));
        }

        // Insert into hash map
        self.map.entry(key).or_insert_with(Vec::new).push(row_index);

        Ok(())
    }

    /// Delete a value from the index
    pub fn delete(&mut self, value: &Value, row_index: usize) {
        let key = IndexKey::from(value);

        if let Some(indices) = self.map.get_mut(&key) {
            indices.retain(|&idx| idx != row_index);
            // Remove entry if no more rows
            if indices.is_empty() {
                self.map.remove(&key);
            }
        }
    }

    /// Search for a value in the index - O(1) average case
    ///
    /// Returns list of row indices that match the value
    pub fn search(&self, value: &Value) -> Vec<usize> {
        let key = IndexKey::from(value);
        self.map.get(&key).cloned().unwrap_or_default()
    }

    /// Get number of unique keys in index
    pub fn key_count(&self) -> usize {
        self.map.len()
    }

    /// Get total number of entries (including duplicates for non-unique)
    pub fn entry_count(&self) -> usize {
        self.map.values().map(|v| v.len()).sum()
    }

    // === Composite index methods (v1.9.0) ===

    /// Insert composite key into index - O(1) average case
    pub fn insert_composite(&mut self, values: &[Value], row_index: usize) -> Result<(), DatabaseError> {
        if values.len() != self.column_names.len() {
            return Err(DatabaseError::ParseError(
                format!("Expected {} values for composite index, got {}",
                    self.column_names.len(), values.len())
            ));
        }

        let key = IndexKey::from_values(values);

        // Check unique constraint
        if self.is_unique && self.map.contains_key(&key) {
            return Err(DatabaseError::UniqueViolation(format!(
                "Duplicate key value violates unique constraint \"{}\"",
                self.name
            )));
        }

        // Insert into hash map
        self.map.entry(key).or_insert_with(Vec::new).push(row_index);
        Ok(())
    }

    /// Delete composite key from index
    pub fn delete_composite(&mut self, values: &[Value], row_index: usize) {
        if values.len() != self.column_names.len() {
            return;
        }

        let key = IndexKey::from_values(values);

        if let Some(indices) = self.map.get_mut(&key) {
            indices.retain(|&idx| idx != row_index);
            if indices.is_empty() {
                self.map.remove(&key);
            }
        }
    }

    /// Search for rows with composite key match - O(1) average case
    pub fn search_composite(&self, values: &[Value]) -> Vec<usize> {
        if values.len() != self.column_names.len() {
            return Vec::new();
        }

        let key = IndexKey::from_values(values);
        self.map.get(&key).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_index_insert_and_search() {
        let mut index = HashIndex::new(
            "idx_test".to_string(),
            "users".to_string(),
            "name".to_string(),
            false,
        );

        // Insert values
        index.insert(&Value::Text("Alice".to_string()), 0).unwrap();
        index.insert(&Value::Text("Bob".to_string()), 1).unwrap();
        index.insert(&Value::Text("Alice".to_string()), 2).unwrap(); // duplicate

        // Search
        assert_eq!(index.search(&Value::Text("Alice".to_string())), vec![0, 2]);
        assert_eq!(index.search(&Value::Text("Bob".to_string())), vec![1]);
        assert_eq!(index.search(&Value::Text("Charlie".to_string())), Vec::<usize>::new());
    }

    #[test]
    fn test_hash_index_unique_constraint() {
        let mut index = HashIndex::new(
            "idx_unique".to_string(),
            "users".to_string(),
            "email".to_string(),
            true,
        );

        // First insert succeeds
        index.insert(&Value::Text("alice@example.com".to_string()), 0).unwrap();

        // Duplicate insert fails
        let result = index.insert(&Value::Text("alice@example.com".to_string()), 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_hash_index_delete() {
        let mut index = HashIndex::new(
            "idx_test".to_string(),
            "users".to_string(),
            "name".to_string(),
            false,
        );

        index.insert(&Value::Text("Alice".to_string()), 0).unwrap();
        index.insert(&Value::Text("Alice".to_string()), 1).unwrap();

        // Delete one entry
        index.delete(&Value::Text("Alice".to_string()), 0);
        assert_eq!(index.search(&Value::Text("Alice".to_string())), vec![1]);

        // Delete last entry
        index.delete(&Value::Text("Alice".to_string()), 1);
        assert_eq!(index.search(&Value::Text("Alice".to_string())), Vec::<usize>::new());
    }

    #[test]
    fn test_hash_index_integer_values() {
        let mut index = HashIndex::new(
            "idx_age".to_string(),
            "users".to_string(),
            "age".to_string(),
            false,
        );

        index.insert(&Value::Integer(25), 0).unwrap();
        index.insert(&Value::Integer(30), 1).unwrap();
        index.insert(&Value::Integer(25), 2).unwrap();

        assert_eq!(index.search(&Value::Integer(25)), vec![0, 2]);
        assert_eq!(index.search(&Value::Integer(30)), vec![1]);
    }

    #[test]
    fn test_hash_index_counts() {
        let mut index = HashIndex::new(
            "idx_test".to_string(),
            "users".to_string(),
            "category".to_string(),
            false,
        );

        index.insert(&Value::Text("A".to_string()), 0).unwrap();
        index.insert(&Value::Text("B".to_string()), 1).unwrap();
        index.insert(&Value::Text("A".to_string()), 2).unwrap();

        assert_eq!(index.key_count(), 2); // 2 unique keys: A, B
        assert_eq!(index.entry_count(), 3); // 3 total entries
    }
}
