use crate::parser::{
    AggregateFunction, ColumnDef, Condition, CountTarget, SelectColumn, SortOrder, Statement,
};
use crate::storage::StorageEngine;
use crate::transaction::TransactionManager;
use crate::types::{Column, Database, DatabaseError, DataType, Row, Table, Value};

pub struct QueryExecutor;

#[derive(Debug)]
pub enum QueryResult {
    Success(String),
    Rows(Vec<Vec<String>>, Vec<String>), // (rows, column_names)
}

impl QueryExecutor {
    /// Executes a query with automatic WAL logging and MVCC support
    pub fn execute(
        db: &mut Database,
        stmt: Statement,
        storage: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        match stmt {
            Statement::CreateTable { name, columns } => {
                Self::create_table(db, name, columns, storage)
            }
            Statement::DropTable { name } => Self::drop_table(db, name, storage),
            Statement::Insert {
                table,
                columns,
                values,
            } => Self::insert(db, table, columns, values, storage, tx_manager),
            Statement::Select {
                distinct,
                columns,
                from,
                joins,
                filter,
                group_by,
                order_by,
                limit,
                offset,
            } => Self::select(db, distinct, columns, from, joins, filter, group_by, order_by, limit, offset, tx_manager),
            Statement::Update {
                table,
                assignments,
                filter,
            } => Self::update(db, table, assignments, filter, storage, tx_manager),
            Statement::Delete { from, filter } => Self::delete(db, from, filter, storage, tx_manager),
            Statement::ShowTables => Self::show_tables(db),
            Statement::Begin | Statement::Commit | Statement::Rollback => {
                // Transaction commands should be handled at the server level
                Err(DatabaseError::ParseError(
                    "Transaction commands should not reach executor".to_string(),
                ))
            }
            // User management commands - handled at server level
            Statement::CreateUser { .. } | Statement::DropUser { .. } | Statement::AlterUser { .. } => {
                Err(DatabaseError::ParseError(
                    "User management commands should be handled at server level".to_string(),
                ))
            }
            // Database management commands - handled at server level
            Statement::CreateDatabase { .. } | Statement::DropDatabase { .. } => {
                Err(DatabaseError::ParseError(
                    "Database management commands should be handled at server level".to_string(),
                ))
            }
            // Privilege commands - handled at server level
            Statement::Grant { .. } | Statement::Revoke { .. } => {
                Err(DatabaseError::ParseError(
                    "Privilege management commands should be handled at server level".to_string(),
                ))
            }
            // Metadata queries - handled at server level
            Statement::ShowUsers | Statement::ShowDatabases => {
                Err(DatabaseError::ParseError(
                    "Metadata queries should be handled at server level".to_string(),
                ))
            }
            // Type management
            Statement::CreateType { name, values } => {
                db.create_enum(name.clone(), values)?;
                Ok(QueryResult::Success(format!("Type '{}' created successfully", name)))
            }
        }
    }

