use serde::{Deserialize, Serialize};
use crate::types::{DatabaseError, Row};

/// Page size (8 KB, same as `PostgreSQL`)
pub const PAGE_SIZE: usize = 8192;

/// Page ID - uniquely identifies a page
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PageId {
    pub table_id: u32,
    pub page_number: u32,
}

impl PageId {
    #[must_use] 
    pub const fn new(table_id: u32, page_number: u32) -> Self {
        Self { table_id, page_number }
    }
}

/// Slot - points to a row within a page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slot {
    /// Offset from start of page data
    pub offset: u16,
    /// Length of the row in bytes
    pub length: u16,
    /// Is this slot used (false = deleted)
    pub is_used: bool,
}

/// Page Header - metadata about the page
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageHeader {
    /// Page ID
    pub page_id: PageId,
    /// Free space available in bytes
    pub free_space: u16,
    /// Number of slots (including deleted)
    pub slot_count: u16,
    /// Lower bound of free space (grows upward)
    pub lower: u16,
    /// Upper bound of free space (grows downward)
    pub upper: u16,
    /// Checksum for integrity check (not implemented yet)
    pub checksum: u32,
}

impl PageHeader {
    #[must_use] 
    pub const fn new(page_id: PageId) -> Self {
        Self {
            page_id,
            free_space: (PAGE_SIZE - std::mem::size_of::<Self>()) as u16,
            slot_count: 0,
            lower: std::mem::size_of::<Self>() as u16,
            upper: PAGE_SIZE as u16,
            checksum: 0,
        }
    }
}

/// Page - 8 KB unit of storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    /// Page header
    pub header: PageHeader,
    /// Slots pointing to rows
    pub slots: Vec<Slot>,
    /// Raw page data (8 KB)
    pub data: Vec<u8>,
}

impl Page {
    /// Create a new empty page
    #[must_use] 
    pub fn new(page_id: PageId) -> Self {
        let mut data = vec![0u8; PAGE_SIZE];
        let header = PageHeader::new(page_id);

        // Write header to beginning of page
        let header_bytes = bincode::serialize(&header).unwrap();
        data[..header_bytes.len()].copy_from_slice(&header_bytes);

        Self {
            header,
            slots: Vec::new(),
            data,
        }
    }

    /// Get available free space
    #[must_use] 
    pub const fn free_space(&self) -> u16 {
        self.header.upper.saturating_sub(self.header.lower)
    }

    /// Can this page fit a row of given size?
    #[must_use] 
    pub const fn can_fit(&self, row_size: usize) -> bool {
        let slot_size = std::mem::size_of::<Slot>();
        let needed = row_size + slot_size;
        self.free_space() as usize >= needed
    }

    /// Insert a row into this page
    pub fn insert_row(&mut self, row: &Row) -> Result<u16, DatabaseError> {
        // Serialize the row
        let row_bytes = bincode::serialize(row)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;

        let row_size = row_bytes.len();

        // Check if we have space
        if !self.can_fit(row_size) {
            return Err(DatabaseError::Io(std::io::Error::other(
                "Page is full",
            )));
        }

        // Allocate space from the end of the page (upper grows downward)
        let new_upper = self.header.upper - row_size as u16;
        let offset = new_upper;

        // Write row data
        self.data[offset as usize..(offset as usize + row_size)]
            .copy_from_slice(&row_bytes);

        // Create slot
        let slot = Slot {
            offset,
            length: row_size as u16,
            is_used: true,
        };

        let slot_index = self.slots.len() as u16;
        self.slots.push(slot);

        // Update header
        self.header.slot_count += 1;
        self.header.upper = new_upper;
        self.header.lower += std::mem::size_of::<Slot>() as u16;
        self.header.free_space = self.free_space();

        Ok(slot_index)
    }

