// Storage module - disk persistence and WAL

mod disk;
pub mod wal;
pub mod page;
pub mod buffer_pool;
pub mod page_manager;
pub mod paged_table;
pub mod database_storage;

pub use disk::StorageEngine;
pub use wal::{Operation, WalManager};
pub use page::{Page, PageId, PageHeader, PAGE_SIZE};
pub use buffer_pool::BufferPool;
pub use page_manager::{PageManager, BufferPoolStats};
pub use paged_table::{PagedTable, PagedTableStats};
pub use database_storage::DatabaseStorage;
