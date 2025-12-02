/// DML (Data Manipulation Language) operations
///
/// INSERT, UPDATE, DELETE using RowStorage abstraction.
/// This allows seamless operation with both Vec<Row> and PagedTable.

use crate::types::{Database, DatabaseError, Row, Value, Column, DataType};
use crate::parser::{Statement, Condition};
use crate::storage::StorageEngine;
use crate::transaction::TransactionManager;
use super::storage_adapter::{RowStorage, LegacyStorage};
use super::legacy_executor::QueryResult;
use super::conditions::ConditionEvaluator;

pub struct DmlExecutor;

impl DmlExecutor {
    /// Execute INSERT statement using RowStorage abstraction
    ///
    /// This version uses RowStorage trait, allowing it to work with either:
    /// - LegacyStorage (Vec<Row>) - current default
    /// - PagedStorage (PagedTable) - new high-performance backend
    ///
    /// Borrow-checker friendly: accepts table parts separately instead of &mut Database
    pub fn insert_with_storage<S: RowStorage>(
        table_columns: &[Column],
        table_sequences: &std::collections::HashMap<String, i64>,
        sequences_mut: &mut std::collections::HashMap<String, i64>,
        all_tables: &std::collections::HashMap<String, crate::types::Table>,
        table_name: &str,
        columns: Option<Vec<String>>,
        values: Vec<Value>,
        storage: &mut S,
        storage_engine: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        // Reorder values to match table schema if columns specified
        let mut ordered_values = Self::reorder_values(table_columns, columns, values)?;

        // Handle SERIAL/BIGSERIAL auto-generation
        Self::handle_serial_columns(table_columns, table_sequences, &mut ordered_values);

        // Validate types, VARCHAR lengths, CHAR padding, ENUM values
        Self::validate_and_coerce_types(table_columns, &mut ordered_values)?;

        // Validate foreign key constraints
        Self::validate_foreign_keys_with_tables(all_tables, table_columns, &ordered_values, tx_manager)?;

        // Validate UNIQUE constraints
        Self::validate_unique_constraints(table_columns, &ordered_values, storage, tx_manager)?;

        // Create row with MVCC
        let current_tx_id = tx_manager.current_tx_id();
        let row = Row::new_with_xmin(ordered_values.clone(), current_tx_id);

        // Log to WAL before executing
        if let Some(se) = storage_engine {
            se.log_insert(table_name, &row)?;
        }

        // Insert using RowStorage abstraction
        storage.insert(row)?;

        // Update sequences for SERIAL columns (using mutable reference)
        for (idx, col) in table_columns.iter().enumerate() {
            if matches!(col.data_type, DataType::Serial | DataType::BigSerial) {
                if let Value::Integer(val) = ordered_values[idx] {
                    let current_seq = sequences_mut.get(&col.name).copied().unwrap_or(1);
                    sequences_mut.insert(col.name.clone(), current_seq.max(val + 1));
                }
            }
        }

        Ok(QueryResult::Success("1 row inserted".to_string()))
    }

