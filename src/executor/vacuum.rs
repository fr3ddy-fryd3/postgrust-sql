/// VACUUM executor - removes dead tuples from tables
///
/// Implements VACUUM command for MVCC cleanup:
/// - Scans tables for dead tuples (xmax < oldest_active_tx)
/// - Removes dead tuples from storage
/// - Works with both Vec<Row> (legacy) and PagedTable storage

use crate::core::{Database, DatabaseError};
use crate::transaction::TransactionManager;
use super::dispatcher_executor::QueryResult;

pub struct VacuumExecutor;

impl VacuumExecutor {
    /// Execute VACUUM command
    ///
    /// # Arguments
    /// * `db` - Database instance
    /// * `table_name` - Optional table name (None = vacuum all tables)
    /// * `tx_manager` - Transaction manager for getting cleanup horizon
    /// * `database_storage` - Optional page-based storage
    ///
    /// # Returns
    /// QueryResult with number of tuples removed
    pub fn vacuum(
        db: &mut Database,
        table_name: Option<String>,
        tx_manager: &TransactionManager,
        _database_storage: Option<&mut crate::storage::DatabaseStorage>,
    ) -> Result<QueryResult, DatabaseError> {
        // Get cleanup horizon - only tuples invisible to all transactions can be removed
        let oldest_tx = tx_manager.get_oldest_active_tx();

        // Determine which tables to vacuum
        let tables_to_vacuum: Vec<String> = if let Some(name) = table_name {
            // Single table
            if !db.tables.contains_key(&name) {
                return Err(DatabaseError::TableNotFound(name));
            }
            vec![name]
        } else {
            // All tables
            db.tables.keys().cloned().collect()
        };

        // Vacuum each table
        let mut total_removed = 0;
        for table_name in &tables_to_vacuum {
            let removed = Self::vacuum_table(
                db,
                table_name,
                oldest_tx,
            )?;
            total_removed += removed;
        }

        Ok(QueryResult::Success(format!(
            "VACUUM complete. Removed {} dead tuples.",
            total_removed
        )))
    }

    /// Vacuum single table
    fn vacuum_table(
        db: &mut Database,
        table_name: &str,
        oldest_tx: u64,
    ) -> Result<usize, DatabaseError> {
        // For v1.5.1: Only support legacy storage
        // TODO v1.6: Implement VACUUM for page-based storage
        // Requires either:
        // - PagedTable::clear() + reinsert, OR
        // - In-place page compaction
        Self::vacuum_legacy_table(db, table_name, oldest_tx)
    }

    /// Vacuum legacy Vec<Row> storage
    ///
    /// Simple implementation: Vec::retain() to filter out dead tuples
    fn vacuum_legacy_table(
        db: &mut Database,
        table_name: &str,
        oldest_tx: u64,
    ) -> Result<usize, DatabaseError> {
        let table = db.get_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        let before = table.rows.len();

        // Retain only alive rows (remove dead ones)
        table.rows.retain(|row| !row.is_dead(oldest_tx));

        let after = table.rows.len();
        Ok(before - after)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Table, Column, DataType, Value, Row};

    #[test]
    fn test_vacuum_legacy_removes_dead_tuples() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();

        // Advance tx_manager past the dead tuple xmax values
        for _ in 0..200 {
            tx_manager.begin_transaction();
        }

        // Create table
        let table = Table::new("users".to_string(), vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            },
        ]);
        db.create_table(table).unwrap();

        // Add rows: 1 alive, 2 dead
        let table = db.get_table_mut("users").unwrap();
        table.rows.push(Row {
            values: vec![Value::Integer(1)],
            xmin: 100,
            xmax: None, // Alive
        });
        table.rows.push(Row {
            values: vec![Value::Integer(2)],
            xmin: 100,
            xmax: Some(150), // Dead (tx_manager is at 201, so 150 < 201)
        });
        table.rows.push(Row {
            values: vec![Value::Integer(3)],
            xmin: 100,
            xmax: Some(160), // Dead (160 < 201)
        });

        assert_eq!(table.rows.len(), 3);

        // Vacuum - should remove dead tuples with xmax < current_tx_id
        let result = VacuumExecutor::vacuum(&mut db, Some("users".to_string()), &tx_manager, None);

        assert!(result.is_ok());
        let table = db.get_table("users").unwrap();
        assert_eq!(table.rows.len(), 1); // Only alive row remains
        assert_eq!(table.rows[0].values[0], Value::Integer(1));
    }

    #[test]
    fn test_vacuum_preserves_alive_tuples() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();

        // Create table
        let table = Table::new("users".to_string(), vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ]);
        db.create_table(table).unwrap();

        // Add all alive rows
        let table = db.get_table_mut("users").unwrap();
        table.rows.push(Row {
            values: vec![Value::Integer(1)],
            xmin: 100,
            xmax: None,
        });
        table.rows.push(Row {
            values: vec![Value::Integer(2)],
            xmin: 100,
            xmax: None,
        });

        // Vacuum should not remove anything
        let result = VacuumExecutor::vacuum(&mut db, Some("users".to_string()), &tx_manager, None);

        assert!(result.is_ok());
        let table = db.get_table("users").unwrap();
        assert_eq!(table.rows.len(), 2); // All rows remain
    }

    #[test]
    fn test_vacuum_all_tables() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();

        // Advance tx_manager past dead tuples
        for _ in 0..200 {
            tx_manager.begin_transaction();
        }

        // Create two tables
        for table_name in &["t1", "t2"] {
            let table = Table::new(table_name.to_string(), vec![
                Column {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    foreign_key: None,
                },
            ]);
            db.create_table(table).unwrap();

            // Add 1 alive + 1 dead row to each table
            let table = db.get_table_mut(*table_name).unwrap();
            table.rows.push(Row {
                values: vec![Value::Integer(1)],
                xmin: 100,
                xmax: None,
            });
            table.rows.push(Row {
                values: vec![Value::Integer(2)],
                xmin: 100,
                xmax: Some(150), // Dead (150 < 201)
            });
        }

        // Vacuum all tables (None = all)
        let result = VacuumExecutor::vacuum(&mut db, None, &tx_manager, None);

        assert!(result.is_ok());
        // Each table should have 1 row remaining
        assert_eq!(db.get_table("t1").unwrap().rows.len(), 1);
        assert_eq!(db.get_table("t2").unwrap().rows.len(), 1);
    }
}
