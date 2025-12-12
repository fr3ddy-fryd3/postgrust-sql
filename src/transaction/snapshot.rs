use crate::types::Database;

/// Per-connection transaction state
/// With MVCC, we don't need to clone the entire database anymore
pub struct Transaction {
    /// Current transaction ID (None if no active transaction)
    tx_id: Option<u64>,
    /// Snapshot of database for rollback (fallback for now, will be removed with full MVCC)
    snapshot: Option<Database>,
}

impl Transaction {
    #[must_use] 
    pub const fn new() -> Self {
        Self {
            tx_id: None,
            snapshot: None,
        }
    }

    /// Begins a new transaction with the given transaction ID
    pub fn begin(&mut self, tx_id: u64, db: &Database) {
        self.tx_id = Some(tx_id);
        // Keep snapshot for rollback until we implement full MVCC rollback
        self.snapshot = Some(db.clone());
    }

    /// Commits the current transaction
    pub fn commit(&mut self) {
        self.tx_id = None;
        self.snapshot = None;
    }

    /// Rolls back the current transaction
    pub fn rollback(&mut self, db: &mut Database) {
        if let Some(snapshot) = self.snapshot.take() {
            *db = snapshot;
        }
        self.tx_id = None;
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
