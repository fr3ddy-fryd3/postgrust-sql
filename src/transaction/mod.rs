// Transaction module - MVCC and snapshot isolation

mod snapshot;
mod manager;
mod global_manager;

pub use snapshot::Transaction;
pub use manager::TransactionManager;
pub use global_manager::{GlobalTransactionManager, Snapshot};
