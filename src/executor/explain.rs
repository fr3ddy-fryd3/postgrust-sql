/// EXPLAIN query analyzer (v1.8.0)
///
/// Analyzes SELECT queries and returns execution plan details:
/// - Index usage (index scan vs sequential scan)
/// - Estimated cost (O(1), O(log n), O(n))
/// - Estimated row count
/// - Join strategy
/// - Filter conditions
use crate::parser::{Statement, Condition};
use crate::types::{Database, DatabaseError};

// Define QueryResult locally to avoid circular dependency
#[derive(Debug)]
pub enum QueryResult {
    Success(String),
    Rows(Vec<Vec<String>>, Vec<String>), // (rows, column_names)
}

pub struct ExplainExecutor;

#[derive(Debug)]
pub struct QueryPlan {
    pub scan_type: ScanType,
    pub table_name: String,
    pub index_name: Option<String>,
    pub index_type: Option<String>,  // "hash" or "btree"
    pub filter: Option<String>,
    pub estimated_rows: usize,
    pub cost: String,  // "O(1)", "O(log n)", "O(n)"
}

#[derive(Debug)]
pub enum ScanType {
    SequentialScan,
    IndexScan,
    UniqueIndexScan,
}

impl ExplainExecutor {
    pub fn explain(
        db: &Database,
        statement: &Statement,
    ) -> Result<QueryResult, DatabaseError> {
        match statement {
            Statement::Select {
                from,
                filter,
                joins,
                ..
            } => {
                let plan = Self::analyze_select(db, from, filter)?;
                let output = Self::format_plan(&plan, joins.is_empty());
                Ok(QueryResult::Success(output))
            }
            _ => Err(DatabaseError::ParseError(
                "EXPLAIN only supports SELECT statements".to_string(),
            )),
        }
    }

