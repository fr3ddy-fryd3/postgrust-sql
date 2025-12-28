// Subquery execution (v2.6.0)
//
// This module handles execution of subqueries in various contexts:
// - Scalar subqueries (SELECT col WHERE x = (SELECT ...))
// - IN/NOT IN subqueries (SELECT * WHERE id IN (SELECT ...))
// - EXISTS/NOT EXISTS subqueries (SELECT * WHERE EXISTS (SELECT ...))
// - FROM subqueries (derived tables)

use crate::core::{Database, DatabaseError, Value};
use crate::executor::queries::QueryExecutor;
use crate::parser::Statement;
use crate::storage::DatabaseStorage;
use crate::transaction::GlobalTransactionManager;

/// Context for subquery execution
///
/// Stores information about the outer query's row for correlated subqueries
#[derive(Debug, Clone)]
pub struct SubqueryContext {
    /// Column names from outer query
    pub outer_columns: Vec<String>,
    /// Current row values from outer query
    pub outer_row: Vec<Value>,
}

impl SubqueryContext {
    /// Create a new empty context
    pub fn new() -> Self {
        Self {
            outer_columns: Vec::new(),
            outer_row: Vec::new(),
        }
    }

    /// Create context with outer query data
    pub fn with_outer_row(columns: Vec<String>, row: Vec<Value>) -> Self {
        Self {
            outer_columns: columns,
            outer_row: row,
        }
    }

    /// Get value from outer row by column name
    pub fn get_outer_value(&self, col_name: &str) -> Option<&Value> {
        self.outer_columns
            .iter()
            .position(|c| c == col_name)
            .and_then(|idx| self.outer_row.get(idx))
    }
}

impl Default for SubqueryContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Subquery executor
pub struct SubqueryExecutor;

impl SubqueryExecutor {
    /// Execute a scalar subquery (returns single value)
    ///
    /// Used in contexts like: WHERE price = (SELECT MAX(price) FROM products)
    /// Returns error if subquery returns 0 or >1 rows/columns
    pub fn execute_scalar(
        db: &Database,
        stmt: &Statement,
        tx_manager: &GlobalTransactionManager,
        database_storage: &DatabaseStorage,
        _context: &SubqueryContext,
    ) -> Result<Value, DatabaseError> {
        // Unpack Statement::Select and call QueryExecutor::select
        let result = match stmt {
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
            } => QueryExecutor::select(
                db,
                *distinct,
                columns.clone(),
                from.clone(),
                joins.clone(),
                filter.clone(),
                group_by.clone(),
                order_by.clone(),
                *limit,
                *offset,
                tx_manager,
                database_storage,
            )?,
            _ => {
                return Err(DatabaseError::ParseError(
                    "Subquery must be a SELECT statement".to_string(),
                ))
            }
        };

        if let crate::executor::QueryResult::Rows(rows, columns) = result {
            // Scalar subquery must return exactly 1 row and 1 column
            if rows.is_empty() {
                return Ok(Value::Null);
            }

            if rows.len() > 1 {
                return Err(DatabaseError::ParseError(
                    "Scalar subquery returned more than one row".to_string(),
                ));
            }

            if columns.len() > 1 {
                return Err(DatabaseError::ParseError(
                    "Scalar subquery returned more than one column".to_string(),
                ));
            }

            // Parse the string value back to Value
            // TODO: This is a hack - we should preserve types better
            let value_str = &rows[0][0];
            Ok(crate::types::Value::Text(value_str.clone()))
        } else {
            Err(DatabaseError::ParseError(
                "Scalar subquery did not return rows".to_string(),
            ))
        }
    }

    /// Execute an EXISTS subquery (returns bool)
    ///
    /// Returns true if subquery returns any rows, false otherwise
    pub fn execute_exists(
        db: &Database,
        stmt: &Statement,
        tx_manager: &GlobalTransactionManager,
        database_storage: &DatabaseStorage,
        _context: &SubqueryContext,
    ) -> Result<bool, DatabaseError> {
        let result = match stmt {
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
            } => QueryExecutor::select(
                db,
                *distinct,
                columns.clone(),
                from.clone(),
                joins.clone(),
                filter.clone(),
                group_by.clone(),
                order_by.clone(),
                *limit,
                *offset,
                tx_manager,
                database_storage,
            )?,
            _ => {
                return Err(DatabaseError::ParseError(
                    "Subquery must be a SELECT statement".to_string(),
                ))
            }
        };

        if let crate::executor::QueryResult::Rows(rows, _) = result {
            Ok(!rows.is_empty())
        } else {
            Ok(false)
        }
    }

    /// Execute an IN subquery (returns list of values)
    ///
    /// Used in: WHERE id IN (SELECT user_id FROM orders)
    /// Returns all values from first column of result set
    pub fn execute_in(
        db: &Database,
        stmt: &Statement,
        tx_manager: &GlobalTransactionManager,
        database_storage: &DatabaseStorage,
        _context: &SubqueryContext,
    ) -> Result<Vec<Value>, DatabaseError> {
        let result = match stmt {
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
            } => QueryExecutor::select(
                db,
                *distinct,
                columns.clone(),
                from.clone(),
                joins.clone(),
                filter.clone(),
                group_by.clone(),
                order_by.clone(),
                *limit,
                *offset,
                tx_manager,
                database_storage,
            )?,
            _ => {
                return Err(DatabaseError::ParseError(
                    "Subquery must be a SELECT statement".to_string(),
                ))
            }
        };

        if let crate::executor::QueryResult::Rows(rows, columns) = result {
            if columns.is_empty() {
                return Ok(Vec::new());
            }

            // Take first column from each row
            let values: Vec<Value> = rows
                .into_iter()
                .map(|row| {
                    // TODO: Parse string back to proper Value type
                    crate::types::Value::Text(row[0].clone())
                })
                .collect();

            Ok(values)
        } else {
            Ok(Vec::new())
        }
    }
}
