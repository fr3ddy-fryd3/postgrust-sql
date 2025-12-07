/// Condition evaluation for WHERE clauses
///
/// This module handles evaluation of SQL WHERE conditions against rows.
/// Supports: =, !=, >, <, >=, <=, BETWEEN, LIKE, IN, IS NULL, AND, OR operators (v1.8.0).

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
            Condition::GreaterThanOrEqual(col, val) => {
                let idx = Self::get_column_index(columns, col)?;
                let gt = Self::compare_greater_than(&row.values[idx], val)?;
                let eq = &row.values[idx] == val;
                Ok(gt || eq)
            }
            Condition::LessThanOrEqual(col, val) => {
                let idx = Self::get_column_index(columns, col)?;
                let lt = Self::compare_less_than(&row.values[idx], val)?;
                let eq = &row.values[idx] == val;
                Ok(lt || eq)
            }
            Condition::Between(col, low, high) => {
                let idx = Self::get_column_index(columns, col)?;
                let val = &row.values[idx];
                let ge_low = Self::compare_greater_than(val, low)? || val == low;
                let le_high = Self::compare_less_than(val, high)? || val == high;
                Ok(ge_low && le_high)
            }
            Condition::Like(col, pattern) => {
                let idx = Self::get_column_index(columns, col)?;
                Self::match_like(&row.values[idx], pattern)
            }
            Condition::In(col, values) => {
                let idx = Self::get_column_index(columns, col)?;
                Ok(values.contains(&row.values[idx]))
            }
            Condition::IsNull(col) => {
                let idx = Self::get_column_index(columns, col)?;
                Ok(matches!(row.values[idx], Value::Null))
            }
            Condition::IsNotNull(col) => {
                let idx = Self::get_column_index(columns, col)?;
                Ok(!matches!(row.values[idx], Value::Null))
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

    /// Match LIKE pattern (v1.8.0)
    /// Supports: % (any chars), _ (single char)
    fn match_like(value: &Value, pattern: &str) -> Result<bool, DatabaseError> {
        match value {
            Value::Text(text) => Ok(Self::like_pattern_match(text, pattern)),
            Value::Null => Ok(false), // NULL doesn't match anything
            _ => Err(DatabaseError::TypeMismatch),
        }
    }

    /// Simple LIKE pattern matching
    /// % matches zero or more characters
    /// _ matches exactly one character
    fn like_pattern_match(text: &str, pattern: &str) -> bool {
        let mut text_chars: Vec<char> = text.chars().collect();
        let mut pattern_chars: Vec<char> = pattern.chars().collect();

        Self::match_recursive(&text_chars, &pattern_chars, 0, 0)
    }

    fn match_recursive(text: &[char], pattern: &[char], ti: usize, pi: usize) -> bool {
        // Both exhausted - match
        if pi >= pattern.len() && ti >= text.len() {
            return true;
        }

        // Pattern exhausted but text remains - no match
        if pi >= pattern.len() {
            return false;
        }

        // Handle % wildcard
        if pattern[pi] == '%' {
            // % can match zero characters
            if Self::match_recursive(text, pattern, ti, pi + 1) {
                return true;
            }
            // % can match one or more characters
            if ti < text.len() && Self::match_recursive(text, pattern, ti + 1, pi) {
                return true;
            }
            return false;
        }

        // Text exhausted but pattern has non-% chars - no match
        if ti >= text.len() {
            return false;
        }

        // Handle _ wildcard (matches exactly one char)
        if pattern[pi] == '_' {
            return Self::match_recursive(text, pattern, ti + 1, pi + 1);
        }

        // Exact character match
        if text[ti] == pattern[pi] {
            return Self::match_recursive(text, pattern, ti + 1, pi + 1);
        }

        false
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

    // v1.8.0 tests
    #[test]
    fn test_greater_than_or_equal() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        // age >= 30 should be true (equals)
        let cond = Condition::GreaterThanOrEqual("age".to_string(), Value::Integer(30));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age >= 25 should be true (greater)
        let cond = Condition::GreaterThanOrEqual("age".to_string(), Value::Integer(25));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age >= 35 should be false
        let cond = Condition::GreaterThanOrEqual("age".to_string(), Value::Integer(35));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_less_than_or_equal() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        // age <= 30 should be true (equals)
        let cond = Condition::LessThanOrEqual("age".to_string(), Value::Integer(30));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age <= 35 should be true (less)
        let cond = Condition::LessThanOrEqual("age".to_string(), Value::Integer(35));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age <= 25 should be false
        let cond = Condition::LessThanOrEqual("age".to_string(), Value::Integer(25));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_between() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        // age BETWEEN 25 AND 35 should be true
        let cond = Condition::Between("age".to_string(), Value::Integer(25), Value::Integer(35));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age BETWEEN 30 AND 30 should be true (inclusive)
        let cond = Condition::Between("age".to_string(), Value::Integer(30), Value::Integer(30));
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age BETWEEN 35 AND 40 should be false
        let cond = Condition::Between("age".to_string(), Value::Integer(35), Value::Integer(40));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age BETWEEN 10 AND 25 should be false
        let cond = Condition::Between("age".to_string(), Value::Integer(10), Value::Integer(25));
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_like_pattern() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        // name LIKE 'A%' should be true
        let cond = Condition::Like("name".to_string(), "A%".to_string());
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // name LIKE '%ice' should be true
        let cond = Condition::Like("name".to_string(), "%ice".to_string());
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // name LIKE '%li%' should be true
        let cond = Condition::Like("name".to_string(), "%li%".to_string());
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // name LIKE 'A____' should be true (5 chars)
        let cond = Condition::Like("name".to_string(), "A____".to_string());
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // name LIKE 'B%' should be false
        let cond = Condition::Like("name".to_string(), "B%".to_string());
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // name LIKE 'Alice' should be true (exact match)
        let cond = Condition::Like("name".to_string(), "Alice".to_string());
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_in_list() {
        let columns = create_test_columns();
        let row = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        // age IN (25, 30, 35) should be true
        let cond = Condition::In(
            "age".to_string(),
            vec![Value::Integer(25), Value::Integer(30), Value::Integer(35)],
        );
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // age IN (20, 25) should be false
        let cond = Condition::In(
            "age".to_string(),
            vec![Value::Integer(20), Value::Integer(25)],
        );
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());

        // name IN ('Alice', 'Bob') should be true
        let cond = Condition::In(
            "name".to_string(),
            vec![Value::Text("Alice".to_string()), Value::Text("Bob".to_string())],
        );
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row, &cond).unwrap());
    }

    #[test]
    fn test_is_null() {
        let mut columns = create_test_columns();
        columns[1].nullable = true; // Make name nullable

        // Row with NULL name
        let row_with_null = Row::new(vec![
            Value::Integer(1),
            Value::Null,
            Value::Integer(30),
        ]);

        // Row with non-NULL name
        let row_without_null = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        // name IS NULL should be true for row with NULL
        let cond = Condition::IsNull("name".to_string());
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row_with_null, &cond).unwrap());

        // name IS NULL should be false for row without NULL
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row_without_null, &cond).unwrap());
    }

    #[test]
    fn test_is_not_null() {
        let mut columns = create_test_columns();
        columns[1].nullable = true; // Make name nullable

        // Row with NULL name
        let row_with_null = Row::new(vec![
            Value::Integer(1),
            Value::Null,
            Value::Integer(30),
        ]);

        // Row with non-NULL name
        let row_without_null = Row::new(vec![
            Value::Integer(1),
            Value::Text("Alice".to_string()),
            Value::Integer(30),
        ]);

        // name IS NOT NULL should be false for row with NULL
        let cond = Condition::IsNotNull("name".to_string());
        assert!(!ConditionEvaluator::evaluate_with_columns(&columns, &row_with_null, &cond).unwrap());

        // name IS NOT NULL should be true for row without NULL
        assert!(ConditionEvaluator::evaluate_with_columns(&columns, &row_without_null, &cond).unwrap());
    }
}
