use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataType {
    // Numeric types
    SmallInt,
    Integer,
    Real,
    Numeric { precision: u8, scale: u8 }, // NUMERIC(p, s)
    Serial,       // Auto-incrementing INTEGER
    BigSerial,    // Auto-incrementing BIGINT
    // String types
    Text,
    Varchar { max_length: usize },  // VARCHAR(n)
    Char { length: usize },         // CHAR(n)
    // Boolean
    Boolean,
    // Date/Time types
    Date,
    Timestamp,
    TimestampTz,
    // Special types
    Uuid,
    Json,
    Jsonb,  // Binary JSON (stored same as JSON for now)
    Bytea,
    Enum { name: String, values: Vec<String> },
}
