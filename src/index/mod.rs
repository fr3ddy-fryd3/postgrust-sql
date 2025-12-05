/// Index structures for fast data access
///
/// Implements B-tree and Hash indexes.
/// Future: bitmap indexes, GiST, GIN, etc.

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
        IndexType::BTree
    }
}

impl IndexType {
    pub fn as_str(&self) -> &'static str {
        match self {
            IndexType::BTree => "btree",
            IndexType::Hash => "hash",
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
    pub fn name(&self) -> &str {
        match self {
            Index::BTree(idx) => &idx.name,
            Index::Hash(idx) => &idx.name,
        }
    }

    pub fn table_name(&self) -> &str {
        match self {
            Index::BTree(idx) => &idx.table_name,
            Index::Hash(idx) => &idx.table_name,
        }
    }

    pub fn column_name(&self) -> &str {
        match self {
            Index::BTree(idx) => &idx.column_name,
            Index::Hash(idx) => &idx.column_name,
        }
    }

    pub fn is_unique(&self) -> bool {
        match self {
            Index::BTree(idx) => idx.is_unique,
            Index::Hash(idx) => idx.is_unique,
        }
    }

    pub fn index_type(&self) -> IndexType {
        match self {
            Index::BTree(_) => IndexType::BTree,
            Index::Hash(_) => IndexType::Hash,
        }
    }

    pub fn insert(&mut self, value: &crate::types::Value, row_index: usize) -> Result<(), crate::types::DatabaseError> {
        match self {
            Index::BTree(idx) => idx.insert(value, row_index),
            Index::Hash(idx) => idx.insert(value, row_index),
        }
    }

    pub fn delete(&mut self, value: &crate::types::Value, row_index: usize) {
        match self {
            Index::BTree(idx) => idx.delete(value, row_index),
            Index::Hash(idx) => idx.delete(value, row_index),
        }
    }

    pub fn search(&self, value: &crate::types::Value) -> Vec<usize> {
        match self {
            Index::BTree(idx) => idx.search(value),
            Index::Hash(idx) => idx.search(value),
        }
    }
}
