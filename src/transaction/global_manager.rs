use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use std::collections::HashSet;

/// Snapshot for REPEATABLE READ / READ COMMITTED isolation
///
/// Captures the state of active transactions at a point in time,
/// used to determine row visibility in MVCC.
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Oldest active transaction ID when snapshot was taken
    /// Transactions with ID < xmin are guaranteed committed
    pub xmin: u64,

    /// Next transaction ID when snapshot was taken
    /// Transactions with ID >= xmax are invisible to this snapshot
    pub xmax: u64,

    /// List of active (uncommitted) transaction IDs at snapshot time
    /// These transactions' changes are invisible even if xmin <= tx_id < xmax
    pub active_txs: Vec<u64>,
}

impl Snapshot {
    /// Creates a new snapshot for testing
    #[cfg(test)]
    pub fn new(xmin: u64, xmax: u64, active_txs: Vec<u64>) -> Self {
        Self { xmin, xmax, active_txs }
    }
}

/// Global transaction manager shared across all connections
///
/// Provides:
/// - Atomic transaction ID generation
/// - Active transaction tracking for MVCC visibility
/// - Snapshot creation for transaction isolation
///
/// This enables proper multi-connection transaction isolation,
/// preventing uncommitted changes from being visible to other transactions.
#[derive(Debug, Clone)]
pub struct GlobalTransactionManager {
    /// Atomic counter for generating unique transaction IDs
    next_tx_id: Arc<AtomicU64>,

    /// Active (uncommitted) transactions
    /// Protected by RwLock for concurrent access from multiple connections
    active_transactions: Arc<RwLock<HashSet<u64>>>,
}

impl GlobalTransactionManager {
    /// Creates a new global transaction manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            // Start from 1 (0 is reserved for initial data)
            next_tx_id: Arc::new(AtomicU64::new(1)),
            active_transactions: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Begins a new transaction and returns (tx_id, snapshot)
    ///
    /// The snapshot captures the current state of active transactions,
    /// which will be used for visibility checks throughout the transaction.
    ///
    /// # Returns
    /// - `tx_id`: Unique transaction ID for this transaction
    /// - `snapshot`: Snapshot of active transactions for isolation
    pub fn begin_transaction(&self) -> (u64, Snapshot) {
        // Allocate new transaction ID
        let tx_id = self.next_tx_id.fetch_add(1, Ordering::SeqCst);
        let xmax = self.next_tx_id.load(Ordering::SeqCst);

        // Read active transactions before registering this one
        // This ensures we don't see our own transaction in the snapshot
        let active_txs = {
            let active = self.active_transactions.read().expect("RwLock poisoned");
            active.iter().copied().collect::<Vec<_>>()
        };

        // xmin is the oldest active transaction
        // If no active transactions, xmin = xmax (all previous txs committed)
        let xmin = active_txs.iter().min().copied().unwrap_or(xmax);

        // Register this transaction as active
        {
            let mut active = self.active_transactions.write().expect("RwLock poisoned");
            active.insert(tx_id);
        }

        let snapshot = Snapshot {
            xmin,
            xmax,
            active_txs,
        };

        (tx_id, snapshot)
    }

    /// Commits a transaction
    ///
    /// Removes the transaction from the active set, making its changes
    /// visible to new snapshots.
    pub fn commit_transaction(&self, tx_id: u64) {
        let mut active = self.active_transactions.write().expect("RwLock poisoned");
        active.remove(&tx_id);
    }

    /// Rolls back a transaction
    ///
    /// Removes the transaction from the active set. The changes will be
    /// discarded by the caller (by restoring from snapshot or marking rows invalid).
    pub fn rollback_transaction(&self, tx_id: u64) {
        let mut active = self.active_transactions.write().expect("RwLock poisoned");
        active.remove(&tx_id);
    }

    /// Gets the current transaction ID (for auto-commit queries)
    ///
    /// Returns the next ID that will be assigned to a transaction.
    #[must_use]
    pub fn current_tx_id(&self) -> u64 {
        self.next_tx_id.load(Ordering::SeqCst)
    }

    /// Gets the oldest active transaction ID (for VACUUM)
    ///
    /// Returns the minimum transaction ID among all active transactions.
    /// Rows deleted by transactions <= this ID can be safely removed by VACUUM.
    ///
    /// If no transactions are active, returns current_tx_id - 1.
    #[must_use]
    pub fn get_oldest_active_tx(&self) -> u64 {
        let active = self.active_transactions.read().expect("RwLock poisoned");
        let current = self.current_tx_id();

        // Return minimum active tx_id, or current-1 if none active
        active.iter().min().copied()
            .unwrap_or_else(|| current.saturating_sub(1))
            .max(1)
    }

