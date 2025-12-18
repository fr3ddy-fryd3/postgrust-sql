use crate::types::Database;
use super::Snapshot as MvccSnapshot;

/// Per-connection transaction state
///
/// Manages both:
/// 1. MVCC snapshot for visibility checks (lightweight)
/// 2. Database snapshot for rollback (heavyweight, will be replaced with WAL-based rollback)
pub struct Transaction {
    /// Current transaction ID (None if no active transaction)
    tx_id: Option<u64>,

    /// MVCC snapshot for REPEATABLE READ isolation
    /// Captures active transactions at BEGIN time for visibility checks
    mvcc_snapshot: Option<MvccSnapshot>,

    /// Full database snapshot for rollback (legacy, will be removed in future)
    /// TODO v2.2: Replace with WAL-based rollback
    db_snapshot: Option<Database>,
}

impl Transaction {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tx_id: None,
            mvcc_snapshot: None,
            db_snapshot: None,
        }
    }

    /// Begins a new transaction with the given transaction ID and MVCC snapshot
    ///
    /// # Arguments
    /// - `tx_id`: Unique transaction ID
    /// - `mvcc_snapshot`: Snapshot of active transactions for visibility
    /// - `db`: Database reference for rollback snapshot
    pub fn begin(&mut self, tx_id: u64, mvcc_snapshot: MvccSnapshot, db: &Database) {
        self.tx_id = Some(tx_id);
        self.mvcc_snapshot = Some(mvcc_snapshot);
        // Keep full DB snapshot for rollback (legacy)
        self.db_snapshot = Some(db.clone());
    }

    /// Commits the current transaction
    ///
    /// Clears transaction state. The caller should call `GlobalTransactionManager::commit_transaction()`
    /// to remove the transaction from active set.
    pub fn commit(&mut self) {
        self.tx_id = None;
        self.mvcc_snapshot = None;
        self.db_snapshot = None;
    }

    /// Rolls back the current transaction
    ///
    /// Restores database to the state before transaction began.
    /// The caller should call `GlobalTransactionManager::rollback_transaction()`
    /// to remove the transaction from active set.
    pub fn rollback(&mut self, db: &mut Database) {
        if let Some(snapshot) = self.db_snapshot.take() {
            *db = snapshot;
        }
        self.tx_id = None;
        self.mvcc_snapshot = None;
    }

    /// Gets the MVCC snapshot for this transaction
    ///
    /// Used for visibility checks in SELECT/UPDATE/DELETE queries.
    /// For REPEATABLE READ: snapshot taken at BEGIN
    /// For READ COMMITTED: new snapshot before each statement
    #[must_use]
    pub const fn snapshot(&self) -> Option<&MvccSnapshot> {
        self.mvcc_snapshot.as_ref()
    }

    /// Checks if there's an active transaction
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.tx_id.is_some()
    }

    /// Gets the current transaction ID
    #[must_use]
    pub const fn tx_id(&self) -> Option<u64> {
        self.tx_id
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}
