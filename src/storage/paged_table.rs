use std::sync::{Arc, Mutex};
use crate::types::{DatabaseError, Row};
use super::page_manager::PageManager;
use super::page::PageId;

/// `PagedTable` - table storage using page-based architecture
pub struct PagedTable {
    /// Table ID (unique identifier)
    pub table_id: u32,
    /// Page manager for disk I/O
    page_manager: Arc<Mutex<PageManager>>,
    /// Number of pages currently allocated
    page_count: u32,
    /// Total row count (cached)
    row_count: usize,
}

impl PagedTable {
    /// Create a new paged table
    pub const fn new(table_id: u32, page_manager: Arc<Mutex<PageManager>>) -> Self {
        Self {
            table_id,
            page_manager,
            page_count: 0,
            row_count: 0,
        }
    }

    /// Insert a row into the table
    pub fn insert(&mut self, row: Row) -> Result<(), DatabaseError> {
        // Try to find a page with free space
        let mut inserted = false;

        for page_num in 0..self.page_count {
            let page_id = PageId::new(self.table_id, page_num);

            // Try to insert into this page
            let pm = self.page_manager.lock().unwrap();
            let guard = pm.get_page_mut(page_id)?;

            let result = guard.get_mut(|page| {
                if page.can_fit(bincode::serialize(&row).unwrap().len()) {
                    page.insert_row(&row)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            })?;

            drop(guard);
            drop(pm);

            if result {
                inserted = true;
                self.row_count += 1;
                break;
            }
        }

        // If not inserted, create a new page
        if !inserted {
            let new_page_id = {
                let pm = self.page_manager.lock().unwrap();
                pm.create_page(self.table_id, self.page_count)?
            };

            let pm = self.page_manager.lock().unwrap();
            let guard = pm.get_page_mut(new_page_id)?;
            guard.get_mut(|page| page.insert_row(&row))?;
            drop(guard);
            drop(pm);

            self.page_count += 1;
            self.row_count += 1;
        }

        Ok(())
    }

    /// Get all rows from the table
    pub fn get_all_rows(&self) -> Result<Vec<Row>, DatabaseError> {
        let mut all_rows = Vec::new();

        let pm = self.page_manager.lock().unwrap();

        for page_num in 0..self.page_count {
            let page_id = PageId::new(self.table_id, page_num);
            let page = pm.get_page(page_id)?;
            all_rows.extend(page.get_all_rows());
        }

        Ok(all_rows)
    }

    /// Get row count
    #[must_use] 
    pub const fn row_count(&self) -> usize {
        self.row_count
    }

    /// Delete rows matching predicate (MVCC-aware: marks with xmax instead of physical removal)
    pub fn delete_where<F>(&mut self, predicate: F, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool,
    {
        let mut deleted_count = 0;

        let pm = self.page_manager.lock().unwrap();

        for page_num in 0..self.page_count {
            let page_id = PageId::new(self.table_id, page_num);
            let guard = pm.get_page_mut(page_id)?;

            let count = guard.get_mut(|page| {
                let mut local_count = 0;
                for slot_idx in 0..page.slots.len() {
                    if let Ok(mut row) = page.get_row(slot_idx as u16)
                        && predicate(&row) {
                            // MVCC: mark row as deleted instead of physical removal
                            row.mark_deleted(tx_id);
                            page.update_row(slot_idx as u16, &row)?;
                            local_count += 1;
                        }
                }
                Ok(local_count)
            })?;

            deleted_count += count;
        }

        // Note: row_count stays the same (rows are marked, not removed)
        // VACUUM will physically remove them later
        Ok(deleted_count)
    }

    /// Update rows matching predicate (MVCC-aware: marks old + inserts new versions)
    pub fn update_where<F, U>(&mut self, predicate: F, updater: U, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool,
        U: Fn(&Row) -> Row,
    {
        let pm = self.page_manager.lock().unwrap();
        let mut new_rows = Vec::new();
        let mut updated_count = 0;

        // Phase 1: Mark old rows and collect new versions
        for page_num in 0..self.page_count {
            let page_id = PageId::new(self.table_id, page_num);
            let guard = pm.get_page_mut(page_id)?;

            guard.get_mut(|page| {
                for slot_idx in 0..page.slots.len() {
                    if let Ok(mut row) = page.get_row(slot_idx as u16)
                        && predicate(&row) {
                            // Mark old version as deleted
                            row.mark_deleted(tx_id);
                            page.update_row(slot_idx as u16, &row)?;

                            // Create new version
                            let mut new_row = updater(&row);
                            new_row.xmin = tx_id;
                            new_row.xmax = None;
                            new_rows.push(new_row);
                            updated_count += 1;
                        }
                }
                Ok(())
            })?;
        }

        // Phase 2: Insert new versions (drop lock first to avoid deadlock)
        drop(pm);
        for new_row in new_rows {
            self.insert(new_row)?;
        }

        Ok(updated_count)
    }

    /// Flush all dirty pages to disk
    pub fn flush(&self) -> Result<(), DatabaseError> {
        let pm = self.page_manager.lock().unwrap();
        pm.checkpoint()?;
        Ok(())
    }

    /// VACUUM - physically remove dead tuples (rows with xmax < oldest_tx)
    ///
    /// Scans all pages and deletes slots containing dead rows that are no longer
    /// visible to any active transaction.
    ///
    /// # Arguments
    /// * `oldest_tx` - Cleanup horizon: only remove rows with xmax < oldest_tx
    ///
    /// # Returns
    /// Number of tuples removed
    pub fn vacuum(&mut self, oldest_tx: u64) -> Result<usize, DatabaseError> {
        let mut removed_count = 0;
        let page_manager = self.page_manager.lock().unwrap();

        // Iterate through all pages
        for page_num in 0..self.page_count {
            let page_id = PageId::new(self.table_id, page_num);
            let guard = page_manager.get_page_mut(page_id)?;

            // Scan all slots in this page
            let count = guard.get_mut(|page| {
                let mut local_removed = 0;

                // Collect indices of dead rows (iterate backwards to avoid index issues)
                let mut dead_slots: Vec<u16> = Vec::new();

                for slot_idx in 0..page.slots.len() {
                    if let Ok(row) = page.get_row(slot_idx as u16) {
                        // Check if row is dead (has xmax and xmax < oldest_tx)
                        if row.is_dead(oldest_tx) {
                            dead_slots.push(slot_idx as u16);
                        }
                    }
                }

                // Physically delete dead rows
                for slot_idx in dead_slots.iter().rev() {
                    page.delete_row(*slot_idx)?;
                    local_removed += 1;
                }

                Ok(local_removed)
            })?;

            removed_count += count;
        }

        Ok(removed_count)
    }

    /// Get statistics
    #[must_use]
    pub const fn stats(&self) -> PagedTableStats {
        PagedTableStats {
            table_id: self.table_id,
            page_count: self.page_count,
            row_count: self.row_count,
        }
    }
}

/// Statistics for a paged table
#[derive(Debug, Clone)]
pub struct PagedTableStats {
    pub table_id: u32,
    pub page_count: u32,
    pub row_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;
    use tempfile::TempDir;

    #[test]
    fn test_paged_table_creation() {
        let temp_dir = TempDir::new().unwrap();
        let pm = Arc::new(Mutex::new(PageManager::new(temp_dir.path(), 100).unwrap()));

        let table = PagedTable::new(1, pm);
        assert_eq!(table.table_id, 1);
        assert_eq!(table.page_count, 0);
        assert_eq!(table.row_count, 0);
    }

    #[test]
    fn test_insert_and_get_rows() {
        let temp_dir = TempDir::new().unwrap();
        let pm = Arc::new(Mutex::new(PageManager::new(temp_dir.path(), 100).unwrap()));

        let mut table = PagedTable::new(1, pm);

        // Insert 10 rows
        for i in 0..10 {
            let row = Row::new(vec![Value::Integer(i), Value::Text(format!("User{}", i))]);
            table.insert(row).unwrap();
        }

        assert_eq!(table.row_count(), 10);

        let all_rows = table.get_all_rows().unwrap();
        assert_eq!(all_rows.len(), 10);
    }

    #[test]
    fn test_delete_rows() {
        let temp_dir = TempDir::new().unwrap();
        let pm = Arc::new(Mutex::new(PageManager::new(temp_dir.path(), 100).unwrap()));

        let mut table = PagedTable::new(1, pm);

        // Insert 10 rows
        for i in 0..10 {
            let row = Row::new(vec![Value::Integer(i)]);
            table.insert(row).unwrap();
        }

        // Delete rows where value > 5
        let deleted = table.delete_where(|row| {
            if let Value::Integer(val) = row.values[0] {
                val > 5
            } else {
                false
            }
        }, 100 /* tx_id */).unwrap();

        assert_eq!(deleted, 4);
        // MVCC: rows are marked, not physically removed
        assert_eq!(table.row_count(), 10); // All rows still present
    }

    #[test]
    fn test_update_rows() {
        let temp_dir = TempDir::new().unwrap();
        let pm = Arc::new(Mutex::new(PageManager::new(temp_dir.path(), 100).unwrap()));

        let mut table = PagedTable::new(1, pm);

        // Insert rows
        for i in 0..5 {
            let row = Row::new(vec![Value::Integer(i), Value::Text("old".to_string())]);
            table.insert(row).unwrap();
        }

        // Update all rows
        let updated = table.update_where(
            |_| true,
            |row| Row::new(vec![row.values[0].clone(), Value::Text("new".to_string())]),
            100 // tx_id
        ).unwrap();

        assert_eq!(updated, 5);
    }

    #[test]
    fn test_multiple_pages() {
        let temp_dir = TempDir::new().unwrap();
        let pm = Arc::new(Mutex::new(PageManager::new(temp_dir.path(), 100).unwrap()));

        let mut table = PagedTable::new(1, pm);

        // Insert many rows to force multiple pages
        for i in 0..200 {
            let row = Row::new(vec![
                Value::Integer(i),
                Value::Text(format!("Long text data to fill up pages - row {}", i)),
            ]);
            table.insert(row).unwrap();
        }

        assert!(table.page_count > 1);
        assert_eq!(table.row_count(), 200);

        let all_rows = table.get_all_rows().unwrap();
        assert_eq!(all_rows.len(), 200);
    }

    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();

        // Insert data
        {
            let pm = Arc::new(Mutex::new(PageManager::new(temp_dir.path(), 100).unwrap()));
            let mut table = PagedTable::new(1, pm);

            for i in 0..10 {
                let row = Row::new(vec![Value::Integer(i)]);
                table.insert(row).unwrap();
            }

            table.flush().unwrap();
        }

        // Read data back
        {
            let pm = Arc::new(Mutex::new(PageManager::new(temp_dir.path(), 100).unwrap()));
            let mut table = PagedTable::new(1, pm.clone());

            // Manually set page_count by checking disk
            let page_count = {
                let pm_lock = pm.lock().unwrap();
                pm_lock.get_page_count(1)
            };
            table.page_count = page_count as u32;

            let all_rows = table.get_all_rows().unwrap();
            assert_eq!(all_rows.len(), 10);
        }
    }
}
