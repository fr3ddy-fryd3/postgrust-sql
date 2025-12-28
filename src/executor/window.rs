/// Window function execution (v2.6.0)
///
/// ROW_NUMBER(), RANK(), DENSE_RANK(), LAG(), LEAD() with PARTITION BY and ORDER BY
use crate::types::{DatabaseError, Row, Value};
use crate::parser::{WindowFunction, WindowSpec, SortOrder};
use std::collections::HashMap;

pub struct WindowFunctionExecutor;

impl WindowFunctionExecutor {
    /// Execute window function on a set of rows
    /// Returns Vec of result values (one per row)
    pub fn execute(
        function: &WindowFunction,
        spec: &WindowSpec,
        rows: &[&Row],
        table_columns: &[crate::core::Column],
    ) -> Result<Vec<String>, DatabaseError> {
        // Group rows by partition
        let partitions = Self::partition_rows(rows, &spec.partition_by, table_columns)?;

        // Process each partition
        let mut results = vec![String::new(); rows.len()];

        for (_, partition_rows) in partitions.into_iter() {
            // Sort partition by ORDER BY
            let sorted_rows = Self::sort_rows(&partition_rows, &spec.order_by, table_columns)?;

            // Compute window function for this partition
            let partition_results = match function {
                WindowFunction::RowNumber => Self::compute_row_number(&sorted_rows),
                WindowFunction::Rank => Self::compute_rank(&sorted_rows, &spec.order_by, table_columns)?,
                WindowFunction::DenseRank => Self::compute_dense_rank(&sorted_rows, &spec.order_by, table_columns)?,
                WindowFunction::Lag(col, offset) => Self::compute_lag(&sorted_rows, col, *offset, table_columns)?,
                WindowFunction::Lead(col, offset) => Self::compute_lead(&sorted_rows, col, *offset, table_columns)?,
            };

            // Map results back to original row positions
            for (idx, (row_idx, _)) in sorted_rows.iter().enumerate() {
                results[*row_idx] = partition_results[idx].clone();
            }
        }

        Ok(results)
    }

