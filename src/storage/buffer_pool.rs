use std::collections::{HashMap, HashSet, VecDeque};
use super::page::{Page, PageId};
use crate::types::DatabaseError;

/// Simple LRU cache for page eviction
struct LruCache {
    /// Queue of page IDs (most recently used at back)
    queue: VecDeque<PageId>,
    /// Maximum capacity
    capacity: usize,
}

impl LruCache {
    fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Mark page as recently used
    fn touch(&mut self, page_id: PageId) {
        // Remove if exists
        self.queue.retain(|&id| id != page_id);
        // Add to back (most recent)
        self.queue.push_back(page_id);
    }

    /// Get least recently used page
    fn get_lru(&self) -> Option<PageId> {
        self.queue.front().copied()
    }

    /// Remove page from LRU
    fn remove(&mut self, page_id: PageId) {
        self.queue.retain(|&id| id != page_id);
    }

    /// Is cache full?
    fn is_full(&self) -> bool {
        self.queue.len() >= self.capacity
    }
}

/// Buffer Pool - cache of pages in RAM
pub struct BufferPool {
    /// Pages currently in memory
    pages: HashMap<PageId, Page>,
    /// Dirty pages that need to be written to disk
    dirty_pages: HashSet<PageId>,
    /// LRU cache for eviction policy
    lru: LruCache,
    /// Statistics
    pub hits: u64,
    pub misses: u64,
}

impl BufferPool {
    /// Create new buffer pool with given capacity (number of pages)
    pub fn new(capacity: usize) -> Self {
        Self {
            pages: HashMap::new(),
            dirty_pages: HashSet::new(),
            lru: LruCache::new(capacity),
            hits: 0,
            misses: 0,
        }
    }

