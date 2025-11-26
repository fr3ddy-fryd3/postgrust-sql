use serde::{Deserialize, Serialize};
use chrono::{NaiveDate, NaiveDateTime, DateTime, Utc};
use uuid::Uuid;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
    Null,
    // Numeric types
    SmallInt(i16),
    Integer(i64),
    Real(f64),
    Numeric(Decimal),  // NUMERIC/DECIMAL with precision
    // String types
    Text(String),
    Char(String),      // Fixed-length CHAR(n)
    // Boolean
    Boolean(bool),
    // Date/Time types
    Date(NaiveDate),
    Timestamp(NaiveDateTime),
    TimestampTz(DateTime<Utc>),
    // Special types
    Uuid(Uuid),
    Json(String),      // JSON as text
    Bytea(Vec<u8>),    // Binary data
    Enum(String, String), // (enum_name, value)
}

impl Value {
    #[allow(dead_code)]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Integer(i) => Some(*i),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Boolean(b) => Some(*b),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "NULL"),
            Value::SmallInt(i) => write!(f, "{}", i),
            Value::Integer(i) => write!(f, "{}", i),
            Value::Real(r) => write!(f, "{}", r),
            Value::Numeric(d) => write!(f, "{}", d),
            Value::Text(s) => write!(f, "{}", s),
            Value::Char(s) => write!(f, "{}", s),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Date(d) => write!(f, "{}", d.format("%Y-%m-%d")),
            Value::Timestamp(t) => write!(f, "{}", t.format("%Y-%m-%d %H:%M:%S")),
            Value::TimestampTz(t) => write!(f, "{}", t.format("%Y-%m-%d %H:%M:%S %Z")),
            Value::Uuid(u) => write!(f, "{}", u),
            Value::Json(j) => write!(f, "{}", j),
            Value::Bytea(b) => write!(f, "\\x{}", hex::encode(b)),
            Value::Enum(_, v) => write!(f, "{}", v),
        }
    }
}