    /// Partition rows by PARTITION BY columns
    fn partition_rows<'a>(
        rows: &'a [&'a Row],
        partition_cols: &[String],
        table_columns: &[crate::core::Column],
    ) -> Result<HashMap<Vec<String>, Vec<(usize, &'a Row)>>, DatabaseError> {
        let mut partitions: HashMap<Vec<String>, Vec<(usize, &Row)>> = HashMap::new();

        for (idx, row) in rows.iter().enumerate() {
            let key = if partition_cols.is_empty() {
                // No PARTITION BY = all rows in one partition
                vec![]
            } else {
                partition_cols.iter().map(|col| {
                    let col_idx = table_columns.iter()
                        .position(|c| &c.name == col)
                        .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col}")))?;
                    Ok(row.values[col_idx].to_string())
                }).collect::<Result<Vec<_>, DatabaseError>>()?
            };

            partitions.entry(key).or_default().push((idx, row));
        }

        Ok(partitions)
    }

    /// Sort rows by ORDER BY columns
    fn sort_rows<'a>(
        rows: &[(usize, &'a Row)],
        order_by: &[(String, SortOrder)],
        table_columns: &[crate::core::Column],
    ) -> Result<Vec<(usize, &'a Row)>, DatabaseError> {
        let mut sorted = rows.to_vec();

        if !order_by.is_empty() {
            // Get column indices for ORDER BY
            let order_indices: Vec<(usize, SortOrder)> = order_by.iter().map(|(col, order)| {
                let idx = table_columns.iter()
                    .position(|c| &c.name == col)
                    .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col}")))?;
                Ok((idx, order.clone()))
            }).collect::<Result<Vec<_>, DatabaseError>>()?;

            sorted.sort_by(|(_, a), (_, b)| {
                for (col_idx, order) in &order_indices {
                    let cmp = a.values[*col_idx].to_string().cmp(&b.values[*col_idx].to_string());
                    if cmp != std::cmp::Ordering::Equal {
                        return if *order == SortOrder::Asc { cmp } else { cmp.reverse() };
                    }
                }
                std::cmp::Ordering::Equal
            });
        }

        Ok(sorted)
    }

    /// ROW_NUMBER() - sequential number within partition
    fn compute_row_number(rows: &[(usize, &Row)]) -> Vec<String> {
        (1..=rows.len()).map(|n| n.to_string()).collect()
    }

    /// RANK() - rank with gaps for ties
    fn compute_rank(
        rows: &[(usize, &Row)],
        order_by: &[(String, SortOrder)],
        table_columns: &[crate::core::Column],
    ) -> Result<Vec<String>, DatabaseError> {
        if rows.is_empty() {
            return Ok(vec![]);
        }

        let mut results = Vec::with_capacity(rows.len());
        let mut current_rank = 1;
        let mut same_rank_count = 1;

        results.push("1".to_string());

        for i in 1..rows.len() {
            // Check if current row equals previous row on ORDER BY columns
            let is_same = Self::rows_equal_on_columns(&rows[i-1].1, &rows[i].1, order_by, table_columns)?;

            if is_same {
                results.push(current_rank.to_string());
                same_rank_count += 1;
            } else {
                current_rank += same_rank_count;
                results.push(current_rank.to_string());
                same_rank_count = 1;
            }
        }

        Ok(results)
    }

    /// DENSE_RANK() - rank without gaps for ties
    fn compute_dense_rank(
        rows: &[(usize, &Row)],
        order_by: &[(String, SortOrder)],
        table_columns: &[crate::core::Column],
    ) -> Result<Vec<String>, DatabaseError> {
        if rows.is_empty() {
            return Ok(vec![]);
        }

        let mut results = Vec::with_capacity(rows.len());
        let mut current_rank = 1;

        results.push("1".to_string());

        for i in 1..rows.len() {
            let is_same = Self::rows_equal_on_columns(&rows[i-1].1, &rows[i].1, order_by, table_columns)?;

            if !is_same {
                current_rank += 1;
            }
            results.push(current_rank.to_string());
        }

        Ok(results)
    }

    /// LAG(col, offset) - value from previous row
    fn compute_lag(
        rows: &[(usize, &Row)],
        col_name: &str,
        offset: Option<i64>,
        table_columns: &[crate::core::Column],
    ) -> Result<Vec<String>, DatabaseError> {
        let col_idx = table_columns.iter()
            .position(|c| &c.name == col_name)
            .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col_name}")))?;

        let offset = offset.unwrap_or(1).max(0) as usize;

        let mut results = Vec::with_capacity(rows.len());
        for i in 0..rows.len() {
            if i < offset {
                results.push("NULL".to_string());
            } else {
                results.push(rows[i - offset].1.values[col_idx].to_string());
            }
        }

        Ok(results)
    }

    /// LEAD(col, offset) - value from next row
    fn compute_lead(
        rows: &[(usize, &Row)],
        col_name: &str,
        offset: Option<i64>,
        table_columns: &[crate::core::Column],
    ) -> Result<Vec<String>, DatabaseError> {
        let col_idx = table_columns.iter()
            .position(|c| &c.name == col_name)
            .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col_name}")))?;

        let offset = offset.unwrap_or(1).max(0) as usize;

        let mut results = Vec::with_capacity(rows.len());
        for i in 0..rows.len() {
            if i + offset >= rows.len() {
                results.push("NULL".to_string());
            } else {
                results.push(rows[i + offset].1.values[col_idx].to_string());
            }
        }

        Ok(results)
    }

    /// Check if two rows are equal on specified columns
    fn rows_equal_on_columns(
        row1: &Row,
        row2: &Row,
        order_by: &[(String, SortOrder)],
        table_columns: &[crate::core::Column],
    ) -> Result<bool, DatabaseError> {
        for (col, _) in order_by {
            let col_idx = table_columns.iter()
                .position(|c| &c.name == col)
                .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {col}")))?;

            if row1.values[col_idx].to_string() != row2.values[col_idx].to_string() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
