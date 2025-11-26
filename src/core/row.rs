use serde::{Deserialize, Serialize};
use super::value::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub values: Vec<Value>,
    /// Transaction ID that created this row (for MVCC)
    pub xmin: u64,
    /// Transaction ID that deleted this row (None if still visible, for MVCC)
    pub xmax: Option<u64>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self {
            values,
            xmin: 0, // Will be set by TransactionManager
            xmax: None,
        }
    }

    pub fn new_with_xmin(values: Vec<Value>, xmin: u64) -> Self {
        Self {
            values,
            xmin,
            xmax: None,
        }
    }

    /// Checks if this row is visible to a given transaction (Read Committed isolation)
    pub fn is_visible(&self, current_tx_id: u64) -> bool {
        // Row is visible if:
        // 1. It was created before or in current transaction (xmin <= current_tx_id)
        // 2. AND it hasn't been deleted (xmax is None) OR was deleted by a transaction
        //    that started after current transaction (xmax > current_tx_id)
        self.xmin <= current_tx_id && self.xmax.map_or(true, |xmax| xmax > current_tx_id)
    }
}
