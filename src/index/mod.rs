/// Index structures for fast data access
///
/// Currently implements B-tree indexes for equality lookups.
/// Future: Hash indexes, bitmap indexes, GiST, etc.

pub mod btree;

pub use btree::BTreeIndex;
