/// Query (SELECT) operations
///
/// SELECT, JOIN, aggregate functions, GROUP BY

use crate::types::{Database, DatabaseError, Row, Table, Value};
use crate::parser::{SelectColumn, Condition, AggregateFunction, CountTarget, SortOrder};
use crate::transaction::TransactionManager;
use super::legacy_executor::QueryResult;
use super::conditions::ConditionEvaluator;
use crate::index::BTreeIndex;

pub struct QueryExecutor;

impl QueryExecutor {
    /// Find usable index for WHERE condition
    ///
    /// Returns (index_name, column_name, value) if:
    /// - Filter is Equals(col, val) or GreaterThan/LessThan
    /// - Index exists on that column
    fn find_usable_index<'a>(
        db: &'a Database,
        table_name: &str,
        filter: &'a Option<Condition>,
    ) -> Option<(&'a str, &'a BTreeIndex, &'a str, &'a Value)> {
        // Only optimize simple equality/range conditions (not AND/OR yet)
        let (column, value) = match filter {
            Some(Condition::Equals(col, val)) => (col, val),
            Some(Condition::GreaterThan(col, val)) => (col, val),
            Some(Condition::LessThan(col, val)) => (col, val),
            _ => return None, // AND/OR/NotEquals require full scan
        };

        // Find index on this column
        for (idx_name, index) in &db.indexes {
            if index.table_name == table_name && index.column_name == *column {
                return Some((idx_name, index, column, value));
            }
        }

        None
    }

    /// Main SELECT dispatcher
    ///
    /// Routes to appropriate handler based on:
    /// - JOIN presence → select_with_join()
    /// - GROUP BY → select_with_group_by()
    /// - Aggregates without GROUP BY → select_aggregate()
    /// - Regular → select_regular()
    pub fn select(
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
        database_storage: Option<&crate::storage::DatabaseStorage>,
    ) -> Result<QueryResult, DatabaseError> {
        // Check if this is a JOIN query
        if !joins.is_empty() {
            return Self::select_with_join(db, distinct, columns, from, joins, filter, order_by, limit, offset, tx_manager, database_storage);
        }

        // Check if this is an aggregate query
        let has_aggregates = columns
            .iter()
            .any(|col| matches!(col, SelectColumn::Aggregate(_)));

        if group_by.is_some() {
            Self::select_with_group_by(db, distinct, columns, from, filter, group_by.unwrap(), order_by, limit, offset, tx_manager, database_storage)
        } else if has_aggregates {
            Self::select_aggregate(db, distinct, columns, from, filter, tx_manager, database_storage)
        } else {
            Self::select_regular(db, distinct, columns, from, filter, order_by, limit, offset, tx_manager, database_storage)
        }
    }

    /// Regular SELECT (no aggregates, no GROUP BY, no JOIN)
    ///
    /// Execution order:
    /// 1. MVCC visibility check
    /// 2. WHERE filter
    /// 3. ORDER BY
    /// 4. DISTINCT
    /// 5. OFFSET
    /// 6. LIMIT
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
        database_storage: Option<&crate::storage::DatabaseStorage>,
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

        // Try to use index if available
        let use_index = Self::find_usable_index(db, &from, &filter);

        // Get rows from appropriate storage backend
        let rows_vec: Vec<Row>;
        let rows_iter: Box<dyn Iterator<Item = &Row>>;

        if let Some(db_storage) = database_storage {
            // Page-based storage: read from PagedTable
            if let Some(paged_table) = db_storage.get_paged_table(&from) {
                rows_vec = paged_table.get_all_rows()?;
                rows_iter = Box::new(rows_vec.iter());
            } else {
                return Err(DatabaseError::TableNotFound(from.clone()));
            }
        } else {
            // Legacy storage: read from table.rows
            rows_iter = Box::new(table.rows.iter());
        }

        // Collect rows with their original indices (for sorting)
        let mut rows_with_data: Vec<(Row, Vec<String>)> = Vec::new();

        // Index scan vs sequential scan
        if let Some((_idx_name, index, _col_name, search_value)) = use_index {
            // INDEX SCAN: Use B-tree index for fast lookup
            let row_indices = index.search(search_value);

            // Get all rows first (needed to access by index)
            let all_rows: Vec<Row> = if let Some(db_storage) = database_storage {
                if let Some(paged_table) = db_storage.get_paged_table(&from) {
                    paged_table.get_all_rows()?
                } else {
                    return Err(DatabaseError::TableNotFound(from.clone()));
                }
            } else {
                table.rows.clone()
            };

            for &row_idx in &row_indices {
                if row_idx >= all_rows.len() {
                    continue; // Skip invalid indices
                }

                let row = &all_rows[row_idx];

                // MVCC: Check row visibility
                if !row.is_visible(current_tx_id) {
                    continue;
                }

                // Index already filtered by equality, but double-check condition
                if let Some(ref cond) = filter {
                    if !ConditionEvaluator::evaluate_with_columns(&table.columns, row, cond)? {
                        continue;
                    }
                }

                let result_row: Vec<String> = column_indices
                    .iter()
                    .map(|&idx| row.values[idx].to_string())
                    .collect();
                rows_with_data.push((row.clone(), result_row));
            }
        } else {
            // SEQUENTIAL SCAN: Full table scan
            for row in rows_iter {
                // MVCC: Check row visibility
                if !row.is_visible(current_tx_id) {
                    continue;
                }

                if let Some(ref cond) = filter {
                    if !ConditionEvaluator::evaluate_with_columns(&table.columns, row, cond)? {
                        continue;
                    }
                }

                let result_row: Vec<String> = column_indices
                    .iter()
                    .map(|&idx| row.values[idx].to_string())
                    .collect();
                rows_with_data.push((row.clone(), result_row));
            }
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

    /// Aggregate SELECT (COUNT, SUM, AVG, MIN, MAX)
    ///
    /// No GROUP BY - returns single row with aggregate results
    fn select_aggregate(
        db: &Database,
        _distinct: bool,
        columns: Vec<SelectColumn>,
        from: String,
        filter: Option<Condition>,
        tx_manager: &TransactionManager,
        _database_storage: Option<&crate::storage::DatabaseStorage>,
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
                    ConditionEvaluator::evaluate_with_columns(&table.columns, row, cond).unwrap_or(false)
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

    /// Compute aggregate function (COUNT, SUM, AVG, MIN, MAX)
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

    /// SELECT with GROUP BY
    ///
    /// Groups rows by specified columns and computes aggregates per group
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
        _database_storage: Option<&crate::storage::DatabaseStorage>,
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
                    ConditionEvaluator::evaluate_with_columns(&table.columns, row, f).unwrap_or(false)
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

    /// SELECT with JOIN (INNER, LEFT, RIGHT)
    ///
    /// Limitations:
    /// - Only one JOIN per query
    /// - WHERE with JOIN not fully supported
    /// - Returns all columns (column selection TODO)
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
        _database_storage: Option<&crate::storage::DatabaseStorage>,
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