    /// Get a row by slot index
    pub fn get_row(&self, slot_index: u16) -> Result<Row, DatabaseError> {
        let slot = self.slots.get(slot_index as usize)
            .ok_or_else(|| DatabaseError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid slot index",
            )))?;

        if !slot.is_used {
            return Err(DatabaseError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Slot is not in use",
            )));
        }

        let offset = slot.offset as usize;
        let length = slot.length as usize;
        let row_bytes = &self.data[offset..offset + length];

        bincode::deserialize(row_bytes)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))
    }

    /// Mark a row as deleted (doesn't reclaim space)
    pub fn delete_row(&mut self, slot_index: u16) -> Result<(), DatabaseError> {
        let slot = self.slots.get_mut(slot_index as usize)
            .ok_or_else(|| DatabaseError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid slot index",
            )))?;

        slot.is_used = false;
        Ok(())
    }

    /// Update a row in place (if it fits)
    pub fn update_row(&mut self, slot_index: u16, new_row: &Row) -> Result<bool, DatabaseError> {
        let row_bytes = bincode::serialize(new_row)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;

        let slot = self.slots.get_mut(slot_index as usize)
            .ok_or_else(|| DatabaseError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid slot index",
            )))?;

        // Check if new row fits in the same space
        if row_bytes.len() <= slot.length as usize {
            // Update in place
            let offset = slot.offset as usize;
            self.data[offset..offset + row_bytes.len()].copy_from_slice(&row_bytes);
            slot.length = row_bytes.len() as u16;
            Ok(true)
        } else {
            // Doesn't fit - caller needs to delete and insert elsewhere
            Ok(false)
        }
    }

    /// Get all rows in this page
    #[must_use] 
    pub fn get_all_rows(&self) -> Vec<Row> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_used)
            .filter_map(|(idx, _)| self.get_row(idx as u16).ok())
            .collect()
    }

    /// Serialize page to bytes for disk storage
    pub fn to_bytes(&self) -> Result<Vec<u8>, DatabaseError> {
        bincode::serialize(self)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))
    }

    /// Deserialize page from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DatabaseError> {
        bincode::deserialize(bytes)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Value;

    #[test]
    fn test_page_creation() {
        let page_id = PageId::new(1, 0);
        let page = Page::new(page_id);

        assert_eq!(page.header.page_id, page_id);
        assert_eq!(page.header.slot_count, 0);
        assert!(page.free_space() > 8000); // Should have most of 8KB free
    }

    #[test]
    fn test_insert_row() {
        let page_id = PageId::new(1, 0);
        let mut page = Page::new(page_id);

        let row = Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]);
        let slot_idx = page.insert_row(&row).unwrap();

        assert_eq!(slot_idx, 0);
        assert_eq!(page.header.slot_count, 1);

        let retrieved = page.get_row(slot_idx).unwrap();
        assert_eq!(retrieved.values, row.values);
    }

    #[test]
    fn test_multiple_inserts() {
        let page_id = PageId::new(1, 0);
        let mut page = Page::new(page_id);

        for i in 0..10 {
            let row = Row::new(vec![Value::Integer(i), Value::Text(format!("User{}", i))]);
            page.insert_row(&row).unwrap();
        }

        assert_eq!(page.header.slot_count, 10);
        let all_rows = page.get_all_rows();
        assert_eq!(all_rows.len(), 10);
    }

    #[test]
    fn test_delete_row() {
        let page_id = PageId::new(1, 0);
        let mut page = Page::new(page_id);

        let row = Row::new(vec![Value::Integer(1)]);
        let slot_idx = page.insert_row(&row).unwrap();

        page.delete_row(slot_idx).unwrap();
        assert!(!page.slots[slot_idx as usize].is_used);
        assert!(page.get_row(slot_idx).is_err());
    }

    #[test]
    fn test_update_row() {
        let page_id = PageId::new(1, 0);
        let mut page = Page::new(page_id);

        let row = Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]);
        let slot_idx = page.insert_row(&row).unwrap();

        let new_row = Row::new(vec![Value::Integer(1), Value::Text("Bob".to_string())]);
        let fits = page.update_row(slot_idx, &new_row).unwrap();

        assert!(fits);
        let retrieved = page.get_row(slot_idx).unwrap();
        assert_eq!(retrieved.values[1], Value::Text("Bob".to_string()));
    }

    #[test]
    fn test_serialization() {
        let page_id = PageId::new(1, 0);
        let mut page = Page::new(page_id);

        let row = Row::new(vec![Value::Integer(42)]);
        page.insert_row(&row).unwrap();

        let bytes = page.to_bytes().unwrap();
        let deserialized = Page::from_bytes(&bytes).unwrap();

        assert_eq!(deserialized.header.page_id, page_id);
        assert_eq!(deserialized.header.slot_count, 1);
        assert_eq!(deserialized.get_row(0).unwrap().values, row.values);
    }
}
