use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Global transaction manager for MVCC
/// Provides unique transaction IDs and manages transaction state
#[derive(Debug, Clone)]
pub struct TransactionManager {
    /// Atomic counter for generating unique transaction IDs
    next_tx_id: Arc<AtomicU64>,
}

impl TransactionManager {
    pub fn new() -> Self {
        Self {
            // Start from 1 (0 is reserved for initial data)
            next_tx_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Generates a new unique transaction ID
    pub fn begin_transaction(&self) -> u64 {
        self.next_tx_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Gets the current transaction ID (for visibility checks)
    /// This returns the next ID that will be assigned
    pub fn current_tx_id(&self) -> u64 {
        self.next_tx_id.load(Ordering::SeqCst)
    }

    /// Gets the oldest transaction ID that could still see data
    /// Used by VACUUM to determine safe cleanup horizon
    ///
    /// Simplified implementation for v1.6.0:
    /// Returns current_tx_id - 1, assuming all previous transactions are committed.
    /// This works for single-connection scenarios but isn't safe for concurrent transactions.
    ///
    /// TODO v1.7: Track active transactions with HashSet<u64> for proper multi-connection support
    pub fn get_oldest_active_tx(&self) -> u64 {
        let current = self.current_tx_id();
        // Assume all transactions before current are committed
        // Safe for testing, but needs proper tracking for production
        current.saturating_sub(1).max(1)
    }
}

impl Default for TransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_manager_new() {
        let tm = TransactionManager::new();
        assert_eq!(tm.current_tx_id(), 1);
    }

    #[test]
    fn test_begin_transaction_increments() {
        let tm = TransactionManager::new();
        let tx1 = tm.begin_transaction();
        let tx2 = tm.begin_transaction();
        let tx3 = tm.begin_transaction();

        assert_eq!(tx1, 1);
        assert_eq!(tx2, 2);
        assert_eq!(tx3, 3);
        assert_eq!(tm.current_tx_id(), 4);
    }

    #[test]
    fn test_clone_shares_counter() {
        let tm1 = TransactionManager::new();
        let tm2 = tm1.clone();

        let tx1 = tm1.begin_transaction();
        let tx2 = tm2.begin_transaction();

        assert_eq!(tx1, 1);
        assert_eq!(tx2, 2);
        assert_eq!(tm1.current_tx_id(), 3);
        assert_eq!(tm2.current_tx_id(), 3);
    }
}