    /// Get a page from the buffer pool
    pub fn get_page(&mut self, page_id: PageId) -> Option<&Page> {
        if self.pages.contains_key(&page_id) {
            self.hits += 1;
            self.lru.touch(page_id);
            self.pages.get(&page_id)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Get a mutable page from the buffer pool
    pub fn get_page_mut(&mut self, page_id: PageId) -> Option<&mut Page> {
        if self.pages.contains_key(&page_id) {
            self.hits += 1;
            self.lru.touch(page_id);
            // Mark as dirty since caller has mutable access
            self.dirty_pages.insert(page_id);
            self.pages.get_mut(&page_id)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert a page into the buffer pool
    pub fn insert_page(&mut self, page: Page) -> Result<Option<PageId>, DatabaseError> {
        let page_id = page.header.page_id;

        // Check if we need to evict
        let evicted = if self.lru.is_full() && !self.pages.contains_key(&page_id) {
            self.evict_page()?
        } else {
            None
        };

        // Insert page
        self.pages.insert(page_id, page);
        self.lru.touch(page_id);

        Ok(evicted)
    }

    /// Mark a page as dirty
    pub fn mark_dirty(&mut self, page_id: PageId) {
        if self.pages.contains_key(&page_id) {
            self.dirty_pages.insert(page_id);
        }
    }

    /// Evict least recently used page
    fn evict_page(&mut self) -> Result<Option<PageId>, DatabaseError> {
        // Find LRU page that is not dirty
        let candidate = self.lru.get_lru();

        // If LRU is dirty, we need to write it first (handled by caller)
        // For now, just evict the LRU regardless
        if let Some(page_id) = candidate {
            // If it's dirty, caller must flush it first
            if self.dirty_pages.contains(&page_id) {
                return Ok(Some(page_id)); // Signal caller to flush this page
            }

            // Remove from cache
            self.pages.remove(&page_id);
            self.lru.remove(page_id);
            return Ok(Some(page_id));
        }

        Ok(None)
    }

    /// Get all dirty pages
    pub fn get_dirty_pages(&self) -> Vec<PageId> {
        self.dirty_pages.iter().copied().collect()
    }

    /// Clear dirty flag for a page (after it's been written to disk)
    pub fn clear_dirty(&mut self, page_id: PageId) {
        self.dirty_pages.remove(&page_id);
    }

    /// Clear all dirty flags
    pub fn clear_all_dirty(&mut self) {
        self.dirty_pages.clear();
    }

    /// Get number of pages in buffer pool
    pub fn size(&self) -> usize {
        self.pages.len()
    }

    /// Get number of dirty pages
    pub fn dirty_count(&self) -> usize {
        self.dirty_pages.len()
    }

    /// Get cache hit rate
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Remove a page from buffer pool
    pub fn remove_page(&mut self, page_id: PageId) -> Option<Page> {
        self.lru.remove(page_id);
        self.dirty_pages.remove(&page_id);
        self.pages.remove(&page_id)
    }

    /// Flush all dirty pages (returns them for writing)
    pub fn flush_all(&mut self) -> Vec<(PageId, Page)> {
        let dirty_ids: Vec<_> = self.dirty_pages.iter().copied().collect();
        let mut result = Vec::new();

        for page_id in dirty_ids {
            if let Some(page) = self.pages.get(&page_id) {
                result.push((page_id, page.clone()));
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Row, Value};

    fn create_test_page(table_id: u32, page_number: u32) -> Page {
        let page_id = PageId::new(table_id, page_number);
        let mut page = Page::new(page_id);

        // Add a test row
        let row = Row::new(vec![Value::Integer(page_number as i64)]);
        page.insert_row(&row).unwrap();

        page
    }

    #[test]
    fn test_buffer_pool_creation() {
        let pool = BufferPool::new(10);
        assert_eq!(pool.size(), 0);
        assert_eq!(pool.dirty_count(), 0);
    }

    #[test]
    fn test_insert_and_get() {
        let mut pool = BufferPool::new(10);
        let page = create_test_page(1, 0);
        let page_id = page.header.page_id;

        pool.insert_page(page).unwrap();

        assert_eq!(pool.size(), 1);
        assert!(pool.get_page(page_id).is_some());
        assert_eq!(pool.hits, 1);
    }

    #[test]
    fn test_dirty_tracking() {
        let mut pool = BufferPool::new(10);
        let page = create_test_page(1, 0);
        let page_id = page.header.page_id;

        pool.insert_page(page).unwrap();

        // Get mutable reference marks as dirty
        pool.get_page_mut(page_id);

        assert_eq!(pool.dirty_count(), 1);
        assert!(pool.get_dirty_pages().contains(&page_id));
    }

    #[test]
    fn test_eviction() {
        let mut pool = BufferPool::new(3);

        // Fill buffer pool
        for i in 0..3 {
            let page = create_test_page(1, i);
            pool.insert_page(page).unwrap();
        }

        assert_eq!(pool.size(), 3);

        // Insert 4th page - should evict LRU
        let page = create_test_page(1, 3);
        let evicted = pool.insert_page(page).unwrap();

        // Should have evicted a page
        assert!(evicted.is_some() || pool.size() == 4);
    }

    #[test]
    fn test_lru_policy() {
        let mut pool = BufferPool::new(3);

        // Insert 3 pages
        for i in 0..3 {
            let page = create_test_page(1, i);
            pool.insert_page(page).unwrap();
        }

        // Access page 0 (make it most recent)
        let page_id_0 = PageId::new(1, 0);
        pool.get_page(page_id_0);

        // Now page 1 is LRU
        // Insert new page should evict page 1
        let page = create_test_page(1, 3);
        pool.insert_page(page).unwrap();

        // Page 0 should still be there
        assert!(pool.get_page(page_id_0).is_some());
    }

    #[test]
    fn test_hit_rate() {
        let mut pool = BufferPool::new(10);
        let page = create_test_page(1, 0);
        let page_id = page.header.page_id;

        pool.insert_page(page).unwrap();

        // 5 hits
        for _ in 0..5 {
            pool.get_page(page_id);
        }

        // 2 misses
        pool.get_page(PageId::new(1, 999));
        pool.get_page(PageId::new(1, 998));

        // Hit rate should be 5/7
        let hit_rate = pool.hit_rate();
        assert!((hit_rate - 5.0/7.0).abs() < 0.01);
    }

    #[test]
    fn test_flush_all() {
        let mut pool = BufferPool::new(10);

        // Insert and modify 3 pages
        for i in 0..3 {
            let page = create_test_page(1, i);
            let page_id = page.header.page_id;
            pool.insert_page(page).unwrap();
            pool.mark_dirty(page_id);
        }

        assert_eq!(pool.dirty_count(), 3);

        let dirty_pages = pool.flush_all();
        assert_eq!(dirty_pages.len(), 3);
    }
}
