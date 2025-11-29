/// Condition evaluation for WHERE clauses
///
/// This module handles evaluation of SQL WHERE conditions against rows.
/// Supports: =, !=, >, <, AND, OR operators.

use crate::types::{Column, Row, Value, DatabaseError, Table};
use crate::parser::Condition;

pub struct ConditionEvaluator;

impl ConditionEvaluator {
    /// Evaluate condition against a row using table metadata
    pub fn evaluate(
        table: &Table,
        row: &Row,
        condition: &Condition,
    ) -> Result<bool, DatabaseError> {
        Self::evaluate_with_columns(&table.columns, row, condition)
    }

    /// Evaluate condition against a row using column metadata
    ///
    /// This is the core evaluation function that works with any column slice.
    pub fn evaluate_with_columns(
        columns: &[Column],
        row: &Row,
        condition: &Condition,
    ) -> Result<bool, DatabaseError> {
        match condition {
            Condition::Equals(col, val) => {
                let idx = Self::get_column_index(columns, col)?;
                Ok(&row.values[idx] == val)
            }
            Condition::NotEquals(col, val) => {
                let idx = Self::get_column_index(columns, col)?;
                Ok(&row.values[idx] != val)
            }
            Condition::GreaterThan(col, val) => {
                let idx = Self::get_column_index(columns, col)?;
                Self::compare_greater_than(&row.values[idx], val)
            }
            Condition::LessThan(col, val) => {
                let idx = Self::get_column_index(columns, col)?;
                Self::compare_less_than(&row.values[idx], val)
            }
            Condition::And(left, right) => {
                let left_result = Self::evaluate_with_columns(columns, row, left)?;
                let right_result = Self::evaluate_with_columns(columns, row, right)?;
                Ok(left_result && right_result)
            }
            Condition::Or(left, right) => {
                let left_result = Self::evaluate_with_columns(columns, row, left)?;
                let right_result = Self::evaluate_with_columns(columns, row, right)?;
                Ok(left_result || right_result)
            }
        }
    }

    /// Get column index by name
    fn get_column_index(columns: &[Column], col_name: &str) -> Result<usize, DatabaseError> {
        columns
            .iter()
            .position(|c| c.name == col_name)
            .ok_or_else(|| DatabaseError::ParseError(format!("Unknown column: {}", col_name)))
    }

    /// Compare two values for greater-than
    fn compare_greater_than(a: &Value, b: &Value) -> Result<bool, DatabaseError> {
        match (a, b) {
            (Value::Integer(x), Value::Integer(y)) => Ok(x > y),
            (Value::SmallInt(x), Value::SmallInt(y)) => Ok(x > y),
            (Value::Real(x), Value::Real(y)) => Ok(x > y),
            (Value::Text(x), Value::Text(y)) => Ok(x > y),
            // Cross-type numeric comparisons
            (Value::Integer(x), Value::SmallInt(y)) => Ok(*x > *y as i64),
            (Value::SmallInt(x), Value::Integer(y)) => Ok((*x as i64) > *y),
            _ => Err(DatabaseError::TypeMismatch),
        }
    }

    /// Compare two values for less-than
    fn compare_less_than(a: &Value, b: &Value) -> Result<bool, DatabaseError> {
        match (a, b) {
            (Value::Integer(x), Value::Integer(y)) => Ok(x < y),
            (Value::SmallInt(x), Value::SmallInt(y)) => Ok(x < y),
            (Value::Real(x), Value::Real(y)) => Ok(x < y),
            (Value::Text(x), Value::Text(y)) => Ok(x < y),
            // Cross-type numeric comparisons
            (Value::Integer(x), Value::SmallInt(y)) => Ok(*x < *y as i64),
            (Value::SmallInt(x), Value::Integer(y)) => Ok((*x as i64) < *y),
            _ => Err(DatabaseError::TypeMismatch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DataType, Column};

    fn create_test_columns() -> Vec<Column> {
        vec![
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
            Column {
                name: "age".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: false,
                unique: false,
                foreign_key: None,
            },
        ]
    }

    #[test]
    fn test_equals_condition() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        let cond = Condition::Equals("name".to_string(), Value::Text("Alice".to_string()));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        let cond = Condition::Equals("name".to_string(), Value::Text("Bob".to_string()));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_not_equals_condition() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        let cond = Condition::NotEquals("age".to_string(), Value::Integer(25));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        let cond = Condition::NotEquals("age".to_string(), Value::Integer(30));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_greater_than_condition() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        let cond = Condition::GreaterThan("age".to_string(), Value::Integer(25));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        let cond = Condition::GreaterThan("age".to_string(), Value::Integer(35));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_less_than_condition() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        let cond = Condition::LessThan("age".to_string(), Value::Integer(35));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        let cond = Condition::LessThan("age".to_string(), Value::Integer(25));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_and_condition() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        let cond = Condition::And(
            Box::new(Condition::Equals("name".to_string(), Value::Text("Alice".to_string()))),
            Box::new(Condition::GreaterThan("age".to_string(), Value::Integer(25))),
        );
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        let cond = Condition::And(
            Box::new(Condition::Equals("name".to_string(), Value::Text("Bob".to_string()))),
            Box::new(Condition::GreaterThan("age".to_string(), Value::Integer(25))),
        );
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_or_condition() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        let cond = Condition::Or(
            Box::new(Condition::Equals("name".to_string(), Value::Text("Bob".to_string()))),
            Box::new(Condition::GreaterThan("age".to_string(), Value::Integer(25))),
        );
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        let cond = Condition::Or(
            Box::new(Condition::Equals("name".to_string(), Value::Text("Bob".to_string()))),
            Box::new(Condition::LessThan("age".to_string(), Value::Integer(25))),
        );
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_unknown_column() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        let cond = Condition::Equals("unknown".to_string(), Value::Integer(1));
        let result = ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond);
        assert!(result.is_err());
    }
}
