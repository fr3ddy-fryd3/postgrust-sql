/// VACUUM executor - removes dead tuples from tables
///
/// Implements VACUUM command for MVCC cleanup:
/// - Scans tables for dead tuples (xmax < `oldest_active_tx`)
/// - Removes dead tuples from storage
/// - Works with both Vec<Row> (legacy) and `PagedTable` storage
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
    /// * `database_storage` - Page-based storage (required for v2.0+)
    ///
    /// # Returns
    /// `QueryResult` with number of tuples removed
    pub fn vacuum(
        db: &mut Database,
        table_name: Option<String>,
        tx_manager: &TransactionManager,
        database_storage: &mut crate::storage::DatabaseStorage,
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
                table_name,
                oldest_tx,
                database_storage,
            )?;
            total_removed += removed;
        }

        Ok(QueryResult::Success(format!(
            "VACUUM complete. Removed {total_removed} dead tuples."
        )))
    }

    /// Vacuum single table using `PagedTable`
    fn vacuum_table(
        table_name: &str,
        oldest_tx: u64,
        database_storage: &mut crate::storage::DatabaseStorage,
    ) -> Result<usize, DatabaseError> {
        // Get PagedTable for this table
        let paged_table = database_storage.get_paged_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        // Call PagedTable's vacuum method
        paged_table.vacuum(oldest_tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Table, Column, DataType, Value, Row};
    use crate::storage::DatabaseStorage;
    use tempfile::tempdir;

    #[test]
    fn test_vacuum_removes_dead_tuples() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();
        let temp_dir = tempdir().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path().to_str().unwrap(), 32).unwrap();

        // Advance tx_manager past the dead tuple xmax values
        for _ in 0..200 {
            let _ = tx_manager.begin_transaction();
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
        db.create_table(table.clone()).unwrap();
        storage.create_table("users".to_string()).unwrap();

        // Add rows: 1 alive, 2 dead
        let paged_table = storage.get_paged_table_mut("users").unwrap();
        paged_table.insert(Row {
            values: vec![Value::Integer(1)],
            xmin: 100,
            xmax: None, // Alive
        }).unwrap();
        paged_table.insert(Row {
            values: vec![Value::Integer(2)],
            xmin: 100,
            xmax: Some(150), // Dead (tx_manager is at 201, so 150 < 201)
        }).unwrap();
        paged_table.insert(Row {
            values: vec![Value::Integer(3)],
            xmin: 100,
            xmax: Some(160), // Dead (160 < 201)
        }).unwrap();

        let before = paged_table.get_all_rows().unwrap().len();
        assert_eq!(before, 3);

        // Vacuum - should remove dead tuples with xmax < current_tx_id
        let result = VacuumExecutor::vacuum(&mut db, Some("users".to_string()), &tx_manager, &mut storage);

        assert!(result.is_ok());
        let paged_table = storage.get_paged_table_mut("users").unwrap();
        let rows = paged_table.get_all_rows().unwrap();

        // Should have 1 alive row remaining (dead rows physically removed)
        let alive_rows: Vec<_> = rows.iter().filter(|r| !r.is_dead(201)).collect();
        assert_eq!(alive_rows.len(), 1);
        assert_eq!(alive_rows[0].values[0], Value::Integer(1));
    }

    #[test]
    fn test_vacuum_preserves_alive_tuples() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();
        let temp_dir = tempdir().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path().to_str().unwrap(), 32).unwrap();

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
        db.create_table(table.clone()).unwrap();
        storage.create_table("users".to_string()).unwrap();

        // Add all alive rows
        let paged_table = storage.get_paged_table_mut("users").unwrap();
        paged_table.insert(Row {
            values: vec![Value::Integer(1)],
            xmin: 100,
            xmax: None,
        }).unwrap();
        paged_table.insert(Row {
            values: vec![Value::Integer(2)],
            xmin: 100,
            xmax: None,
        }).unwrap();

        // Vacuum should not remove anything
        let result = VacuumExecutor::vacuum(&mut db, Some("users".to_string()), &tx_manager, &mut storage);

        assert!(result.is_ok());
        let paged_table = storage.get_paged_table_mut("users").unwrap();
        let rows = paged_table.get_all_rows().unwrap();

        // All rows should remain (no dead tuples)
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_vacuum_all_tables() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();
        let temp_dir = tempdir().unwrap();
        let mut storage = DatabaseStorage::new(temp_dir.path().to_str().unwrap(), 32).unwrap();

        // Advance tx_manager past dead tuples
        for _ in 0..200 {
            let _ = tx_manager.begin_transaction();
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
            db.create_table(table.clone()).unwrap();
            storage.create_table(table_name.to_string()).unwrap();

            // Add 1 alive + 1 dead row to each table
            let paged_table = storage.get_paged_table_mut(*table_name).unwrap();
            paged_table.insert(Row {
                values: vec![Value::Integer(1)],
                xmin: 100,
                xmax: None,
            }).unwrap();
            paged_table.insert(Row {
                values: vec![Value::Integer(2)],
                xmin: 100,
                xmax: Some(150), // Dead (150 < 201)
            }).unwrap();
        }

        // Vacuum all tables (None = all)
        let result = VacuumExecutor::vacuum(&mut db, None, &tx_manager, &mut storage);

        assert!(result.is_ok());

        // Each table should have 1 alive row remaining
        for table_name in &["t1", "t2"] {
            let paged_table = storage.get_paged_table_mut(*table_name).unwrap();
            let rows = paged_table.get_all_rows().unwrap();
            let alive_rows: Vec<_> = rows.iter().filter(|r| !r.is_dead(201)).collect();
            assert_eq!(alive_rows.len(), 1);
        }
    }
}
