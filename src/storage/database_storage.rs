use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::path::Path;
use crate::types::{DatabaseError, Row};
use super::page_manager::PageManager;
use super::paged_table::PagedTable;

/// `DatabaseStorage` - manages page-based storage for all tables in a database
pub struct DatabaseStorage {
    /// Page manager (shared across all tables)
    page_manager: Arc<Mutex<PageManager>>,
    /// `PagedTable` instances: `table_name` -> (`table_id`, `PagedTable`)
    paged_tables: HashMap<String, (u32, PagedTable)>,
    /// Next available table ID
    next_table_id: u32,
}

impl DatabaseStorage {
    /// Create new database storage
    pub fn new<P: AsRef<Path>>(data_dir: P, buffer_pool_size: usize) -> Result<Self, DatabaseError> {
        let page_manager = Arc::new(Mutex::new(PageManager::new(data_dir, buffer_pool_size)?));

        Ok(Self {
            page_manager,
            paged_tables: HashMap::new(),
            next_table_id: 1,
        })
    }

    /// Create a new paged table
    pub fn create_table(&mut self, table_name: String) -> Result<(), DatabaseError> {
        if self.paged_tables.contains_key(&table_name) {
            return Err(DatabaseError::TableAlreadyExists(table_name));
        }

        let table_id = self.next_table_id;
        self.next_table_id += 1;

        let paged_table = PagedTable::new(table_id, self.page_manager.clone());
        self.paged_tables.insert(table_name, (table_id, paged_table));

        Ok(())
    }

    /// Drop a paged table
    pub fn drop_table(&mut self, table_name: &str) -> Result<(), DatabaseError> {
        if let Some((table_id, _)) = self.paged_tables.remove(table_name) {
            // Delete all pages for this table
            let pm = self.page_manager.lock().unwrap();
            pm.delete_table_pages(table_id)?;
            Ok(())
        } else {
            Err(DatabaseError::TableNotFound(table_name.to_string()))
        }
    }

    /// Get mutable reference to a paged table
    pub fn get_paged_table_mut(&mut self, table_name: &str) -> Option<&mut PagedTable> {
        self.paged_tables.get_mut(table_name).map(|(_, pt)| pt)
    }

    /// Get reference to a paged table
    #[must_use] 
    pub fn get_paged_table(&self, table_name: &str) -> Option<&PagedTable> {
        self.paged_tables.get(table_name).map(|(_, pt)| pt)
    }

    /// Insert a row into a paged table
    pub fn insert(&mut self, table_name: &str, row: Row) -> Result<(), DatabaseError> {
        let paged_table = self.get_paged_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;
        paged_table.insert(row)
    }

    /// Get all rows from a paged table
    pub fn get_all_rows(&self, table_name: &str) -> Result<Vec<Row>, DatabaseError> {
        let paged_table = self.get_paged_table(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;
        paged_table.get_all_rows()
    }

    /// Delete rows matching predicate (MVCC-aware)
    pub fn delete_where<F>(&mut self, table_name: &str, predicate: F, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool,
    {
        let paged_table = self.get_paged_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;
        paged_table.delete_where(predicate, tx_id)
    }

    /// Update rows matching predicate (MVCC-aware)
    pub fn update_where<F, U>(&mut self, table_name: &str, predicate: F, updater: U, tx_id: u64) -> Result<usize, DatabaseError>
    where
        F: Fn(&Row) -> bool,
        U: Fn(&Row) -> Row,
    {
        let paged_table = self.get_paged_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;
        paged_table.update_where(predicate, updater, tx_id)
    }

    /// Flush all dirty pages to disk (checkpoint)
    pub fn checkpoint(&self) -> Result<usize, DatabaseError> {
        let pm = self.page_manager.lock().unwrap();
        pm.checkpoint()
    }

    /// Get statistics for a table
    #[must_use] 
    pub fn get_table_stats(&self, table_name: &str) -> Option<super::paged_table::PagedTableStats> {
        self.get_paged_table(table_name).map(super::paged_table::PagedTable::stats)
    }

    /// Get list of all paged tables
    #[must_use] 
    pub fn list_tables(&self) -> Vec<String> {
        self.paged_tables.keys().cloned().collect()
    }

    /// Get row count for a table
    #[must_use] 
    pub fn row_count(&self, table_name: &str) -> Option<usize> {
        self.get_paged_table(table_name).map(super::paged_table::PagedTable::row_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;
    use tempfile::TempDir;

    #[test]
    fn test_database_storage_creation() {
        let temp_dir = TempDir::new().unwrap();
        let storage = DatabaseStorage::new(temp_dir.path(), 100).unwrap();
        assert_eq!(storage.list_tables().len(), 0);
    }

    #[test]
    fn test_create_and_drop_table() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path(), 100).unwrap();

        storage.create_table("users".to_string()).unwrap();
        assert_eq!(storage.list_tables().len(), 1);

        storage.drop_table("users").unwrap();
        assert_eq!(storage.list_tables().len(), 0);
    }

    #[test]
    fn test_insert_and_get_rows() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path(), 100).unwrap();

        storage.create_table("users".to_string()).unwrap();

        let row = Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]);
        storage.insert("users", row).unwrap();

        let rows = storage.get_all_rows("users").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(storage.row_count("users"), Some(1));
    }

    #[test]
    fn test_delete_where() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path(), 100).unwrap();

        storage.create_table("users".to_string()).unwrap();

        for i in 0..10 {
            let row = Row::new(vec![Value::Integer(i)]);
            storage.insert("users", row).unwrap();
        }

        let deleted = storage.delete_where("users", |row| {
            if let Value::Integer(val) = row.values[0] {
                val > 5
            } else {
                false
            }
        }, 100 /* tx_id */).unwrap();

        assert_eq!(deleted, 4);
        // MVCC: rows are marked, not physically removed
        assert_eq!(storage.row_count("users"), Some(10)); // All rows still present
    }

    #[test]
    fn test_update_where() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path(), 100).unwrap();

        storage.create_table("users".to_string()).unwrap();

        for i in 0..5 {
            let row = Row::new(vec![Value::Integer(i), Value::Text("old".to_string())]);
            storage.insert("users", row).unwrap();
        }

        let updated = storage.update_where(
            "users",
            |_| true,
            |row| Row::new(vec![row.values[0].clone(), Value::Text("new".to_string())]),
            100 // tx_id
        ).unwrap();

        assert_eq!(updated, 5);
    }

    #[test]
    fn test_checkpoint() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path(), 100).unwrap();

        storage.create_table("users".to_string()).unwrap();

        for i in 0..10 {
            let row = Row::new(vec![Value::Integer(i)]);
            storage.insert("users", row).unwrap();
        }

        let pages_flushed = storage.checkpoint().unwrap();
        assert!(pages_flushed > 0);
    }
}
