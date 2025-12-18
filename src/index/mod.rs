/// Index structures for fast data access
///
/// Implements B-tree and Hash indexes.
/// Future: bitmap indexes, `GiST`, GIN, etc.
pub mod btree;
pub mod hash;

pub use btree::BTreeIndex;
pub use hash::HashIndex;

use serde::{Deserialize, Serialize};

/// Index type: B-tree or Hash
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree index: supports range queries, ordering, O(log n)
    BTree,
    /// Hash index: equality only, O(1) average case
    Hash,
}

impl Default for IndexType {
    fn default() -> Self {
        Self::BTree
    }
}

impl IndexType {
    #[must_use] 
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::BTree => "btree",
            Self::Hash => "hash",
        }
    }
}

/// Unified index wrapper for different index types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Index {
    BTree(BTreeIndex),
    Hash(HashIndex),
}

impl Index {
    #[must_use] 
    pub fn name(&self) -> &str {
        match self {
            Self::BTree(idx) => &idx.name,
            Self::Hash(idx) => &idx.name,
        }
    }

    #[must_use] 
    pub fn table_name(&self) -> &str {
        match self {
            Self::BTree(idx) => &idx.table_name,
            Self::Hash(idx) => &idx.table_name,
        }
    }

    #[must_use] 
    pub fn column_name(&self) -> &str {
        match self {
            Self::BTree(idx) => idx.column_name(),
            Self::Hash(idx) => idx.column_name(),
        }
    }

    #[must_use] 
    pub const fn is_unique(&self) -> bool {
        match self {
            Self::BTree(idx) => idx.is_unique,
            Self::Hash(idx) => idx.is_unique,
        }
    }

    #[must_use] 
    pub const fn index_type(&self) -> IndexType {
        match self {
            Self::BTree(_) => IndexType::BTree,
            Self::Hash(_) => IndexType::Hash,
        }
    }

    pub fn insert(&mut self, value: &crate::types::Value, row_index: usize) -> Result<(), crate::types::DatabaseError> {
        match self {
            Self::BTree(idx) => idx.insert(value, row_index),
            Self::Hash(idx) => idx.insert(value, row_index),
        }
    }

    pub fn delete(&mut self, value: &crate::types::Value, row_index: usize) {
        match self {
            Self::BTree(idx) => idx.delete(value, row_index),
            Self::Hash(idx) => idx.delete(value, row_index),
        }
    }

    #[must_use] 
    pub fn search(&self, value: &crate::types::Value) -> Vec<usize> {
        match self {
            Self::BTree(idx) => idx.search(value),
            Self::Hash(idx) => idx.search(value),
        }
    }

    // === Composite index methods (v1.9.0) ===

    #[must_use] 
    pub fn column_names(&self) -> &[String] {
        match self {
            Self::BTree(idx) => &idx.column_names,
            Self::Hash(idx) => &idx.column_names,
        }
    }

    #[must_use] 
    pub const fn is_composite(&self) -> bool {
        match self {
            Self::BTree(idx) => idx.is_composite(),
            Self::Hash(idx) => idx.is_composite(),
        }
    }

    pub fn insert_composite(&mut self, values: &[crate::types::Value], row_index: usize) -> Result<(), crate::types::DatabaseError> {
        match self {
            Self::BTree(idx) => idx.insert_composite(values, row_index),
            Self::Hash(idx) => idx.insert_composite(values, row_index),
        }
    }

    pub fn delete_composite(&mut self, values: &[crate::types::Value], row_index: usize) {
        match self {
            Self::BTree(idx) => idx.delete_composite(values, row_index),
            Self::Hash(idx) => idx.delete_composite(values, row_index),
        }
    }

    #[must_use] 
    pub fn search_composite(&self, values: &[crate::types::Value]) -> Vec<usize> {
        match self {
            Self::BTree(idx) => idx.search_composite(values),
            Self::Hash(idx) => idx.search_composite(values),
        }
    }

    /// Search with prefix match (only for B-tree composite indexes)
    #[must_use] 
    pub fn search_prefix(&self, values: &[crate::types::Value]) -> Option<Vec<usize>> {
        match self {
            Self::BTree(idx) if idx.is_composite() => Some(idx.search_prefix(values)),
            _ => None,  // Hash indexes don't support prefix search
        }
    }
}
