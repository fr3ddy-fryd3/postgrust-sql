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
    #[must_use] 
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Self::Integer(i) => Some(*i),
            _ => None,
        }
    }

    #[allow(dead_code)]
    #[must_use] 
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    #[allow(dead_code)]
    #[must_use] 
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => write!(f, "NULL"),
            Self::SmallInt(i) => write!(f, "{i}"),
            Self::Integer(i) => write!(f, "{i}"),
            Self::Real(r) => write!(f, "{r}"),
            Self::Numeric(d) => write!(f, "{d}"),
            Self::Text(s) => write!(f, "{s}"),
            Self::Char(s) => write!(f, "{s}"),
            Self::Boolean(b) => write!(f, "{b}"),
            Self::Date(d) => write!(f, "{}", d.format("%Y-%m-%d")),
            Self::Timestamp(t) => write!(f, "{}", t.format("%Y-%m-%d %H:%M:%S")),
            Self::TimestampTz(t) => write!(f, "{}", t.format("%Y-%m-%d %H:%M:%S %Z")),
            Self::Uuid(u) => write!(f, "{u}"),
            Self::Json(j) => write!(f, "{j}"),
            Self::Bytea(b) => write!(f, "\\x{}", hex::encode(b)),
            Self::Enum(_, v) => write!(f, "{v}"),
        }
    }
}
