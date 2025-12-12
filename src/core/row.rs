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
    #[must_use] 
    pub const fn new(values: Vec<Value>) -> Self {
        Self {
            values,
            xmin: 0, // Will be set by TransactionManager
            xmax: None,
        }
    }

    #[must_use] 
    pub const fn new_with_xmin(values: Vec<Value>, xmin: u64) -> Self {
        Self {
            values,
            xmin,
            xmax: None,
        }
    }

    /// Checks if this row is visible to a given transaction (Read Committed isolation)
    #[must_use] 
    pub fn is_visible(&self, current_tx_id: u64) -> bool {
        // Row is visible if:
        // 1. It was created before or in current transaction (xmin <= current_tx_id)
        // 2. AND it hasn't been deleted (xmax is None) OR was deleted by a transaction
        //    that started after current transaction (xmax > current_tx_id)
        self.xmin <= current_tx_id && self.xmax.is_none_or(|xmax| xmax > current_tx_id)
    }

    /// Checks if this row is dead and can be removed by VACUUM
    ///
    /// A row is dead if:
    /// 1. It has been deleted/updated (xmax is set)
    /// 2. The deletion is committed and invisible to all active transactions
    ///    (xmax <= `oldest_active_tx`)
    ///
    /// This ensures we only vacuum tuples that no transaction can see.
    #[must_use] 
    pub const fn is_dead(&self, oldest_active_tx: u64) -> bool {
        match self.xmax {
            Some(xmax) => xmax <= oldest_active_tx,
            None => false, // Row is still alive
        }
    }

    /// Mark this row as deleted by setting xmax (MVCC soft delete)
    ///
    /// Instead of physically removing the row, we mark it with the transaction ID
    /// that deleted it. This allows other transactions to still see the row if needed,
    /// and VACUUM will physically remove it later when safe.
    pub const fn mark_deleted(&mut self, tx_id: u64) {
        self.xmax = Some(tx_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_is_dead_with_no_xmax() {
        let row = Row {
            values: vec![],
            xmin: 100,
            xmax: None,
        };

        // Alive row is never dead
        assert!(!row.is_dead(200));
        assert!(!row.is_dead(150));
        assert!(!row.is_dead(100));
    }

    #[test]
    fn test_row_is_dead_with_old_xmax() {
        let row = Row {
            values: vec![],
            xmin: 100,
            xmax: Some(150),
        };

        // Dead if xmax <= oldest_active_tx
        assert!(row.is_dead(200));  // Deleted at 150, oldest tx is 200
        assert!(row.is_dead(151));  // Deleted at 150, oldest tx is 151
        assert!(row.is_dead(150));  // Deleted at 150, oldest tx is 150 (edge case)
    }

    #[test]
    fn test_row_not_dead_if_visible_to_active_tx() {
        let row = Row {
            values: vec![],
            xmin: 100,
            xmax: Some(150),
        };

        // Not dead if some transaction can still see it
        assert!(!row.is_dead(149)); // Transaction 149 can see it (started before delete)
        assert!(!row.is_dead(140)); // Transaction 140 can see it
        assert!(!row.is_dead(100)); // Transaction 100 can see it
    }
}
