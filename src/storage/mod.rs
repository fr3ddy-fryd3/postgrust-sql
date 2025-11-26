// Storage module - disk persistence and WAL

mod disk;
pub mod wal;

pub use disk::StorageEngine;
pub use wal::{Operation, WalManager};
