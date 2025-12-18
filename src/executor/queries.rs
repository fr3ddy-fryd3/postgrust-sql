/// Query (SELECT) operations
///
/// SELECT, JOIN, aggregate functions, GROUP BY
use crate::types::{Database, DatabaseError, Row, Table, Value};
use crate::parser::{SelectColumn, Condition, AggregateFunction, CountTarget, SortOrder, CaseExpression};
use crate::transaction::GlobalTransactionManager;
use super::dispatcher_executor::QueryResult;
use super::conditions::ConditionEvaluator;
use crate::index::Index;

pub struct QueryExecutor;

impl QueryExecutor {
    /// Evaluate CASE expression for a given row (v1.10.0)
    fn evaluate_case(
        case_expr: &CaseExpression,
        columns: &[crate::types::Column],
        row: &Row,
    ) -> Result<Value, DatabaseError> {
        // Evaluate each WHEN clause
        for when in &case_expr.when_clauses {
            if ConditionEvaluator::evaluate_with_columns(columns, row, &when.condition)? {
                return Ok(when.result.clone());
            }
        }

        // If no WHEN matched, return ELSE or NULL
        Ok(case_expr.else_value.clone().unwrap_or(Value::Null))
    }
}

impl QueryExecutor {
    /// Find usable index for WHERE condition (v1.9.0: supports composite indexes)
    ///
    /// Returns Some for:
    /// - Single column: Equals(col, val) or GreaterThan/LessThan
    /// - Composite: AND of multiple Equals conditions matching index columns
    fn find_usable_index<'a>(
        db: &'a Database,
        table_name: &str,
        filter: &'a Option<Condition>,
    ) -> Option<(&'a str, &'a Index, Vec<(&'a str, &'a Value)>)> {
        let filter = match filter {
            Some(f) => f,
            None => return None,
        };

        // First, try to find composite index usage (v1.9.0)
        // Check if filter is AND chain of Equals conditions
        let mut equals_conditions: Vec<(&str, &Value)> = Vec::new();
        Self::extract_equals_from_and(filter, &mut equals_conditions);

        if equals_conditions.len() >= 2 {
            // Multiple equality conditions - look for matching composite index
            for (idx_name, index) in &db.indexes {
                if index.table_name() != table_name || !index.is_composite() {
                    continue;
                }

                let index_cols = index.column_names();
                if index_cols.len() != equals_conditions.len() {
                    continue; // Composite index must match all columns
                }

                // Check if all index columns are present in equals_conditions
                let mut matched_values: Vec<(&str, &Value)> = Vec::new();
                let mut all_matched = true;

                for col_name in index_cols {
                    if let Some((_col, val)) = equals_conditions.iter().find(|(c, _)| *c == col_name) {
                        matched_values.push((col_name, val));
                    } else {
                        all_matched = false;
                        break;
                    }
                }

                if all_matched {
                    return Some((idx_name, index, matched_values));
                }
            }
        }

        // Fall back to single-column index (existing logic)
        let (column, value) = match filter {
            Condition::Equals(col, val) => (col.as_str(), val),
            Condition::GreaterThan(col, val) => (col.as_str(), val),
            Condition::LessThan(col, val) => (col.as_str(), val),
            _ => return None, // Other conditions require full scan
        };

        // Find single-column index
        for (idx_name, index) in &db.indexes {
            if index.table_name() == table_name && !index.is_composite() && index.column_name() == column {
                return Some((idx_name, index, vec![(column, value)]));
            }
        }

        None
    }

    /// Extract Equals conditions from AND chain (v1.9.0)
    fn extract_equals_from_and<'a>(cond: &'a Condition, result: &mut Vec<(&'a str, &'a Value)>) {
        match cond {
            Condition::Equals(col, val) => {
                result.push((col.as_str(), val));
            }
            Condition::And(left, right) => {
                Self::extract_equals_from_and(left, result);
                Self::extract_equals_from_and(right, result);
            }
            _ => {
                // Non-equals conditions stop composite index usage
            }
        }
    }

    /// Main SELECT dispatcher
    ///
    /// Routes to appropriate handler based on:
    /// - JOIN presence → `select_with_join()`
    /// - GROUP BY → `select_with_group_by()`
    /// - Aggregates without GROUP BY → `select_aggregate()`
    /// - Regular → `select_regular()`
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
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        // v2.0.0: Check if 'from' is a system catalog
        if super::system_catalogs::SystemCatalog::is_system_catalog(&from) {
            return super::system_catalogs::SystemCatalog::query(&from, db);
        }

        // Check if 'from' is a view (v1.10.0)
        if let Some(view_query) = db.views.get(&from) {
            // Parse the view's SQL
            let view_stmt = crate::parser::parse_statement(view_query)
                .map_err(DatabaseError::ParseError)?;

            // Execute the view query (only SELECT is supported in views)
            match view_stmt {
                crate::parser::Statement::Select {
                    distinct: view_distinct,
                    columns: view_columns,
                    from: view_from,
                    joins: view_joins,
                    filter: view_filter,
                    group_by: view_group_by,
                    order_by: view_order_by,
                    limit: view_limit,
                    offset: view_offset,
                } => {
                    // Recursively call select (handles nested views)
                    return Self::select(
                        db,
                        view_distinct,
                        view_columns,
                        view_from,
                        view_joins,
                        view_filter,
                        view_group_by,
                        view_order_by,
                        view_limit,
                        view_offset,
                        tx_manager,
                        database_storage,
                    );
                }
                _ => {
                    return Err(DatabaseError::ParseError(
                        format!("View '{from}' contains non-SELECT statement")
                    ));
                }
            }
        }

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
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        // Separate regular columns from CASE expressions (v1.10.0)
        let mut regular_col_names: Vec<String> = Vec::new();
        let mut case_expressions: Vec<(usize, &CaseExpression)> = Vec::new();

        for (idx, col) in columns.iter().enumerate() {
            match col {
                SelectColumn::Regular(name) => regular_col_names.push(name.clone()),
                SelectColumn::Case(case_expr) => case_expressions.push((idx, case_expr)),
                SelectColumn::Aggregate(_) => {
                    panic!("Aggregate in regular select should not happen")
                }
            }
        }

        let is_select_all = regular_col_names.len() == 1 && !regular_col_names.is_empty() && regular_col_names[0] == "*";

        // Only process regular columns for indices
        let column_indices: Vec<usize> = if is_select_all && case_expressions.is_empty() {
            (0..table.columns.len()).collect()
        } else {
            regular_col_names
                .iter()
                .map(|col| {
                    table
                        .get_column_index(col)
                        .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col}")))
                })
                .collect::<Result<Vec<_>, _>>()?
        };

        // Build column names for result (include CASE aliases)
        let mut column_names: Vec<String> = column_indices
            .iter()
            .map(|&idx| table.columns[idx].name.clone())
            .collect();

        // Add CASE expression column names (use alias or "case")
        for (_, case_expr) in &case_expressions {
            column_names.push(case_expr.alias.clone().unwrap_or_else(|| "case".to_string()));
        }

        // Get snapshot for READ COMMITTED isolation (v2.1.0)
        // Creates new snapshot before each statement
        let snapshot = tx_manager.get_snapshot();

        // Try to use index if available
        let use_index = Self::find_usable_index(db, &from, &filter);

        // Get rows from PagedTable (v2.0.0)
        let paged_table = database_storage.get_paged_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;
        let rows_vec = paged_table.get_all_rows()?;
        let rows_iter: Box<dyn Iterator<Item = &Row>> = Box::new(rows_vec.iter());

        // Collect rows with their original indices (for sorting)
        let mut rows_with_data: Vec<(Row, Vec<String>)> = Vec::new();

        // Index scan vs sequential scan (v1.9.0: supports composite indexes)
        if let Some((_idx_name, index, col_values)) = use_index {
            // INDEX SCAN: Use index for fast lookup (single or composite)
            let row_indices = if index.is_composite() && col_values.len() > 1 {
                // Composite index: extract values in column order
                let values: Vec<Value> = col_values.iter().map(|(_, v)| (*v).clone()).collect();
                index.search_composite(&values)
            } else {
                // Single column index
                index.search(col_values[0].1)
            };

            // Get all rows first (needed to access by index)
            let paged_table = database_storage.get_paged_table(&from)
                .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;
            let all_rows = paged_table.get_all_rows()?;

            for &row_idx in &row_indices {
                if row_idx >= all_rows.len() {
                    continue; // Skip invalid indices
                }

                let row = &all_rows[row_idx];

                // MVCC: Check row visibility
                if !row.is_visible_to_snapshot(&snapshot) {
                    continue;
                }

                // Index already filtered by equality, but double-check condition
                if let Some(ref cond) = filter
                    && !ConditionEvaluator::evaluate_with_columns(&table.columns, row, cond)? {
                        continue;
                    }

                // Build result row: regular columns + CASE expressions
                let mut result_row: Vec<String> = column_indices
                    .iter()
                    .map(|&idx| row.values[idx].to_string())
                    .collect();

                // Evaluate CASE expressions (v1.10.0)
                for (_, case_expr) in &case_expressions {
                    let case_value = Self::evaluate_case(case_expr, &table.columns, row)?;
                    result_row.push(case_value.to_string());
                }

                rows_with_data.push((row.clone(), result_row));
            }
        } else {
            // SEQUENTIAL SCAN: Full table scan
            for row in rows_iter {
                // MVCC: Check row visibility
                if !row.is_visible_to_snapshot(&snapshot) {
                    continue;
                }

                if let Some(ref cond) = filter
                    && !ConditionEvaluator::evaluate_with_columns(&table.columns, row, cond)? {
                        continue;
                    }

                // Build result row: regular columns + CASE expressions
                let mut result_row: Vec<String> = column_indices
                    .iter()
                    .map(|&idx| row.values[idx].to_string())
                    .collect();

                // Evaluate CASE expressions (v1.10.0)
                for (_, case_expr) in &case_expressions {
                    let case_value = Self::evaluate_case(case_expr, &table.columns, row)?;
                    result_row.push(case_value.to_string());
                }

                rows_with_data.push((row.clone(), result_row));
            }
        }

        // Apply ORDER BY if specified
        if let Some((sort_column, sort_order)) = order_by {
            let sort_col_idx = table
                .get_column_index(&sort_column)
                .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {sort_column}")))?;

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
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        // Get snapshot for READ COMMITTED isolation (v2.1.0)
        // Creates new snapshot before each statement
        let snapshot = tx_manager.get_snapshot();

        // Get rows from PagedTable
        let paged_table = database_storage.get_paged_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;
        let rows_vec = paged_table.get_all_rows()?;

        // Collect visible rows that match the filter
        let visible_rows: Vec<&Row> = rows_vec
            .iter()
            .filter(|row| {
                // MVCC: Check row visibility
                if !row.is_visible_to_snapshot(&snapshot) {
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
                SelectColumn::Case(_) => {
                    return Err(DatabaseError::ParseError(
                        "Cannot use CASE expressions with aggregates without GROUP BY".to_string(),
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
                                DatabaseError::ParseError(format!("Unknown column: {col_name}"))
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
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col_name}")))?;

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

                Ok((value, format!("sum({col_name})")))
            }
            AggregateFunction::Avg(col_name) => {
                let col_idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col_name}")))?;

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

                let avg = if count > 0 { sum / f64::from(count) } else { 0.0 };
                Ok((avg.to_string(), format!("avg({col_name})")))
            }
            AggregateFunction::Min(col_name) => {
                let col_idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col_name}")))?;

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

                let value = min_val.map_or_else(|| "NULL".to_string(), std::string::ToString::to_string);
                Ok((value, format!("min({col_name})")))
            }
            AggregateFunction::Max(col_name) => {
                let col_idx = table
                    .get_column_index(col_name)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col_name}")))?;

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

                let value = max_val.map_or_else(|| "NULL".to_string(), std::string::ToString::to_string);
                Ok((value, format!("max({col_name})")))
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
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        use std::collections::HashMap;

        let table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        // Get snapshot for READ COMMITTED isolation (v2.1.0)
        let snapshot = tx_manager.get_snapshot();

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

        // Get rows from PagedTable
        let paged_table = database_storage.get_paged_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;
        let rows_vec = paged_table.get_all_rows()?;

        // Filter visible rows
        let visible_rows: Vec<&Row> = rows_vec
            .iter()
            .filter(|row| {
                if !row.is_visible_to_snapshot(&snapshot) {
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
            groups.entry(key).or_default().push(row);
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
                            "Column '{name}' must appear in GROUP BY clause or be used in an aggregate function"
                        )));
                    }
                    column_names.push(name.clone());
                }
                SelectColumn::Aggregate(agg_func) => {
                    let (_, name) = Self::compute_aggregate(agg_func, table, &[])?;
                    column_names.push(name);
                }
                SelectColumn::Case(case_expr) => {
                    // CASE expressions are allowed in GROUP BY context (v1.10.0)
                    column_names.push(case_expr.alias.clone().unwrap_or_else(|| "case".to_string()));
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
                    SelectColumn::Case(case_expr) => {
                        // Evaluate CASE expression on first row of group (v1.10.0)
                        // In GROUP BY context, CASE should be deterministic per group
                        if let Some(first_row) = group_rows.first() {
                            let case_value = Self::evaluate_case(case_expr, &table.columns, first_row)?;
                            row_values.push(case_value.to_string());
                        } else {
                            row_values.push("NULL".to_string());
                        }
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
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        use crate::parser::JoinType;

        // Get the main table
        let main_table = db
            .get_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;

        let snapshot = tx_manager.get_snapshot();

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

        // Get rows from PagedTable (v2.0.0)
        let main_paged_table = database_storage.get_paged_table(&from)
            .ok_or_else(|| DatabaseError::TableNotFound(from.clone()))?;
        let main_rows = main_paged_table.get_all_rows()?;

        let join_paged_table = database_storage.get_paged_table(&join.table)
            .ok_or_else(|| DatabaseError::TableNotFound(join.table.clone()))?;
        let join_rows = join_paged_table.get_all_rows()?;

        // Parse column references (table.column)
        let parse_col_ref = |ref_str: &str| -> Result<(String, String), DatabaseError> {
            let parts: Vec<&str> = ref_str.split('.').collect();
            if parts.len() != 2 {
                return Err(DatabaseError::ParseError(format!(
                    "Invalid column reference: {ref_str}"
                )));
            }
            Ok((parts[0].to_string(), parts[1].to_string()))
        };

        let (left_table_name, left_col_name) = parse_col_ref(&join.on_left)?;
        let (_right_table_name, right_col_name) = parse_col_ref(&join.on_right)?;

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

        for main_row in &main_rows {
            if !main_row.is_visible_to_snapshot(&snapshot) {
                continue;
            }

            let main_join_value = &main_row.values[main_join_idx];
            let mut matched = false;

            for join_row in &join_rows {
                if !join_row.is_visible_to_snapshot(&snapshot) {
                    continue;
                }

                let join_join_value = &join_row.values[join_join_idx];

                if main_join_value == join_join_value {
                    matched = true;
                    // Combine rows
                    let mut combined_row: Vec<String> = main_row
                        .values
                        .iter()
                        .map(std::string::ToString::to_string)
                        .collect();
                    combined_row.extend(join_row.values.iter().map(std::string::ToString::to_string));
                    result_rows.push(combined_row);
                }
            }

            // For LEFT JOIN, include non-matching rows with NULLs
            if !matched && matches!(join.join_type, JoinType::Left) {
                let mut combined_row: Vec<String> = main_row
                    .values
                    .iter()
                    .map(std::string::ToString::to_string)
                    .collect();
                combined_row.extend(vec!["NULL".to_string(); join_table.columns.len()]);
                result_rows.push(combined_row);
            }
        }

        // For RIGHT JOIN, include non-matching rows from join table
        if matches!(join.join_type, JoinType::Right) {
            for join_row in &join_rows {
                if !join_row.is_visible_to_snapshot(&snapshot) {
                    continue;
                }

                let join_join_value = &join_row.values[join_join_idx];
                let matched = main_rows.iter().any(|main_row| {
                    main_row.is_visible_to_snapshot(&snapshot)
                        && &main_row.values[main_join_idx] == join_join_value
                });

                if !matched {
                    let mut combined_row = vec!["NULL".to_string(); main_table.columns.len()];
                    combined_row.extend(join_row.values.iter().map(std::string::ToString::to_string));
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

    /// UNION: Combine results from two queries (v1.10.0)
    ///
    /// UNION removes duplicates, UNION ALL keeps duplicates
    pub fn union(
        db: &Database,
        left: &crate::parser::Statement,
        right: &crate::parser::Statement,
        all: bool,
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        // Execute both queries
        let left_result = Self::execute_query_stmt(db, left, tx_manager, database_storage)?;
        let right_result = Self::execute_query_stmt(db, right, tx_manager, database_storage)?;

        let (mut left_rows, left_cols) = match left_result {
            QueryResult::Rows(rows, cols) => (rows, cols),
            _ => return Err(DatabaseError::ParseError("UNION requires SELECT queries".to_string())),
        };

        let (right_rows, right_cols) = match right_result {
            QueryResult::Rows(rows, cols) => (rows, cols),
            _ => return Err(DatabaseError::ParseError("UNION requires SELECT queries".to_string())),
        };

        // Check column compatibility
        if left_cols.len() != right_cols.len() {
            return Err(DatabaseError::ParseError(
                format!("UNION queries must have same number of columns: {} vs {}", left_cols.len(), right_cols.len())
            ));
        }

        // Combine results
        left_rows.extend(right_rows);

        // Remove duplicates if not UNION ALL
        if !all {
            use std::collections::HashSet;
            let mut seen: HashSet<Vec<String>> = HashSet::new();
            left_rows.retain(|row| seen.insert(row.clone()));
        }

        Ok(QueryResult::Rows(left_rows, left_cols))
    }

    /// INTERSECT: Return rows that appear in both queries (v1.10.0)
    pub fn intersect(
        db: &Database,
        left: &crate::parser::Statement,
        right: &crate::parser::Statement,
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        // Execute both queries
        let left_result = Self::execute_query_stmt(db, left, tx_manager, database_storage)?;
        let right_result = Self::execute_query_stmt(db, right, tx_manager, database_storage)?;

        let (left_rows, left_cols) = match left_result {
            QueryResult::Rows(rows, cols) => (rows, cols),
            _ => return Err(DatabaseError::ParseError("INTERSECT requires SELECT queries".to_string())),
        };

        let (right_rows, right_cols) = match right_result {
            QueryResult::Rows(rows, cols) => (rows, cols),
            _ => return Err(DatabaseError::ParseError("INTERSECT requires SELECT queries".to_string())),
        };

        // Check column compatibility
        if left_cols.len() != right_cols.len() {
            return Err(DatabaseError::ParseError(
                format!("INTERSECT queries must have same number of columns: {} vs {}", left_cols.len(), right_cols.len())
            ));
        }

        // Find intersection using HashSet
        use std::collections::HashSet;
        let right_set: HashSet<Vec<String>> = right_rows.into_iter().collect();
        let result_rows: Vec<Vec<String>> = left_rows
            .into_iter()
            .filter(|row| right_set.contains(row))
            .collect::<HashSet<_>>()  // Remove duplicates
            .into_iter()
            .collect();

        Ok(QueryResult::Rows(result_rows, left_cols))
    }

    /// EXCEPT: Return rows from left query that don't appear in right query (v1.10.0)
    pub fn except(
        db: &Database,
        left: &crate::parser::Statement,
        right: &crate::parser::Statement,
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        // Execute both queries
        let left_result = Self::execute_query_stmt(db, left, tx_manager, database_storage)?;
        let right_result = Self::execute_query_stmt(db, right, tx_manager, database_storage)?;

        let (left_rows, left_cols) = match left_result {
            QueryResult::Rows(rows, cols) => (rows, cols),
            _ => return Err(DatabaseError::ParseError("EXCEPT requires SELECT queries".to_string())),
        };

        let (right_rows, right_cols) = match right_result {
            QueryResult::Rows(rows, cols) => (rows, cols),
            _ => return Err(DatabaseError::ParseError("EXCEPT requires SELECT queries".to_string())),
        };

        // Check column compatibility
        if left_cols.len() != right_cols.len() {
            return Err(DatabaseError::ParseError(
                format!("EXCEPT queries must have same number of columns: {} vs {}", left_cols.len(), right_cols.len())
            ));
        }

        // Find difference using HashSet
        use std::collections::HashSet;
        let right_set: HashSet<Vec<String>> = right_rows.into_iter().collect();
        let result_rows: Vec<Vec<String>> = left_rows
            .into_iter()
            .filter(|row| !right_set.contains(row))
            .collect::<HashSet<_>>()  // Remove duplicates
            .into_iter()
            .collect();

        Ok(QueryResult::Rows(result_rows, left_cols))
    }

    /// Helper: Execute a Statement that should be a query
    fn execute_query_stmt(
        db: &Database,
        stmt: &crate::parser::Statement,
        tx_manager: &GlobalTransactionManager,
        database_storage: &crate::storage::DatabaseStorage,
    ) -> Result<QueryResult, DatabaseError> {
        match stmt {
            crate::parser::Statement::Select { distinct, columns, from, joins, filter, group_by, order_by, limit, offset } => {
                Self::select(db, *distinct, columns.clone(), from.clone(), joins.clone(), filter.clone(), group_by.clone(), order_by.clone(), *limit, *offset, tx_manager, database_storage)
            }
            crate::parser::Statement::Union { left, right, all } => {
                Self::union(db, left, right, *all, tx_manager, database_storage)
            }
            crate::parser::Statement::Intersect { left, right } => {
                Self::intersect(db, left, right, tx_manager, database_storage)
            }
            crate::parser::Statement::Except { left, right } => {
                Self::except(db, left, right, tx_manager, database_storage)
            }
            _ => Err(DatabaseError::ParseError("Not a query statement".to_string())),
        }
    }
}