    /// Reorder values to match table schema when columns are specified
    fn reorder_values(
        table_columns: &[Column],
        columns: Option<Vec<String>>,
        values: Vec<Value>,
    ) -> Result<Vec<Value>, DatabaseError> {
        if let Some(col_names) = columns {
            let mut ordered_values = vec![Value::Null; table_columns.len()];
            for (col_name, value) in col_names.iter().zip(values.iter()) {
                let idx = table_columns
                    .iter()
                    .position(|c| &c.name == col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;
                ordered_values[idx] = value.clone();
            }
            Ok(ordered_values)
        } else {
            Ok(values)
        }
    }

    /// Handle SERIAL/BIGSERIAL auto-generation for NULL values
    fn handle_serial_columns(
        columns: &[Column],
        sequences: &std::collections::HashMap<String, i64>,
        values: &mut [Value],
    ) {
        for (idx, col) in columns.iter().enumerate() {
            if matches!(col.data_type, crate::types::DataType::Serial | crate::types::DataType::BigSerial) {
                if matches!(values[idx], Value::Null) {
                    let seq_value = sequences.get(&col.name).copied().unwrap_or(1);
                    values[idx] = Value::Integer(seq_value);
                }
            }
        }
    }

    /// Validate and coerce value types (VARCHAR, CHAR, ENUM)
    fn validate_and_coerce_types(
        columns: &[Column],
        values: &mut [Value],
    ) -> Result<(), DatabaseError> {
        for (idx, col) in columns.iter().enumerate() {
            let value = &mut values[idx];

            // Validate VARCHAR length
            if let crate::types::DataType::Varchar { max_length } = col.data_type {
                if let Value::Text(s) = value {
                    if s.len() > max_length {
                        return Err(DatabaseError::ParseError(format!(
                            "Value too long for column '{}': {} exceeds VARCHAR({})",
                            col.name, s.len(), max_length
                        )));
                    }
                }
            }

            // Validate and pad CHAR length
            if let crate::types::DataType::Char { length } = col.data_type {
                match value {
                    Value::Text(s) | Value::Char(s) => {
                        if s.len() > length {
                            return Err(DatabaseError::ParseError(format!(
                                "Value too long for column '{}': {} exceeds CHAR({})",
                                col.name, s.len(), length
                            )));
                        }
                        *value = Value::Char(format!("{:<width$}", s, width = length));
                    }
                    _ => {}
                }
            }

            // Validate ENUM values
            if let crate::types::DataType::Enum { ref name, ref values } = col.data_type {
                match value {
                    Value::Text(s) => {
                        if !values.contains(s) {
                            return Err(DatabaseError::ParseError(format!(
                                "Invalid value '{}' for ENUM type '{}'. Expected one of: {:?}",
                                s, name, values
                            )));
                        }
                        *value = Value::Enum(name.clone(), s.clone());
                    }
                    Value::Enum(_, val) => {
                        if !values.contains(val) {
                            return Err(DatabaseError::ParseError(format!(
                                "Invalid value '{}' for ENUM type '{}'",
                                val, name
                            )));
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// Validate foreign key constraints (using HashMap<String, Table>)
    ///
    /// Borrow-checker friendly version that accepts all_tables instead of &Database
    fn validate_foreign_keys_with_tables(
        all_tables: &std::collections::HashMap<String, crate::types::Table>,
        columns: &[Column],
        values: &[Value],
        tx_manager: &TransactionManager,
    ) -> Result<(), DatabaseError> {
        for (idx, col) in columns.iter().enumerate() {
            if let Some(ref fk) = col.foreign_key {
                let value = &values[idx];

                // NULL values are allowed unless column is NOT NULL
                if matches!(value, Value::Null) {
                    if !col.nullable {
                        return Err(DatabaseError::ForeignKeyViolation(
                            format!("Column '{}' cannot be NULL", col.name)
                        ));
                    }
                    continue;
                }

                // Check if value exists in referenced table
                let ref_table = all_tables
                    .get(&fk.referenced_table)
                    .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                        format!("Referenced table '{}' does not exist", fk.referenced_table)
                    ))?;

                let ref_col_idx = ref_table
                    .get_column_index(&fk.referenced_column)
                    .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                        format!("Referenced column '{}' not found", fk.referenced_column)
                    ))?;

                let current_tx_id = tx_manager.current_tx_id();
                let exists = ref_table.rows.iter()
                    .any(|row| row.is_visible(current_tx_id) && &row.values[ref_col_idx] == value);

                if !exists {
                    return Err(DatabaseError::ForeignKeyViolation(
                        format!("Foreign key constraint violation: value {:?} not found in {}.{}",
                                value, fk.referenced_table, fk.referenced_column)
                    ));
                }
            }
        }
        Ok(())
    }

    /// Validate foreign key constraints (legacy version using &Database)
    #[allow(dead_code)]
    fn validate_foreign_keys(
        db: &Database,
        columns: &[Column],
        values: &[Value],
        tx_manager: &TransactionManager,
    ) -> Result<(), DatabaseError> {
        Self::validate_foreign_keys_with_tables(&db.tables, columns, values, tx_manager)
    }

    /// Validate UNIQUE constraints using RowStorage
    fn validate_unique_constraints<S: RowStorage>(
        columns: &[Column],
        values: &[Value],
        storage: &S,
        tx_manager: &TransactionManager,
    ) -> Result<(), DatabaseError> {
        let all_rows = storage.get_all()?;
        let current_tx_id = tx_manager.current_tx_id();

        for (idx, col) in columns.iter().enumerate() {
            if col.unique || col.primary_key {
                let value = &values[idx];

                // NULL values are allowed in UNIQUE columns
                if matches!(value, Value::Null) {
                    continue;
                }

                // Check if value already exists
                let exists = all_rows.iter()
                    .any(|row| row.is_visible(current_tx_id) && &row.values[idx] == value);

                if exists {
                    return Err(DatabaseError::UniqueViolation(
                        format!("UNIQUE constraint violation: value {:?} already exists in column '{}'",
                                value, col.name)
                    ));
                }
            }
        }
        Ok(())
    }

    /// Update SERIAL sequences after successful insert
    fn update_serial_sequences(
        table: &mut crate::types::Table,
        values: &[Value],
    ) {
        for (idx, col) in table.columns.iter().enumerate() {
            if matches!(col.data_type, crate::types::DataType::Serial | crate::types::DataType::BigSerial) {
                let val = match values[idx] {
                    Value::Integer(v) => v,
                    Value::SmallInt(v) => v as i64,
                    _ => continue,
                };
                let current_seq = table.sequences.get(&col.name).copied().unwrap_or(1);
                let new_seq = std::cmp::max(current_seq, val + 1);
                table.sequences.insert(col.name.clone(), new_seq);
            }
        }
    }

    /// Execute UPDATE statement using RowStorage abstraction
    ///
    /// Updates rows matching the filter condition.
    pub fn update_with_storage<S: RowStorage>(
        table_columns: &[Column],
        assignments: Vec<(String, Value)>,
        filter: Option<Condition>,
        storage: &mut S,
        storage_engine: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
        table_name: &str,
    ) -> Result<QueryResult, DatabaseError> {
        // Pre-calculate column indices
        let column_updates: Vec<(usize, Value)> = assignments
            .into_iter()
            .map(|(col_name, value)| {
                let idx = table_columns
                    .iter()
                    .position(|c| c.name == col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;
                Ok((idx, value))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

        // Get current transaction ID for MVCC
        let current_tx_id = tx_manager.current_tx_id();

        // Define predicate and updater closures
        let predicate = |row: &Row| -> bool {
            if let Some(ref cond) = filter {
                ConditionEvaluator::evaluate_with_columns(table_columns, row, cond).unwrap_or(false)
            } else {
                true
            }
        };

        let updater = |row: &Row| -> Row {
            let mut new_values = row.values.clone();
            for (idx, new_value) in &column_updates {
                new_values[*idx] = new_value.clone();
            }
            Row::new_with_xmin(new_values, current_tx_id)
        };

        // Execute update (MVCC: mark old + insert new versions)
        let updated_count = storage.update_where(predicate, updater, current_tx_id)?;

        // TODO: WAL logging
        if let Some(_se) = storage_engine {
            // storage_engine.log_update(table_name, ...)?;
        }

        Ok(QueryResult::Success(format!("{} row(s) updated", updated_count)))
    }

    /// Execute DELETE statement using RowStorage abstraction
    ///
    /// Deletes rows matching the filter condition.
    pub fn delete_with_storage<S: RowStorage>(
        table_columns: &[Column],
        filter: Option<Condition>,
        storage: &mut S,
        storage_engine: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
        table_name: &str,
    ) -> Result<QueryResult, DatabaseError> {
        // Get current transaction ID for MVCC
        let current_tx_id = tx_manager.current_tx_id();

        // Define predicate closure
        let predicate = |row: &Row| -> bool {
            // Check MVCC visibility first
            if !row.is_visible(current_tx_id) {
                return false;
            }

            if let Some(ref cond) = filter {
                ConditionEvaluator::evaluate_with_columns(table_columns, row, cond).unwrap_or(false)
            } else {
                true
            }
        };

        // Execute delete (MVCC: mark with xmax instead of physical removal)
        let deleted_count = storage.delete_where(predicate, current_tx_id)?;

        // TODO: WAL logging
        if let Some(_se) = storage_engine {
            // storage_engine.log_delete(table_name, ...)?;
        }

        Ok(QueryResult::Success(format!("{} row(s) deleted", deleted_count)))
    }

    /// Convenience wrapper that uses LegacyStorage (Vec<Row>)
    ///
    /// This maintains backward compatibility with existing code.
    ///
    /// Note: This function needs to be restructured to avoid borrow checker issues.
    /// For now, users should call insert_with_storage directly.
    #[allow(dead_code)]
    pub fn insert_legacy(
        db: &mut Database,
        table_name: &str,
        columns: Option<Vec<String>>,
        values: Vec<Value>,
        storage_engine: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        // TODO: This needs refactoring due to borrow checker constraints
        // For now, legacy executor continues to use direct table.rows access
        Err(DatabaseError::ParseError("Use insert_with_storage directly".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Table, Column, DataType};

    #[test]
    fn test_reorder_values() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];

        let result = DmlExecutor::reorder_values(
            &columns,
            Some(vec!["name".to_string(), "id".to_string()]),
            vec![Value::Text("Alice".to_string()), Value::Integer(1)],
        ).unwrap();

        assert_eq!(result[0], Value::Integer(1));
        assert_eq!(result[1], Value::Text("Alice".to_string()));
    }

    #[test]
    fn test_handle_serial_columns() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Serial,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ];

        let mut sequences = std::collections::HashMap::new();
        sequences.insert("id".to_string(), 5);

        let mut values = vec![Value::Null, Value::Text("Alice".to_string())];
        DmlExecutor::handle_serial_columns(&columns, &sequences, &mut values);

        assert_eq!(values[0], Value::Integer(5));
        assert_eq!(values[1], Value::Text("Alice".to_string()));
    }
}