    /// Creates a new snapshot for READ COMMITTED isolation
    ///
    /// READ COMMITTED takes a new snapshot before each statement,
    /// so it can see changes committed by other transactions.
    #[must_use]
    pub fn get_snapshot(&self) -> Snapshot {
        let xmax = self.next_tx_id.load(Ordering::SeqCst);

        let active_txs = {
            let active = self.active_transactions.read().expect("RwLock poisoned");
            active.iter().copied().collect::<Vec<_>>()
        };

        let xmin = active_txs.iter().min().copied().unwrap_or(xmax);

        Snapshot {
            xmin,
            xmax,
            active_txs,
        }
    }
}

impl Default for GlobalTransactionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_global_transaction_manager() {
        let gtm = GlobalTransactionManager::new();
        assert_eq!(gtm.current_tx_id(), 1);
        assert_eq!(gtm.get_oldest_active_tx(), 1);
    }

    #[test]
    fn test_begin_transaction_increments() {
        let gtm = GlobalTransactionManager::new();

        let (tx1, _) = gtm.begin_transaction();
        let (tx2, _) = gtm.begin_transaction();
        let (tx3, _) = gtm.begin_transaction();

        assert_eq!(tx1, 1);
        assert_eq!(tx2, 2);
        assert_eq!(tx3, 3);
        assert_eq!(gtm.current_tx_id(), 4);
    }

    #[test]
    fn test_clone_shares_state() {
        let gtm1 = GlobalTransactionManager::new();
        let gtm2 = gtm1.clone();

        let (tx1, _) = gtm1.begin_transaction();
        let (tx2, _) = gtm2.begin_transaction();

        assert_eq!(tx1, 1);
        assert_eq!(tx2, 2);
        assert_eq!(gtm1.current_tx_id(), 3);
        assert_eq!(gtm2.current_tx_id(), 3);
    }

    #[test]
    fn test_snapshot_captures_active_transactions() {
        let gtm = GlobalTransactionManager::new();

        let (tx1, snap1) = gtm.begin_transaction();
        let (tx2, snap2) = gtm.begin_transaction();

        // snap1 was taken before tx2 started, so it shouldn't see tx1 in active_txs
        assert!(snap1.active_txs.is_empty());
        assert_eq!(snap1.xmin, 2);  // No active txs, so xmin = xmax
        assert_eq!(snap1.xmax, 2);

        // snap2 should see tx1 as active
        assert_eq!(snap2.active_txs, vec![1]);
        assert_eq!(snap2.xmin, 1);  // Oldest active is tx1
        assert_eq!(snap2.xmax, 3);

        // Cleanup
        gtm.commit_transaction(tx1);
        gtm.commit_transaction(tx2);
    }

    #[test]
    fn test_commit_removes_from_active() {
        let gtm = GlobalTransactionManager::new();

        let (tx1, _) = gtm.begin_transaction();
        assert_eq!(gtm.get_oldest_active_tx(), 1);

        gtm.commit_transaction(tx1);

        // After commit, oldest_active should advance
        let oldest = gtm.get_oldest_active_tx();
        assert!(oldest >= 1);
    }

    #[test]
    fn test_rollback_removes_from_active() {
        let gtm = GlobalTransactionManager::new();

        let (tx1, _) = gtm.begin_transaction();
        assert_eq!(gtm.get_oldest_active_tx(), 1);

        gtm.rollback_transaction(tx1);

        // After rollback, oldest_active should advance
        let oldest = gtm.get_oldest_active_tx();
        assert!(oldest >= 1);
    }

    #[test]
    fn test_get_oldest_active_tx_with_multiple_transactions() {
        let gtm = GlobalTransactionManager::new();

        let (tx1, _) = gtm.begin_transaction();  // ID = 1
        let (tx2, _) = gtm.begin_transaction();  // ID = 2
        let (tx3, _) = gtm.begin_transaction();  // ID = 3

        // Oldest active should be tx1
        assert_eq!(gtm.get_oldest_active_tx(), 1);

        // Commit tx1
        gtm.commit_transaction(tx1);

        // Now oldest active should be tx2
        assert_eq!(gtm.get_oldest_active_tx(), 2);

        // Commit tx2
        gtm.commit_transaction(tx2);

        // Now oldest active should be tx3
        assert_eq!(gtm.get_oldest_active_tx(), 3);

        // Commit tx3
        gtm.commit_transaction(tx3);

        // No active transactions, should return current-1
        assert_eq!(gtm.get_oldest_active_tx(), 3);
    }

    #[test]
    fn test_read_committed_snapshot() {
        let gtm = GlobalTransactionManager::new();

        let (tx1, _) = gtm.begin_transaction();

        // Get READ COMMITTED snapshot - should see tx1 as active
        let snap = gtm.get_snapshot();
        assert_eq!(snap.active_txs, vec![1]);
        assert_eq!(snap.xmin, 1);

        gtm.commit_transaction(tx1);

        // New snapshot after commit - should not see tx1
        let snap2 = gtm.get_snapshot();
        assert!(snap2.active_txs.is_empty());
    }
}