    fn create_table(
        db: &mut Database,
        name: String,
        column_defs: Vec<ColumnDef>,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        let columns: Vec<Column> = column_defs
            .into_iter()
            .map(|def| {
                // Resolve enum types: if data_type is Enum with empty values, look up in db.enums
                let data_type = match def.data_type {
                    DataType::Enum { ref name, ref values } if values.is_empty() => {
                        // Look up enum values from database
                        if let Some(enum_values) = db.enums.get(name) {
                            DataType::Enum {
                                name: name.clone(),
                                values: enum_values.clone(),
                            }
                        } else {
                            return Err(DatabaseError::ParseError(format!(
                                "Unknown enum type '{}'", name
                            )));
                        }
                    }
                    other => other,
                };

                Ok(Column {
                    name: def.name.clone(),
                    data_type,
                    nullable: def.nullable,
                    primary_key: def.primary_key,
                    unique: def.unique,
                    foreign_key: def.foreign_key.clone(),
                })
            })
            .collect::<Result<Vec<Column>, DatabaseError>>()?;

        // Validate foreign key references
        for col in &columns {
            if let Some(ref fk) = col.foreign_key {
                // Check if referenced table exists
                let ref_table = db
                    .get_table(&fk.referenced_table)
                    .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                        format!("Referenced table '{}' does not exist", fk.referenced_table)
                    ))?;

                // Check if referenced column exists and is a primary key
                let ref_col = ref_table.columns.iter().find(|c| c.name == fk.referenced_column)
                    .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                        format!("Referenced column '{}' does not exist in table '{}'",
                                fk.referenced_column, fk.referenced_table)
                    ))?;

                if !ref_col.primary_key {
                    return Err(DatabaseError::ForeignKeyViolation(
                        format!("Referenced column '{}' must be a primary key", fk.referenced_column)
                    ));
                }
            }
        }

        let table = Table::new(name.clone(), columns);

        // Log to WAL before executing
        if let Some(storage) = storage {
            storage.log_create_table(&table)?;
        }

        db.create_table(table)?;
        Ok(QueryResult::Success(format!(
            "Table '{}' created successfully",
            name
        )))
    }

    fn drop_table(
        db: &mut Database,
        name: String,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        // Log to WAL before executing
        if let Some(storage) = storage {
            storage.log_drop_table(&name)?;
        }

        db.drop_table(&name)?;
        Ok(QueryResult::Success(format!(
            "Table '{}' dropped successfully",
            name
        )))
    }

    fn insert(
        db: &mut Database,
        table_name: String,
        columns: Option<Vec<String>>,
        values: Vec<Value>,
        storage: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db
            .get_table(&table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.clone()))?;

        // If columns are specified, reorder values to match table schema
        let mut ordered_values = if let Some(col_names) = columns {
            let mut ordered_values = vec![Value::Null; table.columns.len()];
            for (col_name, value) in col_names.iter().zip(values.iter()) {
                let idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;
                ordered_values[idx] = value.clone();
            }
            ordered_values
        } else {
            values
        };

        // Handle SERIAL/BIGSERIAL columns - auto-generate values for NULL/missing columns
        for (idx, col) in table.columns.iter().enumerate() {
            if matches!(col.data_type, crate::types::DataType::Serial | crate::types::DataType::BigSerial) {
                // If value is NULL or not provided, use sequence
                if matches!(ordered_values[idx], Value::Null) {
                    let seq_value = table.sequences.get(&col.name).copied().unwrap_or(1);
                    ordered_values[idx] = Value::Integer(seq_value);
                }
            }
        }

        // Validate and coerce types
        for (idx, col) in table.columns.iter().enumerate() {
            let value = &mut ordered_values[idx];

            // Validate VARCHAR length
            if let crate::types::DataType::Varchar { max_length } = col.data_type {
                match value {
                    Value::Text(s) => {
                        if s.len() > max_length {
                            return Err(DatabaseError::ParseError(format!(
                                "Value too long for column '{}': {} exceeds VARCHAR({})",
                                col.name, s.len(), max_length
                            )));
                        }
                    }
                    _ => {}
                }
            }

            // Validate and pad CHAR length
            if let crate::types::DataType::Char { length } = col.data_type {
                match value {
                    Value::Text(s) => {
                        if s.len() > length {
                            return Err(DatabaseError::ParseError(format!(
                                "Value too long for column '{}': {} exceeds CHAR({})",
                                col.name, s.len(), length
                            )));
                        }
                        // Pad with spaces if needed
                        *value = Value::Char(format!("{:<width$}", s, width = length));
                    }
                    Value::Char(s) => {
                        if s.len() != length {
                            *value = Value::Char(format!("{:<width$}", s, width = length));
                        }
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

        // Validate foreign key constraints
        for (idx, col) in table.columns.iter().enumerate() {
            if let Some(ref fk) = col.foreign_key {
                let value = &ordered_values[idx];

                // NULL values are allowed in foreign keys (unless column is NOT NULL)
                if matches!(value, Value::Null) {
                    if !col.nullable {
                        return Err(DatabaseError::ForeignKeyViolation(
                            format!("Column '{}' cannot be NULL", col.name)
                        ));
                    }
                    continue;
                }

                // Check if the value exists in the referenced table
                let ref_table = db
                    .get_table(&fk.referenced_table)
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

        // Validate UNIQUE constraints
        for (idx, col) in table.columns.iter().enumerate() {
            if col.unique || col.primary_key {
                let value = &ordered_values[idx];

                // NULL values are allowed in UNIQUE columns (unless NOT NULL)
                if matches!(value, Value::Null) {
                    continue;
                }

                // Check if value already exists in this column
                let current_tx_id = tx_manager.current_tx_id();
                let exists = table.rows.iter()
                    .any(|row| row.is_visible(current_tx_id) && &row.values[idx] == value);

                if exists {
                    return Err(DatabaseError::UniqueViolation(
                        format!("UNIQUE constraint violation: value {:?} already exists in column '{}'",
                                value, col.name)
                    ));
                }
            }
        }

        // Get current transaction ID for MVCC
        let current_tx_id = tx_manager.current_tx_id();
        let row = Row::new_with_xmin(ordered_values.clone(), current_tx_id);

        // Log to WAL before executing
        if let Some(storage) = storage {
            storage.log_insert(&table_name, &row)?;
        }

        let table = db.get_table_mut(&table_name).unwrap();
        table.insert(row)?;

        // Update sequences for SERIAL/BIGSERIAL columns
        for (idx, col) in table.columns.iter().enumerate() {
            if matches!(col.data_type, crate::types::DataType::Serial | crate::types::DataType::BigSerial) {
                if let Value::Integer(val) = ordered_values[idx] {
                    // Update sequence to max(current_seq, inserted_value + 1)
                    let current_seq = table.sequences.get(&col.name).copied().unwrap_or(1);
                    let new_seq = std::cmp::max(current_seq, val + 1);
                    table.sequences.insert(col.name.clone(), new_seq);
                } else if let Value::SmallInt(val) = ordered_values[idx] {
                    let current_seq = table.sequences.get(&col.name).copied().unwrap_or(1);
                    let new_seq = std::cmp::max(current_seq, val as i64 + 1);
                    table.sequences.insert(col.name.clone(), new_seq);
                }
            }
        }

        Ok(QueryResult::Success("1 row inserted".to_string()))
    }

    fn select(
        db: &Database,
        distinct: bool,
        columns: Vec<SelectColumn>,
        from: String,
        joins: Vec<crate::parser::JoinClause>,
        filter: Option<Condition>,
        group_by: Option<Vec<String>>,
        order_by: Option<(String, SortOrder)>,
        limit: Option<usize>,
        offset: Option<usize>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        // Check if this is a JOIN query
        if !joins.is_empty() {
            return Self::select_with_join(db, distinct, columns, from, joins, filter, order_by, limit, offset, tx_manager);
        }

        // Check if this is an aggregate query
        let has_aggregates = columns
            .iter()
            .any(|col| matches!(col, SelectColumn::Aggregate(_)));

        if group_by.is_some() {
            Self::select_with_group_by(db, distinct, columns, from, filter, group_by.unwrap(), order_by, limit, offset, tx_manager)
        } else if has_aggregates {
            Self::select_aggregate(db, distinct, columns, from, filter, tx_manager)
        } else {
            Self::select_regular(db, distinct, columns, from, filter, order_by, limit, offset, tx_manager)
        }
    }

    fn select_regular(
        db: &Database,
        distinct: bool,
        columns: Vec<SelectColumn>,
        from: String,
        filter: Option<Condition>,
        order_by: Option<(String, SortOrder)>,
        limit: Option<usize>,
        offset: Option<usize>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        // Extract regular column names
        let col_names: Vec<String> = columns
            .iter()
            .map(|col| match col {
                SelectColumn::Regular(name) => name.clone(),
                SelectColumn::Aggregate(_) => {
                    panic!("Aggregate in regular select should not happen")
                }
            })
            .collect();

        let is_select_all = col_names.len() == 1 && col_names[0] == "*";

        let column_indices: Vec<usize> = if is_select_all {
            (0..table.columns.len()).collect()
        } else {
            col_names
                .iter()
                .map(|col| {
                    table
                        .get_column_index(col)
                        .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col)))
                })
                .collect::<Result<Vec<_>, _>>()?
        };

        let column_names: Vec<String> = column_indices
            .iter()
            .map(|&idx| table.columns[idx].name.clone())
            .collect();

        // Get current transaction ID for visibility checks (MVCC)
        let current_tx_id = tx_manager.current_tx_id();

        // Collect rows with their original indices (for sorting)
        let mut rows_with_data: Vec<(&Row, Vec<String>)> = Vec::new();

        for row in &table.rows {
            // MVCC: Check row visibility
            if !row.is_visible(current_tx_id) {
                continue;
            }

            if let Some(ref cond) = filter {
                if !Self::evaluate_condition(table, row, cond)? {
                    continue;
                }
            }

            let result_row: Vec<String> = column_indices
                .iter()
                .map(|&idx| row.values[idx].to_string())
                .collect();
            rows_with_data.push((row, result_row));
        }

        // Apply ORDER BY if specified
        if let Some((sort_column, sort_order)) = order_by {
            let sort_col_idx = table
                .get_column_index(&sort_column)
                .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", sort_column)))?;

            rows_with_data.sort_by(|(row_a, _), (row_b, _)| {
                let val_a = &row_a.values[sort_col_idx];
                let val_b = &row_b.values[sort_col_idx];

                let cmp = match (val_a, val_b) {
                    (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
                    (Value::Real(a), Value::Real(b)) => {
                        if a < b {
                            std::cmp::Ordering::Less
                        } else if a > b {
                            std::cmp::Ordering::Greater
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    }
                    (Value::Text(a), Value::Text(b)) => a.cmp(b),
                    (Value::Boolean(a), Value::Boolean(b)) => a.cmp(b),
                    (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
                    (Value::Null, _) => std::cmp::Ordering::Less,
                    (_, Value::Null) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                };

                match sort_order {
                    SortOrder::Asc => cmp,
                    SortOrder::Desc => cmp.reverse(),
                }
            });
        }

        // Extract result rows
        let mut result_rows: Vec<Vec<String>> = rows_with_data
            .into_iter()
            .map(|(_, row_data)| row_data)
            .collect();

        // Apply DISTINCT if specified
        if distinct {
            use std::collections::HashSet;
            let mut seen: HashSet<Vec<String>> = HashSet::new();
            result_rows.retain(|row| seen.insert(row.clone()));
        }

        // Apply OFFSET
        if let Some(offset_val) = offset {
            result_rows = result_rows.into_iter().skip(offset_val).collect();
        }

        // Apply LIMIT
        if let Some(limit_val) = limit {
            result_rows.truncate(limit_val);
        }

        Ok(QueryResult::Rows(result_rows, column_names))
    }

    fn select_aggregate(
        db: &Database,
        _distinct: bool,
        columns: Vec<SelectColumn>,
        from: String,
        filter: Option<Condition>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        // Get current transaction ID for visibility checks (MVCC)
        let current_tx_id = tx_manager.current_tx_id();

        // Collect visible rows that match the filter
        let visible_rows: Vec<&Row> = table
            .rows
            .iter()
            .filter(|row| {
                // MVCC: Check row visibility
                if !row.is_visible(current_tx_id) {
                    return false;
                }

                // Apply filter
                if let Some(ref cond) = filter {
                    Self::evaluate_condition(table, row, cond).unwrap_or(false)
                } else {
                    true
                }
            })
            .collect();

        // Calculate aggregates
        let mut result_row = Vec::new();
        let mut column_names = Vec::new();

        for col in columns {
            match col {
                SelectColumn::Aggregate(agg_func) => {
                    let (value, name) = Self::compute_aggregate(&agg_func, table, &visible_rows)?;
                    result_row.push(value);
                    column_names.push(name);
                }
                SelectColumn::Regular(_) => {
                    return Err(DatabaseError::ParseError(
                        "Cannot mix aggregates with regular columns without GROUP BY".to_string(),
                    ));
                }
            }
        }

        Ok(QueryResult::Rows(vec![result_row], column_names))
    }

    fn compute_aggregate(
        agg_func: &AggregateFunction,
        table: &Table,
        rows: &[&Row],
    ) -> Result<(String, String), DatabaseError> {
        match agg_func {
            AggregateFunction::Count(target) => {
                let count = match target {
                    CountTarget::All => rows.len(),
                    CountTarget::Column(col_name) => {
                        let col_idx = table
                            .get_column_index(col_name)
                            .ok_or_else(|| {
                                DatabaseError::ParseError(format!("Unknown column: {}", col_name))
                            })?;
                        rows.iter()
                            .filter(|row| !matches!(row.values[col_idx], Value::Null))
                            .count()
                    }
                };
                Ok((count.to_string(), "count".to_string()))
            }
            AggregateFunction::Sum(col_name) => {
                let col_idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;

                let mut sum_int: Option<i64> = None;
                let mut sum_real: Option<f64> = None;

                for row in rows {
                    match &row.values[col_idx] {
                        Value::Integer(i) => {
                            sum_int = Some(sum_int.unwrap_or(0) + i);
                        }
                        Value::Real(r) => {
                            sum_real = Some(sum_real.unwrap_or(0.0) + r);
                        }
                        Value::Null => {}
                        _ => return Err(DatabaseError::TypeMismatch),
                    }
                }

                let value = if let Some(r) = sum_real {
                    r.to_string()
                } else if let Some(i) = sum_int {
                    i.to_string()
                } else {
                    "0".to_string()
                };

                Ok((value, format!("sum({})", col_name)))
            }
            AggregateFunction::Avg(col_name) => {
                let col_idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;

                let mut sum = 0.0;
                let mut count = 0;

                for row in rows {
                    match &row.values[col_idx] {
                        Value::Integer(i) => {
                            sum += *i as f64;
                            count += 1;
                        }
                        Value::Real(r) => {
                            sum += r;
                            count += 1;
                        }
                        Value::Null => {}
                        _ => return Err(DatabaseError::TypeMismatch),
                    }
                }

                let avg = if count > 0 { sum / count as f64 } else { 0.0 };
                Ok((avg.to_string(), format!("avg({})", col_name)))
            }
            AggregateFunction::Min(col_name) => {
                let col_idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;

                let mut min_val: Option<&Value> = None;

                for row in rows {
                    let val = &row.values[col_idx];
                    if matches!(val, Value::Null) {
                        continue;
                    }

                    if min_val.is_none() {
                        min_val = Some(val);
                    } else if let Some(current_min) = min_val {
                        let is_less = match (val, current_min) {
                            (Value::Integer(a), Value::Integer(b)) => a < b,
                            (Value::Real(a), Value::Real(b)) => a < b,
                            (Value::Text(a), Value::Text(b)) => a < b,
                            _ => false,
                        };
                        if is_less {
                            min_val = Some(val);
                        }
                    }
                }

                let value = min_val.map(|v| v.to_string()).unwrap_or_else(|| "NULL".to_string());
                Ok((value, format!("min({})", col_name)))
            }
            AggregateFunction::Max(col_name) => {
                let col_idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;

                let mut max_val: Option<&Value> = None;

                for row in rows {
                    let val = &row.values[col_idx];
                    if matches!(val, Value::Null) {
                        continue;
                    }

                    if max_val.is_none() {
                        max_val = Some(val);
                    } else if let Some(current_max) = max_val {
                        let is_greater = match (val, current_max) {
                            (Value::Integer(a), Value::Integer(b)) => a > b,
                            (Value::Real(a), Value::Real(b)) => a > b,
                            (Value::Text(a), Value::Text(b)) => a > b,
                            _ => false,
                        };
                        if is_greater {
                            max_val = Some(val);
                        }
                    }
                }

                let value = max_val.map(|v| v.to_string()).unwrap_or_else(|| "NULL".to_string());
                Ok((value, format!("max({})", col_name)))
            }
        }
    }

    fn select_with_group_by(
        db: &Database,
        distinct: bool,
        columns: Vec<SelectColumn>,
        from: String,
        filter: Option<Condition>,
        group_by: Vec<String>,
        order_by: Option<(String, SortOrder)>,
        limit: Option<usize>,
        offset: Option<usize>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        use std::collections::HashMap;

        let table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        // Get current transaction ID for MVCC visibility
        let current_tx_id = tx_manager.current_tx_id();

        // Get indices for GROUP BY columns
        let group_by_indices: Vec<usize> = group_by
            .iter()
            .map(|col| {
                table
                    .columns
                    .iter()
                    .position(|c| c.name == *col)
                    .ok_or_else(|| DatabaseError::ColumnNotFound(col.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Filter visible rows
        let visible_rows: Vec<&Row> = table
            .rows
            .iter()
            .filter(|row| {
                if !row.is_visible(current_tx_id) {
                    return false;
                }
                if let Some(ref f) = filter {
                    Self::evaluate_condition_with_columns(&table.columns, row, f).unwrap_or(false)
                } else {
                    true
                }
            })
            .collect();

        // Group rows by GROUP BY columns
        let mut groups: HashMap<Vec<String>, Vec<&Row>> = HashMap::new();
        for row in visible_rows {
            let key: Vec<String> = group_by_indices
                .iter()
                .map(|&idx| row.values[idx].to_string())
                .collect();
            groups.entry(key).or_insert_with(Vec::new).push(row);
        }

        // Build result rows
        let mut result_rows = Vec::new();
        let mut column_names = Vec::new();

        // Determine column names
        for col in &columns {
            match col {
                SelectColumn::Regular(name) => {
                    // Must be in GROUP BY list
                    if !group_by.contains(name) {
                        return Err(DatabaseError::ParseError(format!(
                            "Column '{}' must appear in GROUP BY clause or be used in an aggregate function",
                            name
                        )));
                    }
                    column_names.push(name.clone());
                }
                SelectColumn::Aggregate(agg_func) => {
                    let (_, name) = Self::compute_aggregate(agg_func, table, &[])?;
                    column_names.push(name);
                }
            }
        }

        // Compute result for each group
        for (group_key, group_rows) in groups {
            let mut row_values = Vec::new();

            for col in &columns {
                match col {
                    SelectColumn::Regular(name) => {
                        // Get value from group key
                        let idx = group_by.iter().position(|g| g == name).unwrap();
                        row_values.push(group_key[idx].clone());
                    }
                    SelectColumn::Aggregate(agg_func) => {
                        let (value, _) = Self::compute_aggregate(agg_func, table, &group_rows)?;
                        row_values.push(value);
                    }
                }
            }

            result_rows.push(row_values);
        }

        // Apply ORDER BY if specified
        if let Some((ref sort_column, sort_order)) = order_by {
            let sort_col_idx = column_names
                .iter()
                .position(|c| c == sort_column)
                .ok_or_else(|| DatabaseError::ColumnNotFound(sort_column.clone()))?;

            result_rows.sort_by(|row_a, row_b| {
                let val_a = &row_a[sort_col_idx];
                let val_b = &row_b[sort_col_idx];

                // Parse values for comparison (simplified - comparing as strings)
                let cmp = val_a.cmp(val_b);

                match sort_order {
                    crate::parser::SortOrder::Asc => cmp,
                    crate::parser::SortOrder::Desc => cmp.reverse(),
                }
            });
        }

        // Apply DISTINCT if specified
        if distinct {
            use std::collections::HashSet;
            let mut seen: HashSet<Vec<String>> = HashSet::new();
            result_rows.retain(|row| seen.insert(row.clone()));
        }

        // Apply OFFSET + LIMIT if specified
        if let Some(offset_val) = offset {
            result_rows = result_rows.into_iter().skip(offset_val).collect();
        }

        if let Some(limit_count) = limit {
            result_rows.truncate(limit_count);
        }

        Ok(QueryResult::Rows(result_rows, column_names))
    }

    fn update(
        db: &mut Database,
        table_name: String,
        assignments: Vec<(String, Value)>,
        filter: Option<Condition>,
        storage: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db
            .get_table_mut(&table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.clone()))?;

        // Pre-calculate column indices
        let column_updates: Vec<(usize, Value)> = assignments
            .into_iter()
            .map(|(col_name, value)| {
                let idx = table
                    .get_column_index(&col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))?;
                Ok((idx, value))
            })
            .collect::<Result<Vec<_>, DatabaseError>>()?;

        let mut updated_count = 0;

        // Clone columns for condition evaluation
        let columns = table.columns.clone();

        // Get current transaction ID for MVCC
        let current_tx_id = tx_manager.current_tx_id();

        // First pass: collect indices of rows to update and their new values
        let mut updates = Vec::new();
        for (row_index, row) in table.rows.iter().enumerate() {
            if let Some(ref cond) = filter {
                if !Self::evaluate_condition_with_columns(&columns, row, cond)? {
                    continue;
                }
            }

            // Create new row version with updates applied (MVCC)
            let mut new_values = row.values.clone();
            for (idx, new_value) in &column_updates {
                new_values[*idx] = new_value.clone();
            }
            let new_row = Row::new_with_xmin(new_values, current_tx_id);
            updates.push((row_index, new_row));
        }

        // Log to WAL before executing
        if let Some(storage) = storage {
            for (row_index, new_row) in &updates {
                storage.log_update(&table_name, *row_index, new_row)?;
            }
        }

        // Second pass: mark old rows as deleted and add new versions (MVCC)
        for (row_index, new_row) in updates {
            // Mark old row as deleted by this transaction
            table.rows[row_index].xmax = Some(current_tx_id);
            // Add new row version
            table.rows.push(new_row);
            updated_count += 1;
        }

        Ok(QueryResult::Success(format!(
            "{} row(s) updated",
            updated_count
        )))
    }

    fn delete(
        db: &mut Database,
        table_name: String,
        filter: Option<Condition>,
        storage: Option<&mut StorageEngine>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db
            .get_table_mut(&table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.clone()))?;

        // Get current transaction ID for MVCC
        let current_tx_id = tx_manager.current_tx_id();

        // Collect indices of rows to delete
        let indices_to_delete: Vec<usize> = if let Some(ref cond) = filter {
            let columns = table.columns.clone();
            table
                .rows
                .iter()
                .enumerate()
                .filter_map(|(idx, row)| {
                    if Self::evaluate_condition_with_columns(&columns, row, cond).unwrap_or(false) {
                        Some(idx)
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            (0..table.rows.len()).collect()
        };

        // Log to WAL before executing
        if let Some(storage) = storage {
            for &row_index in &indices_to_delete {
                storage.log_delete(&table_name, row_index)?;
            }
        }

        // MVCC: Mark rows as deleted instead of physically removing them
        for row_index in &indices_to_delete {
            table.rows[*row_index].xmax = Some(current_tx_id);
        }

        let deleted_count = indices_to_delete.len();

        Ok(QueryResult::Success(format!(
            "{} row(s) deleted",
            deleted_count
        )))
    }

    fn show_tables(db: &Database) -> Result<QueryResult, DatabaseError> {
        let table_names: Vec<Vec<String>> = db
            .tables
            .keys()
            .map(|name| vec![name.clone()])
            .collect();

        if table_names.is_empty() {
            return Ok(QueryResult::Success("No tables found".to_string()));
        }

        Ok(QueryResult::Rows(
            table_names,
            vec!["Tables".to_string()],
        ))
    }

    fn evaluate_condition(
        table: &Table,
        row: &Row,
        condition: &Condition,
    ) -> Result<bool, DatabaseError> {
        Self::evaluate_condition_with_columns(&table.columns, row, condition)
    }

    fn evaluate_condition_with_columns(
        columns: &[Column],
        row: &Row,
        condition: &Condition,
    ) -> Result<bool, DatabaseError> {
        let get_column_index = |col_name: &str| -> Result<usize, DatabaseError> {
            columns
                .iter()
                .position(|c| c.name == col_name)
                .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))
        };

        match condition {
            Condition::Equals(col, val) => {
                let idx = get_column_index(col)?;
                Ok(&row.values[idx] == val)
            }
            Condition::NotEquals(col, val) => {
                let idx = get_column_index(col)?;
                Ok(&row.values[idx] != val)
            }
            Condition::GreaterThan(col, val) => {
                let idx = get_column_index(col)?;
                match (&row.values[idx], val) {
                    (Value::Integer(a), Value::Integer(b)) => Ok(a > b),
                    (Value::Real(a), Value::Real(b)) => Ok(a > b),
                    _ => Err(DatabaseError::TypeMismatch),
                }
            }
            Condition::LessThan(col, val) => {
                let idx = get_column_index(col)?;
                match (&row.values[idx], val) {
                    (Value::Integer(a), Value::Integer(b)) => Ok(a < b),
                    (Value::Real(a), Value::Real(b)) => Ok(a < b),
                    _ => Err(DatabaseError::TypeMismatch),
                }
            }
            Condition::And(left, right) => {
                let left_result = Self::evaluate_condition_with_columns(columns, row, left)?;
                let right_result = Self::evaluate_condition_with_columns(columns, row, right)?;
                Ok(left_result && right_result)
            }
            Condition::Or(left, right) => {
                let left_result = Self::evaluate_condition_with_columns(columns, row, left)?;
                let right_result = Self::evaluate_condition_with_columns(columns, row, right)?;
                Ok(left_result || right_result)
            }
        }
    }

    fn select_with_join(
        db: &Database,
        distinct: bool,
        _columns: Vec<SelectColumn>,
        from: String,
        joins: Vec<crate::parser::JoinClause>,
        _filter: Option<Condition>,
        _order_by: Option<(String, SortOrder)>,
        limit: Option<usize>,
        offset: Option<usize>,
        tx_manager: &TransactionManager,
    ) -> Result<QueryResult, DatabaseError> {
        use crate::parser::JoinType;

        // Get the main table
        let main_table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        let current_tx_id = tx_manager.current_tx_id();

        // For now, support only one JOIN
        if joins.len() != 1 {
            return Err(DatabaseError::ParseError(
                "Currently only one JOIN is supported".to_string(),
            ));
        }

        let join = &joins[0];

        // Get the joined table
        let join_table = db
            .get_table(&join.table)
            .ok_or_else(|| DatabaseError::TableNotFound(join.table.clone()))?;

        // Parse column references (table.column)
        let parse_col_ref = |ref_str: &str| -> Result<(String, String), DatabaseError> {
            let parts: Vec<&str> = ref_str.split('.').collect();
            if parts.len() != 2 {
                return Err(DatabaseError::ParseError(format!(
                    "Invalid column reference: {}",
                    ref_str
                )));
            }
            Ok((parts[0].to_string(), parts[1].to_string()))
        };

        let (left_table_name, left_col_name) = parse_col_ref(&join.on_left)?;
        let (right_table_name, right_col_name) = parse_col_ref(&join.on_right)?;

        // Determine which is main and which is joined
        let (main_join_col, join_join_col) = if left_table_name == from {
            (left_col_name, right_col_name)
        } else {
            (right_col_name, left_col_name)
        };

        let main_join_idx = main_table
            .get_column_index(&main_join_col)
            .ok_or_else(|| DatabaseError::ColumnNotFound(main_join_col.clone()))?;

        let join_join_idx = join_table
            .get_column_index(&join_join_col)
            .ok_or_else(|| DatabaseError::ColumnNotFound(join_join_col.clone()))?;

        // Build combined column names
        let mut combined_columns = Vec::new();
        for col in &main_table.columns {
            combined_columns.push(format!("{}.{}", from, col.name));
        }
        for col in &join_table.columns {
            combined_columns.push(format!("{}.{}", join.table, col.name));
        }

        // Perform the join
        let mut result_rows = Vec::new();

        for main_row in &main_table.rows {
            if !main_row.is_visible(current_tx_id) {
                continue;
            }

            let main_join_value = &main_row.values[main_join_idx];
            let mut matched = false;

            for join_row in &join_table.rows {
                if !join_row.is_visible(current_tx_id) {
                    continue;
                }

                let join_join_value = &join_row.values[join_join_idx];

                if main_join_value == join_join_value {
                    matched = true;
                    // Combine rows
                    let mut combined_row: Vec<String> = main_row
                        .values
                        .iter()
                        .map(|v| v.to_string())
                        .collect();
                    combined_row.extend(join_row.values.iter().map(|v| v.to_string()));
                    result_rows.push(combined_row);
                }
            }

            // For LEFT JOIN, include non-matching rows with NULLs
            if !matched && matches!(join.join_type, JoinType::Left) {
                let mut combined_row: Vec<String> = main_row
                    .values
                    .iter()
                    .map(|v| v.to_string())
                    .collect();
                combined_row.extend(vec!["NULL".to_string(); join_table.columns.len()]);
                result_rows.push(combined_row);
            }
        }

        // For RIGHT JOIN, include non-matching rows from join table
        if matches!(join.join_type, JoinType::Right) {
            for join_row in &join_table.rows {
                if !join_row.is_visible(current_tx_id) {
                    continue;
                }

                let join_join_value = &join_row.values[join_join_idx];
                let matched = main_table.rows.iter().any(|main_row| {
                    main_row.is_visible(current_tx_id)
                        && &main_row.values[main_join_idx] == join_join_value
                });

                if !matched {
                    let mut combined_row = vec!["NULL".to_string(); main_table.columns.len()];
                    combined_row.extend(join_row.values.iter().map(|v| v.to_string()));
                    result_rows.push(combined_row);
                }
            }
        }

        // Apply DISTINCT if specified
        if distinct {
            use std::collections::HashSet;
            let mut seen: HashSet<Vec<String>> = HashSet::new();
            result_rows.retain(|row| seen.insert(row.clone()));
        }

        // Apply OFFSET + LIMIT if specified
        if let Some(offset_val) = offset {
            result_rows = result_rows.into_iter().skip(offset_val).collect();
        }

        if let Some(limit_val) = limit {
            result_rows.truncate(limit_val);
        }

        // For simplicity, return all columns for now
        // TODO: Filter by selected columns
        Ok(QueryResult::Rows(result_rows, combined_columns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{SelectColumn, Statement};
    use crate::transaction::TransactionManager;
    use crate::types::{Column, DataType, Database, Value};

    fn create_test_table() -> Table {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
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
            Column {
                name: "age".to_string(),
                data_type: DataType::Integer,
                nullable: true,
                primary_key: false,
                unique: false,
                    foreign_key: None,
            },
        ];
        Table::new("users".to_string(), columns)
    }

    #[test]
    fn test_execute_create_table() {
        let mut db = Database::new("test".to_string());
        let stmt = Statement::CreateTable {
            name: "users".to_string(),
            columns: vec![
                crate::parser::ColumnDef {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: true,
                unique: false,
                    foreign_key: None,
                },
                crate::parser::ColumnDef {
                    name: "name".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
            ],
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));
        assert!(db.get_table("users").is_some());
    }

    #[test]
    fn test_execute_drop_table() {
        let mut db = Database::new("test".to_string());
        let table = create_test_table();
        db.create_table(table).unwrap();

        let stmt = Statement::DropTable {
            name: "users".to_string(),
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));
        assert!(db.get_table("users").is_none());
    }

    #[test]
    fn test_execute_insert() {
        let mut db = Database::new("test".to_string());
        let table = create_test_table();
        db.create_table(table).unwrap();

        let stmt = Statement::Insert {
            table: "users".to_string(),
            columns: Some(vec!["id".to_string(), "name".to_string(), "age".to_string()]),
            values: vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ],
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));

        let table = db.get_table("users").unwrap();
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn test_execute_insert_without_columns() {
        let mut db = Database::new("test".to_string());
        let table = create_test_table();
        db.create_table(table).unwrap();

        let stmt = Statement::Insert {
            table: "users".to_string(),
            columns: None,
            values: vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ],
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));

        let table = db.get_table("users").unwrap();
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn test_execute_select_all() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, columns) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(columns.len(), 3);
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_execute_select_with_filter() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::GreaterThan(
                "age".to_string(),
                Value::Integer(26),
            )),
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][1], "Alice");
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_execute_select_specific_columns() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("name".to_string()), SelectColumn::Regular("age".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, columns) => {
                assert_eq!(columns.len(), 2);
                assert_eq!(columns[0], "name");
                assert_eq!(columns[1], "age");
                assert_eq!(rows[0].len(), 2);
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_execute_update() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();

        // Insert initial data
        let mut table = create_test_table();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(1),
                    Value::Text("Alice".to_string()),
                    Value::Integer(30),
                ],
                1,
            ))
            .unwrap();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(2),
                    Value::Text("Bob".to_string()),
                    Value::Integer(25),
                ],
                1,
            ))
            .unwrap();
        db.create_table(table).unwrap();

        // Update Alice's age
        let stmt = Statement::Update {
            table: "users".to_string(),
            assignments: vec![("age".to_string(), Value::Integer(31))],
            filter: Some(crate::parser::Condition::Equals(
                "name".to_string(),
                Value::Text("Alice".to_string()),
            )),
        };

        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));

        // Verify using SELECT (respects MVCC visibility)
        let select_stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("age".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::Equals(
                "name".to_string(),
                Value::Text("Alice".to_string()),
            )),
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let result = QueryExecutor::execute(&mut db, select_stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "31");
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_execute_update_all_rows() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();
        let mut table = create_test_table();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(1),
                    Value::Text("Alice".to_string()),
                    Value::Integer(30),
                ],
                1,
            ))
            .unwrap();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(2),
                    Value::Text("Bob".to_string()),
                    Value::Integer(25),
                ],
                1,
            ))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Update {
            table: "users".to_string(),
            assignments: vec![("age".to_string(), Value::Integer(100))],
            filter: None,
        };

        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));

        // Verify using SELECT (respects MVCC visibility)
        let select_stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("age".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let result = QueryExecutor::execute(&mut db, select_stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0][0], "100");
                assert_eq!(rows[1][0], "100");
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_execute_delete() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();
        let mut table = create_test_table();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(1),
                    Value::Text("Alice".to_string()),
                    Value::Integer(30),
                ],
                1,
            ))
            .unwrap();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(2),
                    Value::Text("Bob".to_string()),
                    Value::Integer(25),
                ],
                1,
            ))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Delete {
            from: "users".to_string(),
            filter: Some(crate::parser::Condition::LessThan(
                "age".to_string(),
                Value::Integer(30),
            )),
        };

        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));

        // Verify using SELECT (respects MVCC visibility)
        let select_stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("name".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let result = QueryExecutor::execute(&mut db, select_stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "Alice");
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_execute_delete_all_rows() {
        let mut db = Database::new("test".to_string());
        let tx_manager = TransactionManager::new();
        let mut table = create_test_table();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(1),
                    Value::Text("Alice".to_string()),
                    Value::Integer(30),
                ],
                1,
            ))
            .unwrap();
        table
            .insert(Row::new_with_xmin(
                vec![
                    Value::Integer(2),
                    Value::Text("Bob".to_string()),
                    Value::Integer(25),
                ],
                1,
            ))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Delete {
            from: "users".to_string(),
            filter: None,
        };

        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        assert!(matches!(result, QueryResult::Success(_)));

        // Verify using SELECT (respects MVCC visibility)
        let select_stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let result = QueryExecutor::execute(&mut db, select_stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 0);
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_condition_equals() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::Equals(
                "name".to_string(),
                Value::Text("Alice".to_string()),
            )),
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_condition_not_equals() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::NotEquals(
                "name".to_string(),
                Value::Text("Alice".to_string()),
            )),
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][1], "Bob");
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_condition_and() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::And(
                Box::new(crate::parser::Condition::GreaterThan(
                    "age".to_string(),
                    Value::Integer(26),
                )),
                Box::new(crate::parser::Condition::LessThan(
                    "age".to_string(),
                    Value::Integer(33),
                )),
            )),
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][1], "Alice"); // age = 30, between 26 and 33
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_condition_or() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::Or(
                Box::new(crate::parser::Condition::Equals(
                    "name".to_string(),
                    Value::Text("Alice".to_string()),
                )),
                Box::new(crate::parser::Condition::Equals(
                    "name".to_string(),
                    Value::Text("Charlie".to_string()),
                )),
            )),
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 2);
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_order_by_asc() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: Some(("age".to_string(), crate::parser::SortOrder::Asc)),
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 3);
                assert_eq!(rows[0][1], "Bob"); // age 25
                assert_eq!(rows[1][1], "Alice"); // age 30
                assert_eq!(rows[2][1], "Charlie"); // age 35
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_order_by_desc() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: Some(("age".to_string(), crate::parser::SortOrder::Desc)),
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 3);
                assert_eq!(rows[0][1], "Charlie"); // age 35
                assert_eq!(rows[1][1], "Alice"); // age 30
                assert_eq!(rows[2][1], "Bob"); // age 25
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_limit() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: Some(2),
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 2); // Only first 2 rows
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_order_by_with_limit() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Regular("*".to_string())],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: Some(("age".to_string(), crate::parser::SortOrder::Desc)),
            limit: Some(2),
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0][1], "Charlie"); // age 35 (highest)
                assert_eq!(rows[1][1], "Alice"); // age 30 (second highest)
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_aggregate_count_all() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Aggregate(
                crate::parser::AggregateFunction::Count(crate::parser::CountTarget::All),
            )],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "3"); // COUNT(*) = 3
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_aggregate_sum() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Aggregate(
                crate::parser::AggregateFunction::Sum("age".to_string()),
            )],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "90"); // 30 + 25 + 35 = 90
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_aggregate_avg() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(20),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(40),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Aggregate(
                crate::parser::AggregateFunction::Avg("age".to_string()),
            )],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "30"); // (30 + 20 + 40) / 3 = 30
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_aggregate_min() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Aggregate(
                crate::parser::AggregateFunction::Min("age".to_string()),
            )],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "25"); // MIN(age) = 25
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_aggregate_max() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Aggregate(
                crate::parser::AggregateFunction::Max("age".to_string()),
            )],
            from: "users".to_string(),
                joins: vec![],
            filter: None,
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "35"); // MAX(age) = 35
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_aggregate_with_where() {
        let mut db = Database::new("test".to_string());
        let mut table = create_test_table();
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Alice".to_string()),
                Value::Integer(30),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Bob".to_string()),
                Value::Integer(25),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Charlie".to_string()),
                Value::Integer(35),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![SelectColumn::Aggregate(
                crate::parser::AggregateFunction::Count(crate::parser::CountTarget::All),
            )],
            from: "users".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::GreaterThan(
                "age".to_string(),
                Value::Integer(26),
            )),
            group_by: None,
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0][0], "2"); // COUNT(*) WHERE age > 26 = 2 (Alice and Charlie)
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_group_by_with_count() {
        let mut db = Database::new("test".to_string());
        let mut table = Table::new(
            "products".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: true,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "category".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "price".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
            ],
        );
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Electronics".to_string()),
                Value::Integer(1000),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Electronics".to_string()),
                Value::Integer(500),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Books".to_string()),
                Value::Integer(20),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![
                SelectColumn::Regular("category".to_string()),
                SelectColumn::Aggregate(crate::parser::AggregateFunction::Count(
                    crate::parser::CountTarget::All,
                )),
            ],
            from: "products".to_string(),
                joins: vec![],
            filter: None,
            group_by: Some(vec!["category".to_string()]),
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, columns) => {
                assert_eq!(columns, vec!["category", "count"]);
                assert_eq!(rows.len(), 2); // 2 categories
                // Results can be in any order, so check both possibilities
                assert!(
                    (rows[0][0] == "Electronics" && rows[0][1] == "2")
                        || (rows[1][0] == "Electronics" && rows[1][1] == "2")
                );
                assert!(
                    (rows[0][0] == "Books" && rows[0][1] == "1")
                        || (rows[1][0] == "Books" && rows[1][1] == "1")
                );
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_group_by_with_sum() {
        let mut db = Database::new("test".to_string());
        let mut table = Table::new(
            "products".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: true,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "category".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "price".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
            ],
        );
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Electronics".to_string()),
                Value::Integer(1000),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Electronics".to_string()),
                Value::Integer(500),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Books".to_string()),
                Value::Integer(20),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![
                SelectColumn::Regular("category".to_string()),
                SelectColumn::Aggregate(crate::parser::AggregateFunction::Sum(
                    "price".to_string(),
                )),
            ],
            from: "products".to_string(),
                joins: vec![],
            filter: None,
            group_by: Some(vec!["category".to_string()]),
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, columns) => {
                assert_eq!(columns, vec!["category", "sum(price)"]);
                assert_eq!(rows.len(), 2);
                // Check sums (order may vary)
                assert!(
                    (rows[0][0] == "Electronics" && rows[0][1] == "1500")
                        || (rows[1][0] == "Electronics" && rows[1][1] == "1500")
                );
                assert!(
                    (rows[0][0] == "Books" && rows[0][1] == "20")
                        || (rows[1][0] == "Books" && rows[1][1] == "20")
                );
            }
            _ => panic!("Expected Rows result"),
        }
    }

    #[test]
    fn test_group_by_without_grouped_column_error() {
        let mut db = Database::new("test".to_string());
        let mut table = Table::new(
            "products".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: true,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "category".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "price".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
            ],
        );
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Electronics".to_string()),
                Value::Integer(1000),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        // Try to select 'price' without including it in GROUP BY
        let stmt = Statement::Select {
                distinct: false,
            columns: vec![
                SelectColumn::Regular("category".to_string()),
                SelectColumn::Regular("price".to_string()), // ERROR: not in GROUP BY
            ],
            from: "products".to_string(),
                joins: vec![],
            filter: None,
            group_by: Some(vec!["category".to_string()]),
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("must appear in GROUP BY clause"));
    }

    #[test]
    fn test_group_by_with_where() {
        let mut db = Database::new("test".to_string());
        let mut table = Table::new(
            "products".to_string(),
            vec![
                Column {
                    name: "id".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: true,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "category".to_string(),
                    data_type: DataType::Text,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
                Column {
                    name: "price".to_string(),
                    data_type: DataType::Integer,
                    nullable: false,
                    primary_key: false,
                unique: false,
                    foreign_key: None,
                },
            ],
        );
        table
            .insert(Row::new(vec![
                Value::Integer(1),
                Value::Text("Electronics".to_string()),
                Value::Integer(1000),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(2),
                Value::Text("Electronics".to_string()),
                Value::Integer(500),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(3),
                Value::Text("Books".to_string()),
                Value::Integer(20),
            ]))
            .unwrap();
        table
            .insert(Row::new(vec![
                Value::Integer(4),
                Value::Text("Books".to_string()),
                Value::Integer(15),
            ]))
            .unwrap();
        db.create_table(table).unwrap();

        let stmt = Statement::Select {
                distinct: false,
            columns: vec![
                SelectColumn::Regular("category".to_string()),
                SelectColumn::Aggregate(crate::parser::AggregateFunction::Count(
                    crate::parser::CountTarget::All,
                )),
            ],
            from: "products".to_string(),
                joins: vec![],
            filter: Some(crate::parser::Condition::GreaterThan(
                "price".to_string(),
                Value::Integer(25),
            )),
            group_by: Some(vec!["category".to_string()]),
            order_by: None,
            limit: None,
                offset: None,
        };

        let tx_manager = TransactionManager::new();
        let result = QueryExecutor::execute(&mut db, stmt, None, &tx_manager).unwrap();
        match result {
            QueryResult::Rows(rows, _) => {
                assert_eq!(rows.len(), 1); // Only Electronics has items > 25
                assert_eq!(rows[0][0], "Electronics");
                assert_eq!(rows[0][1], "2"); // 2 electronics items with price > 25
            }
            _ => panic!("Expected Rows result"),
        }
    }
}
