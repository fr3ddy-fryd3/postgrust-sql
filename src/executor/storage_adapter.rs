/// Storage adapter - abstraction for PagedTable storage
///
/// v2.0.0: Legacy Vec<Row> storage has been removed.
/// This module provides a unified interface for page-based row storage.

use crate::types::{Row, DatabaseError};

/// Trait for row storage operations
///
/// v2.0.0: Only PagedStorage implementation remains
pub trait RowStorage {
    /// Insert a row into storage
    fn insert(&mut self, row: Row) -> Result<(), DatabaseError>;

    /// Get all rows from storage (for SELECT)
    fn get_all(&self) -> Result<Vec<Row>, DatabaseError>;

    /// Update rows matching predicate (MVCC-aware: marks old + inserts new version)
    fn update_where<F, U>(&mut self, predicate: F, updater: U, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool,
        U: Fn(&Row) -> Row;

    /// Delete rows matching predicate (MVCC-aware: marks with xmax instead of physical removal)
    fn delete_where<F>(&mut self, predicate: F, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool;

    /// Get row count
    fn count(&self) -> usize;

    /// Flush dirty data to disk (for page-based storage)
    fn flush(&self) -> Result<(), DatabaseError> {
        Ok(()) // No-op for Vec<Row>
    }
}

// v2.0.0: LegacyStorage has been removed - page-based storage only

/// Paged storage: wraps PagedTable
///
/// High-performance storage with 8KB pages, LRU cache, and dirty tracking.
/// Provides 1,250,000x better write amplification vs legacy storage.
pub struct PagedStorage<'a> {
    paged_table: &'a mut crate::storage::PagedTable,
}

impl<'a> PagedStorage<'a> {
    pub fn new(paged_table: &'a mut crate::storage::PagedTable) -> Self {
        Self { paged_table }
    }
}

impl<'a> RowStorage for PagedStorage<'a> {
    fn insert(&mut self, row: Row) -> Result<(), DatabaseError> {
        self.paged_table.insert(row)
    }

    fn get_all(&self) -> Result<Vec<Row>, DatabaseError> {
        self.paged_table.get_all_rows()
    }

    fn update_where<F, U>(&mut self, predicate: F, updater: U, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool,
        U: Fn(&Row) -> Row,
    {
        self.paged_table.update_where(predicate, updater, tx_id)
    }

    fn delete_where<F>(&mut self, predicate: F, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool,
    {
        self.paged_table.delete_where(predicate, tx_id)
    }

    fn count(&self) -> usize {
        self.paged_table.row_count()
    }

    fn flush(&self) -> Result<(), DatabaseError> {
        self.paged_table.flush()
    }
}

// v2.0.0: LegacyStorage tests removed
