use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use super::page::{Page, PageId};
use super::buffer_pool::BufferPool;
use crate::types::{DatabaseError, Row};

/// PageManager - manages disk I/O for pages
pub struct PageManager {
    /// Root data directory
    data_dir: PathBuf,
    /// Buffer pool for caching pages
    buffer_pool: Arc<Mutex<BufferPool>>,
}

impl PageManager {
    /// Create new PageManager
    pub fn new<P: AsRef<Path>>(data_dir: P, buffer_pool_size: usize) -> Result<Self, DatabaseError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        Ok(Self {
            data_dir,
            buffer_pool: Arc::new(Mutex::new(BufferPool::new(buffer_pool_size))),
        })
    }

    /// Get path to page file
    fn get_page_path(&self, page_id: PageId) -> PathBuf {
        let table_dir = self.data_dir.join(format!("table_{}", page_id.table_id));
        fs::create_dir_all(&table_dir).ok();
        table_dir.join(format!("page_{:08}.dat", page_id.page_number))
    }

    /// Read a page from disk
    fn read_page_from_disk(&self, page_id: PageId) -> Result<Page, DatabaseError> {
        let path = self.get_page_path(page_id);

        if !path.exists() {
            // Page doesn't exist - create new empty page
            return Ok(Page::new(page_id));
        }

        let mut file = File::open(&path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        Page::from_bytes(&buffer)
    }

    /// Write a page to disk
    fn write_page_to_disk(&self, page: &Page) -> Result<(), DatabaseError> {
        let path = self.get_page_path(page.header.page_id);

        let bytes = page.to_bytes()?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;

        file.write_all(&bytes)?;
        file.sync_all()?;

        Ok(())
    }

    /// Get a page (from buffer pool or disk)
    pub fn get_page(&self, page_id: PageId) -> Result<Page, DatabaseError> {
        let mut pool = self.buffer_pool.lock().unwrap();

        // Try buffer pool first
        if let Some(page) = pool.get_page(page_id) {
            return Ok(page.clone());
        }

        // Not in buffer pool - read from disk
        drop(pool); // Release lock while reading from disk
        let page = self.read_page_from_disk(page_id)?;

        // Insert into buffer pool
        let mut pool = self.buffer_pool.lock().unwrap();
        if let Some(evicted_page_id) = pool.insert_page(page.clone())? {
            // Need to write evicted page if it's dirty
            if pool.get_dirty_pages().contains(&evicted_page_id) {
                if let Some(evicted_page) = pool.remove_page(evicted_page_id) {
                    drop(pool);
                    self.write_page_to_disk(&evicted_page)?;
                    // Lock will be re-acquired on next iteration or function exit
                }
            }
        }

        Ok(page)
    }

    /// Get a mutable reference to a page (marks as dirty)
    pub fn get_page_mut(&self, page_id: PageId) -> Result<PageMutGuard<'_>, DatabaseError> {
        // Ensure page is in buffer pool
        self.get_page(page_id)?;

        Ok(PageMutGuard {
            page_id,
            page_manager: self,
        })
    }

    /// Flush a specific page to disk
    pub fn flush_page(&self, page_id: PageId) -> Result<(), DatabaseError> {
        let mut pool = self.buffer_pool.lock().unwrap();

        if let Some(page) = pool.get_page(page_id) {
            let page_clone = page.clone();
            pool.clear_dirty(page_id);
            drop(pool);

            self.write_page_to_disk(&page_clone)?;
        }

        Ok(())
    }

    /// Flush all dirty pages to disk (checkpoint)
    pub fn checkpoint(&self) -> Result<usize, DatabaseError> {
        let mut pool = self.buffer_pool.lock().unwrap();
        let dirty_pages = pool.flush_all();
        let count = dirty_pages.len();

        pool.clear_all_dirty();
        drop(pool);

        // Write all dirty pages
        for (_page_id, page) in dirty_pages {
            self.write_page_to_disk(&page)?;
        }

        Ok(count)
    }

    /// Create a new page for a table
    pub fn create_page(&self, table_id: u32, page_number: u32) -> Result<PageId, DatabaseError> {
        let page_id = PageId::new(table_id, page_number);
        let page = Page::new(page_id);

        // Write to disk
        self.write_page_to_disk(&page)?;

        // Add to buffer pool
        let mut pool = self.buffer_pool.lock().unwrap();
        pool.insert_page(page)?;

        Ok(page_id)
    }

    /// Get number of pages for a table
    pub fn get_page_count(&self, table_id: u32) -> usize {
        let table_dir = self.data_dir.join(format!("table_{}", table_id));

        if !table_dir.exists() {
            return 0;
        }

        fs::read_dir(&table_dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path()
                            .extension()
                            .and_then(|s| s.to_str())
                            == Some("dat")
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// Delete all pages for a table
    pub fn delete_table_pages(&self, table_id: u32) -> Result<(), DatabaseError> {
        let table_dir = self.data_dir.join(format!("table_{}", table_id));

        if table_dir.exists() {
            fs::remove_dir_all(&table_dir)?;
        }

        // Remove from buffer pool
        let mut pool = self.buffer_pool.lock().unwrap();
        let page_count = self.get_page_count(table_id);
        for page_number in 0..page_count as u32 {
            let page_id = PageId::new(table_id, page_number);
            pool.remove_page(page_id);
        }

        Ok(())
    }

    /// Get buffer pool statistics
    pub fn get_stats(&self) -> BufferPoolStats {
        let pool = self.buffer_pool.lock().unwrap();
        BufferPoolStats {
            size: pool.size(),
            dirty_count: pool.dirty_count(),
            hit_rate: pool.hit_rate(),
            hits: pool.hits,
            misses: pool.misses,
        }
    }

    /// Get reference to buffer pool (for advanced operations)
    pub fn buffer_pool(&self) -> Arc<Mutex<BufferPool>> {
        Arc::clone(&self.buffer_pool)
    }
}

/// Guard for mutable access to a page
pub struct PageMutGuard<'a> {
    page_id: PageId,
    page_manager: &'a PageManager,
}

