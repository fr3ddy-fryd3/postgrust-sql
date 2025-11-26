use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Table '{0}' not found")]
    TableNotFound(String),
    #[error("Table '{0}' already exists")]
    TableAlreadyExists(String),
    #[error("Column '{0}' not found")]
    ColumnNotFound(String),
    #[error("Column count mismatch")]
    ColumnCountMismatch,
    #[error("Type mismatch")]
    TypeMismatch,
    #[error("Database '{0}' not found")]
    DatabaseNotFound(String),
    #[error("Database '{0}' already exists")]
    DatabaseAlreadyExists(String),
    #[error("User '{0}' not found")]
    UserNotFound(String),
    #[error("User '{0}' already exists")]
    UserAlreadyExists(String),
    #[error("Authentication failed")]
    AuthenticationFailed,
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Foreign key constraint violation: {0}")]
    ForeignKeyViolation(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Binary Serialization error: {0}")]
    BinarySerialization(String),
    #[error("Parse error: {0}")]
    ParseError(String),
}