    fn analyze_select(
        db: &Database,
        table_name: &str,
        filter: &Option<Condition>,
    ) -> Result<QueryPlan, DatabaseError> {
        // Get table
        let table = db
            .get_table(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        let total_rows = table.rows.len();

        // Check if an index can be used
        let (scan_type, index_info, cost, estimated_rows) = if let Some(cond) = filter {
            Self::find_index_for_condition(db, table_name, cond, total_rows)
        } else {
            // No filter = full table scan
            (ScanType::SequentialScan, None, "O(n)".to_string(), total_rows)
        };

        let (index_name, index_type) = match index_info {
            Some((name, itype)) => (Some(name), Some(itype)),
            None => (None, None),
        };

        Ok(QueryPlan {
            scan_type,
            table_name: table_name.to_string(),
            index_name,
            index_type,
            filter: filter.as_ref().map(Self::format_condition),
            estimated_rows,
            cost,
        })
    }

    fn find_index_for_condition(
        db: &Database,
        table_name: &str,
        condition: &Condition,
        total_rows: usize,
    ) -> (ScanType, Option<(String, String)>, String, usize) {
        // v1.9.0: Try composite index first (AND chain of Equals)
        let mut equals_cols: Vec<&str> = Vec::new();
        Self::extract_equals_columns(condition, &mut equals_cols);

        if equals_cols.len() >= 2 {
            // Multiple equality conditions - look for matching composite index
            for (idx_name, index) in &db.indexes {
                if index.table_name() != table_name || !index.is_composite() {
                    continue;
                }

                let index_cols = index.column_names();
                if index_cols.len() == equals_cols.len() {
                    // Check if all index columns match
                    let all_match = index_cols.iter().all(|ic| equals_cols.contains(&ic.as_str()));
                    if all_match {
                        let index_type_str = match index.index_type() {
                            crate::index::IndexType::Hash => "hash",
                            crate::index::IndexType::BTree => "btree",
                        };

                        let scan_type = if index.is_unique() {
                            ScanType::UniqueIndexScan
                        } else {
                            ScanType::IndexScan
                        };

                        let cost = if index_type_str == "hash" {
                            "O(1)"
                        } else {
                            "O(log n)"
                        };

                        let estimated = if index.is_unique() { 1 } else { total_rows / 10 };

                        return (
                            scan_type,
                            Some((idx_name.clone(), index_type_str.to_string())),
                            cost.to_string(),
                            estimated,
                        );
                    }
                }
            }
        }

        // Fall back to single-column index
        let (column, op) = match condition {
            Condition::Equals(col, _) => (col, "="),
            Condition::GreaterThan(col, _) => (col, ">"),
            Condition::LessThan(col, _) => (col, "<"),
            Condition::GreaterThanOrEqual(col, _) => (col, ">="),
            Condition::LessThanOrEqual(col, _) => (col, "<="),
            Condition::Like(col, _) => (col, "LIKE"),
            Condition::In(col, _) => (col, "IN"),
            _ => return (ScanType::SequentialScan, None, "O(n)".to_string(), total_rows),
        };

        // Find single-column index on this column
        for (idx_name, index) in &db.indexes {
            if index.table_name() == table_name && !index.is_composite() && index.column_name() == column {
                let index_type_str = match index.index_type() {
                    crate::index::IndexType::Hash => "hash",
                    crate::index::IndexType::BTree => "btree",
                };

                // Hash index only supports equality
                if index_type_str == "hash" {
                    if op == "=" || op == "IN" {
                        let scan_type = if index.is_unique() {
                            ScanType::UniqueIndexScan
                        } else {
                            ScanType::IndexScan
                        };
                        // O(1) for hash index
                        return (
                            scan_type,
                            Some((idx_name.clone(), index_type_str.to_string())),
                            "O(1)".to_string(),
                            if index.is_unique() { 1 } else { total_rows / 10 }, // Estimate 10% selectivity
                        );
                    }
                } else {
                    // B-tree supports all comparison operators
                    let scan_type = if index.is_unique() && op == "=" {
                        ScanType::UniqueIndexScan
                    } else {
                        ScanType::IndexScan
                    };

                    let estimated = match op {
                        "=" => if index.is_unique() { 1 } else { total_rows / 10 },
                        ">" | "<" | ">=" | "<=" => total_rows / 3, // Estimate 33% selectivity for range
                        "IN" => total_rows / 10, // Similar to equality
                        _ => total_rows,
                    };

                    return (
                        scan_type,
                        Some((idx_name.clone(), index_type_str.to_string())),
                        "O(log n)".to_string(),
                        estimated,
                    );
                }
            }
        }

        // No suitable index found
        (ScanType::SequentialScan, None, "O(n)".to_string(), total_rows)
    }

    fn format_plan(plan: &QueryPlan, no_joins: bool) -> String {
        let mut output = String::new();
        output.push_str("QUERY PLAN\n");
        output.push_str("──────────────────────────────────────────────────\n");

        match plan.scan_type {
            ScanType::SequentialScan => {
                output.push_str(&format!(
                    "→ Seq Scan on {}\n",
                    plan.table_name
                ));
                if let Some(ref filter) = plan.filter {
                    output.push_str(&format!("  Filter: {filter}\n"));
                }
            }
            ScanType::IndexScan | ScanType::UniqueIndexScan => {
                let scan_name = if matches!(plan.scan_type, ScanType::UniqueIndexScan) {
                    "Unique Index Scan"
                } else {
                    "Index Scan"
                };

                output.push_str(&format!(
                    "→ {} using {} ({})\n",
                    scan_name,
                    plan.index_name.as_ref().unwrap(),
                    plan.index_type.as_ref().unwrap()
                ));
                output.push_str(&format!("  on {}\n", plan.table_name));
                if let Some(ref filter) = plan.filter {
                    output.push_str(&format!("  Index Cond: {filter}\n"));
                }
            }
        }

        output.push_str(&format!("  Rows: ~{}\n", plan.estimated_rows));
        output.push_str(&format!("  Cost: {}\n", plan.cost));

        if !no_joins {
            output.push_str("\n  (Note: JOIN analysis not yet implemented)\n");
        }

        output.push_str("──────────────────────────────────────────────────");
        output
    }

    /// Extract column names from Equals conditions in AND chain (v1.9.0)
    fn extract_equals_columns<'a>(cond: &'a Condition, result: &mut Vec<&'a str>) {
        match cond {
            Condition::Equals(col, _) => {
                result.push(col.as_str());
            }
            Condition::And(left, right) => {
                Self::extract_equals_columns(left, result);
                Self::extract_equals_columns(right, result);
            }
            _ => {}
        }
    }

    fn format_condition(cond: &Condition) -> String {
        match cond {
            Condition::Equals(col, val) => format!("{col} = {val:?}"),
            Condition::NotEquals(col, val) => format!("{col} != {val:?}"),
            Condition::GreaterThan(col, val) => format!("{col} > {val:?}"),
            Condition::LessThan(col, val) => format!("{col} < {val:?}"),
            Condition::GreaterThanOrEqual(col, val) => format!("{col} >= {val:?}"),
            Condition::LessThanOrEqual(col, val) => format!("{col} <= {val:?}"),
            Condition::Between(col, low, high) => {
                format!("{col} BETWEEN {low:?} AND {high:?}")
            }
            Condition::Like(col, pattern) => format!("{col} LIKE '{pattern}'"),
            Condition::In(col, values) => format!("{col} IN ({values:?})"),
            Condition::IsNull(col) => format!("{col} IS NULL"),
            Condition::IsNotNull(col) => format!("{col} IS NOT NULL"),
            Condition::And(left, right) => {
                format!("({}) AND ({})", Self::format_condition(left), Self::format_condition(right))
            }
            Condition::Or(left, right) => {
                format!("({}) OR ({})", Self::format_condition(left), Self::format_condition(right))
            }
        }
    }
}