impl<'a> PageMutGuard<'a> {
    /// Get mutable reference to the page
    pub fn get_mut<F, R>(&self, f: F) -> Result<R, DatabaseError>
    where
        F: FnOnce(&mut Page) -> Result<R, DatabaseError>,
    {
        let mut pool = self.page_manager.buffer_pool.lock().unwrap();

        if let Some(page) = pool.get_page_mut(self.page_id) {
            f(page)
        } else {
            Err(DatabaseError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Page not found in buffer pool",
            )))
        }
    }
}

/// Buffer pool statistics
#[derive(Debug, Clone)]
pub struct BufferPoolStats {
    pub size: usize,
    pub dirty_count: usize,
    pub hit_rate: f64,
    pub hits: u64,
    pub misses: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;
    use tempfile::TempDir;

    #[test]
    fn test_page_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let pm = PageManager::new(temp_dir.path(), 100).unwrap();

        let stats = pm.get_stats();
        assert_eq!(stats.size, 0);
    }

    #[test]
    fn test_create_and_read_page() {
        let temp_dir = TempDir::new().unwrap();
        let pm = PageManager::new(temp_dir.path(), 100).unwrap();

        // Create page
        let page_id = pm.create_page(1, 0).unwrap();

        // Read page
        let page = pm.get_page(page_id).unwrap();
        assert_eq!(page.header.page_id, page_id);
    }

    #[test]
    fn test_insert_and_read_row() {
        let temp_dir = TempDir::new().unwrap();
        let pm = PageManager::new(temp_dir.path(), 100).unwrap();

        let page_id = pm.create_page(1, 0).unwrap();

        // Insert row
        {
            let guard = pm.get_page_mut(page_id).unwrap();
            guard.get_mut(|page| {
                let row = Row::new(vec![Value::Integer(42), Value::Text("test".to_string())]);
                page.insert_row(&row)?;
                Ok(())
            }).unwrap();
        }

        // Flush to disk
        pm.flush_page(page_id).unwrap();

        // Read back
        let page = pm.get_page(page_id).unwrap();
        let rows = page.get_all_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].values[0], Value::Integer(42));
    }

    #[test]
    fn test_checkpoint() {
        let temp_dir = TempDir::new().unwrap();
        let pm = PageManager::new(temp_dir.path(), 100).unwrap();

        // Create and modify 3 pages
        for i in 0..3 {
            let page_id = pm.create_page(1, i).unwrap();
            let guard = pm.get_page_mut(page_id).unwrap();
            guard.get_mut(|page| {
                let row = Row::new(vec![Value::Integer(i as i64)]);
                page.insert_row(&row)?;
                Ok(())
            }).unwrap();
        }

        // Checkpoint
        let flushed = pm.checkpoint().unwrap();
        assert_eq!(flushed, 3);

        // Verify all pages written
        assert_eq!(pm.get_page_count(1), 3);
    }

    #[test]
    fn test_buffer_pool_caching() {
        let temp_dir = TempDir::new().unwrap();
        let pm = PageManager::new(temp_dir.path(), 10).unwrap();

        let page_id = pm.create_page(1, 0).unwrap();

        // First read - cache miss
        pm.get_page(page_id).unwrap();

        // Second read - cache hit
        pm.get_page(page_id).unwrap();

        let stats = pm.get_stats();
        assert!(stats.hits > 0);
        assert!(stats.hit_rate > 0.0);
    }

    #[test]
    fn test_delete_table_pages() {
        let temp_dir = TempDir::new().unwrap();
        let pm = PageManager::new(temp_dir.path(), 100).unwrap();

        // Create 3 pages
        for i in 0..3 {
            pm.create_page(1, i).unwrap();
        }

        assert_eq!(pm.get_page_count(1), 3);

        // Delete table
        pm.delete_table_pages(1).unwrap();

        assert_eq!(pm.get_page_count(1), 0);
    }

    #[test]
    fn test_persistence() {
        let temp_dir = TempDir::new().unwrap();

        let page_id = {
            let pm = PageManager::new(temp_dir.path(), 100).unwrap();
            let page_id = pm.create_page(1, 0).unwrap();

            // Insert row
            let guard = pm.get_page_mut(page_id).unwrap();
            guard.get_mut(|page| {
                let row = Row::new(vec![Value::Text("persistent".to_string())]);
                page.insert_row(&row)?;
                Ok(())
            }).unwrap();

            pm.checkpoint().unwrap();
            page_id
        };

        // Create new PageManager (simulates restart)
        let pm = PageManager::new(temp_dir.path(), 100).unwrap();
        let page = pm.get_page(page_id).unwrap();
        let rows = page.get_all_rows();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].values[0], Value::Text("persistent".to_string()));
    }
}
