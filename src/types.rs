use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use thiserror::Error;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub foreign_key: Option<ForeignKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForeignKey {
    pub referenced_table: String,
    pub referenced_column: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    /// Sequence counters for SERIAL columns: column_name -> next_value
    pub sequences: std::collections::HashMap<String, i64>,
}

impl Table {
    pub fn new(name: String, columns: Vec<Column>) -> Self {
        let mut sequences = std::collections::HashMap::new();

        // Initialize sequences for SERIAL and BIGSERIAL columns
        for col in &columns {
            if matches!(col.data_type, DataType::Serial | DataType::BigSerial) {
                sequences.insert(col.name.clone(), 1);
            }
        }

        Self {
            name,
            columns,
            rows: Vec::new(),
            sequences,
        }
    }

    pub fn insert(&mut self, row: Row) -> Result<(), DatabaseError> {
        if row.values.len() != self.columns.len() {
            return Err(DatabaseError::ColumnCountMismatch);
        }
        self.rows.push(row);
        Ok(())
    }

    pub fn get_column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub values: Vec<Value>,
    /// Transaction ID that created this row (for MVCC)
    pub xmin: u64,
    /// Transaction ID that deleted this row (None if still visible, for MVCC)
    pub xmax: Option<u64>,
}

impl Row {
    pub fn new(values: Vec<Value>) -> Self {
        Self {
            values,
            xmin: 0, // Will be set by TransactionManager
            xmax: None,
        }
    }

    pub fn new_with_xmin(values: Vec<Value>, xmin: u64) -> Self {
        Self {
            values,
            xmin,
            xmax: None,
        }
    }

    /// Checks if this row is visible to a given transaction (Read Committed isolation)
    pub fn is_visible(&self, current_tx_id: u64) -> bool {
        // Row is visible if:
        // 1. It was created before or in current transaction (xmin <= current_tx_id)
        // 2. AND it hasn't been deleted (xmax is None) OR was deleted by a transaction
        //    that started after current transaction (xmax > current_tx_id)
        self.xmin <= current_tx_id && self.xmax.map_or(true, |xmax| xmax > current_tx_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Database {
    pub name: String,
    pub tables: HashMap<String, Table>,
    pub enums: HashMap<String, Vec<String>>, // enum_name -> allowed values
}

impl Database {
    pub fn new(name: String) -> Self {
        Self {
            name,
            tables: HashMap::new(),
            enums: HashMap::new(),
        }
    }

    pub fn create_enum(&mut self, name: String, values: Vec<String>) -> Result<(), DatabaseError> {
        if self.enums.contains_key(&name) {
            return Err(DatabaseError::ParseError(format!("Enum '{}' already exists", name)));
        }
        self.enums.insert(name, values);
        Ok(())
    }

    pub fn get_enum(&self, name: &str) -> Option<&Vec<String>> {
        self.enums.get(name)
    }

    pub fn create_table(&mut self, table: Table) -> Result<(), DatabaseError> {
        if self.tables.contains_key(&table.name) {
            return Err(DatabaseError::TableAlreadyExists(table.name.clone()));
        }
        self.tables.insert(table.name.clone(), table);
        Ok(())
    }

    pub fn get_table(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    pub fn get_table_mut(&mut self, name: &str) -> Option<&mut Table> {
        self.tables.get_mut(name)
    }

    pub fn drop_table(&mut self, name: &str) -> Result<(), DatabaseError> {
        self.tables
            .remove(name)
            .ok_or_else(|| DatabaseError::TableNotFound(name.to_string()))?;
        Ok(())
    }
}

/// Права доступа (privileges) как в PostgreSQL
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Privilege {
    /// Право подключения к базе данных
    Connect,
    /// Право создания объектов в БД
    Create,
    /// Право на SELECT
    Select,
    /// Право на INSERT
    Insert,
    /// Право на UPDATE
    Update,
    /// Право на DELETE
    Delete,
    /// Все права
    All,
}

/// Пользователь базы данных
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    /// SHA-256 хэш пароля (hex string)
    pub password_hash: String,
    /// Является ли суперпользователем (полные права на всё)
    pub is_superuser: bool,
    /// Права на уровне сервера
    pub can_create_db: bool,
    pub can_create_user: bool,
}

impl User {
    pub fn new(username: String, password: &str, is_superuser: bool) -> Self {
        Self {
            username,
            password_hash: Self::hash_password(password),
            is_superuser,
            can_create_db: is_superuser,
            can_create_user: is_superuser,
        }
    }

    /// Хэширует пароль с использованием SHA-256
    pub fn hash_password(password: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Проверяет пароль
    pub fn verify_password(&self, password: &str) -> bool {
        self.password_hash == Self::hash_password(password)
    }

    /// Меняет пароль
    pub fn set_password(&mut self, password: &str) {
        self.password_hash = Self::hash_password(password);
    }
}

/// Метаданные базы данных (владелец и права доступа)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseMetadata {
    pub name: String,
    pub owner: String,
    /// Права доступа: username -> set of privileges
    pub privileges: HashMap<String, HashSet<Privilege>>,
}

impl DatabaseMetadata {
    pub fn new(name: String, owner: String) -> Self {
        let mut privileges = HashMap::new();
        // Владелец получает все права автоматически
        privileges.insert(
            owner.clone(),
            vec![Privilege::All].into_iter().collect(),
        );
        Self {
            name,
            owner,
            privileges,
        }
    }

    /// Выдает права пользователю
    pub fn grant(&mut self, username: &str, privilege: Privilege) {
        self.privileges
            .entry(username.to_string())
            .or_insert_with(HashSet::new)
            .insert(privilege);
    }

    /// Отбирает права у пользователя
    pub fn revoke(&mut self, username: &str, privilege: &Privilege) {
        if let Some(privs) = self.privileges.get_mut(username) {
            privs.remove(privilege);
        }
    }

    /// Проверяет, есть ли у пользователя право
    pub fn has_privilege(&self, username: &str, privilege: &Privilege) -> bool {
        if let Some(privs) = self.privileges.get(username) {
            privs.contains(&Privilege::All) || privs.contains(privilege)
        } else {
            false
        }
    }
}

/// Корневой объект сервера - содержит все БД и пользователей
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInstance {
    /// Все базы данных: name -> Database
    pub databases: HashMap<String, Database>,
    /// Метаданные баз данных: name -> DatabaseMetadata
    pub database_metadata: HashMap<String, DatabaseMetadata>,
    /// Все пользователи: username -> User
    pub users: HashMap<String, User>,
}

impl ServerInstance {
    pub fn new() -> Self {
        Self {
            databases: HashMap::new(),
            database_metadata: HashMap::new(),
            users: HashMap::new(),
        }
    }

    /// Создает начальную конфигурацию (суперпользователь + БД)
    pub fn initialize(
        superuser_name: &str,
        superuser_password: &str,
        initial_db_name: &str,
    ) -> Self {
        let mut instance = Self::new();

        // Создаем суперпользователя
        let superuser = User::new(superuser_name.to_string(), superuser_password, true);
        instance.users.insert(superuser_name.to_string(), superuser);

        // Создаем начальную БД
        let db = Database::new(initial_db_name.to_string());
        let db_meta = DatabaseMetadata::new(initial_db_name.to_string(), superuser_name.to_string());

        instance.databases.insert(initial_db_name.to_string(), db);
        instance.database_metadata.insert(initial_db_name.to_string(), db_meta);

        instance
    }

    /// Создает пользователя
    pub fn create_user(&mut self, username: &str, password: &str, is_superuser: bool) -> Result<(), DatabaseError> {
        if self.users.contains_key(username) {
            return Err(DatabaseError::UserAlreadyExists(username.to_string()));
        }
        let user = User::new(username.to_string(), password, is_superuser);
        self.users.insert(username.to_string(), user);
        Ok(())
    }

    /// Удаляет пользователя
    pub fn drop_user(&mut self, username: &str) -> Result<(), DatabaseError> {
        if !self.users.contains_key(username) {
            return Err(DatabaseError::UserNotFound(username.to_string()));
        }
        self.users.remove(username);
        Ok(())
    }

    /// Создает базу данных
    pub fn create_database(&mut self, db_name: &str, owner: &str) -> Result<(), DatabaseError> {
        if self.databases.contains_key(db_name) {
            return Err(DatabaseError::DatabaseAlreadyExists(db_name.to_string()));
        }
        if !self.users.contains_key(owner) {
            return Err(DatabaseError::UserNotFound(owner.to_string()));
        }

        let db = Database::new(db_name.to_string());
        let db_meta = DatabaseMetadata::new(db_name.to_string(), owner.to_string());

        self.databases.insert(db_name.to_string(), db);
        self.database_metadata.insert(db_name.to_string(), db_meta);

        Ok(())
    }

    /// Удаляет базу данных
    pub fn drop_database(&mut self, db_name: &str) -> Result<(), DatabaseError> {
        if !self.databases.contains_key(db_name) {
            return Err(DatabaseError::DatabaseNotFound(db_name.to_string()));
        }
        self.databases.remove(db_name);
        self.database_metadata.remove(db_name);
        Ok(())
    }

    /// Получает БД
    pub fn get_database(&self, name: &str) -> Option<&Database> {
        self.databases.get(name)
    }

    /// Получает мутабельную БД
    pub fn get_database_mut(&mut self, name: &str) -> Option<&mut Database> {
        self.databases.get_mut(name)
    }

    /// Получает метаданные БД
    pub fn get_database_metadata(&self, name: &str) -> Option<&DatabaseMetadata> {
        self.database_metadata.get(name)
    }

    /// Получает мутабельные метаданные БД
    pub fn get_database_metadata_mut(&mut self, name: &str) -> Option<&mut DatabaseMetadata> {
        self.database_metadata.get_mut(name)
    }

    /// Проверяет пароль пользователя
    pub fn authenticate(&self, username: &str, password: &str) -> bool {
        if let Some(user) = self.users.get(username) {
            user.verify_password(password)
        } else {
            false
        }
    }

    /// Проверяет, есть ли у пользователя право на БД
    pub fn check_privilege(&self, username: &str, db_name: &str, privilege: &Privilege) -> Result<bool, DatabaseError> {
        // Суперпользователь имеет все права
        if let Some(user) = self.users.get(username) {
            if user.is_superuser {
                return Ok(true);
            }
        }

        // Проверяем права в метаданных БД
        if let Some(db_meta) = self.database_metadata.get(db_name) {
            Ok(db_meta.has_privilege(username, privilege))
        } else {
            Err(DatabaseError::DatabaseNotFound(db_name.to_string()))
        }
    }
}

impl Default for ServerInstance {
    fn default() -> Self {
        Self::new()
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_display() {
        assert_eq!(Value::Null.to_string(), "NULL");
        assert_eq!(Value::Integer(42).to_string(), "42");
        assert_eq!(Value::Real(3.14).to_string(), "3.14");
        assert_eq!(Value::Text("hello".to_string()).to_string(), "hello");
        assert_eq!(Value::Boolean(true).to_string(), "true");
    }

    #[test]
    fn test_value_as_int() {
        assert_eq!(Value::Integer(42).as_int(), Some(42));
        assert_eq!(Value::Text("hello".to_string()).as_int(), None);
        assert_eq!(Value::Null.as_int(), None);
    }

    #[test]
    fn test_value_as_text() {
        assert_eq!(Value::Text("hello".to_string()).as_text(), Some("hello"));
        assert_eq!(Value::Integer(42).as_text(), None);
    }

    #[test]
    fn test_value_as_bool() {
        assert_eq!(Value::Boolean(true).as_bool(), Some(true));
        assert_eq!(Value::Boolean(false).as_bool(), Some(false));
        assert_eq!(Value::Integer(1).as_bool(), None);
    }

    #[test]
    fn test_table_creation() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
        ];

        let table = Table::new("users".to_string(), columns.clone());
        assert_eq!(table.name, "users");
        assert_eq!(table.columns.len(), 2);
        assert_eq!(table.rows.len(), 0);
    }

    #[test]
    fn test_table_insert() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
        ];

        let mut table = Table::new("users".to_string(), columns);
        let row = Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]);

        assert!(table.insert(row).is_ok());
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn test_table_insert_wrong_column_count() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
        ];

        let mut table = Table::new("users".to_string(), columns);
        let row = Row::new(vec![Value::Integer(1), Value::Text("Alice".to_string())]);

        assert!(matches!(
            table.insert(row),
            Err(DatabaseError::ColumnCountMismatch)
        ));
    }

    #[test]
    fn test_table_get_column_index() {
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Column {
                name: "name".to_string(),
                data_type: DataType::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
        ];

        let table = Table::new("users".to_string(), columns);
        assert_eq!(table.get_column_index("id"), Some(0));
        assert_eq!(table.get_column_index("name"), Some(1));
        assert_eq!(table.get_column_index("age"), None);
    }

    #[test]
    fn test_database_creation() {
        let db = Database::new("test_db".to_string());
        assert_eq!(db.name, "test_db");
        assert_eq!(db.tables.len(), 0);
    }

    #[test]
    fn test_database_create_table() {
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
        ];

        let table = Table::new("users".to_string(), columns);
        assert!(db.create_table(table).is_ok());
        assert_eq!(db.tables.len(), 1);
        assert!(db.get_table("users").is_some());
    }

    #[test]
    fn test_database_create_duplicate_table() {
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
        ];

        let table1 = Table::new("users".to_string(), columns.clone());
        let table2 = Table::new("users".to_string(), columns);

        assert!(db.create_table(table1).is_ok());
        assert!(matches!(
            db.create_table(table2),
            Err(DatabaseError::TableAlreadyExists(_))
        ));
    }

    #[test]
    fn test_database_drop_table() {
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
        ];

        let table = Table::new("users".to_string(), columns);
        db.create_table(table).unwrap();

        assert!(db.drop_table("users").is_ok());
        assert_eq!(db.tables.len(), 0);
    }

    #[test]
    fn test_database_drop_nonexistent_table() {
        let mut db = Database::new("test_db".to_string());
        assert!(matches!(
            db.drop_table("users"),
            Err(DatabaseError::TableNotFound(_))
        ));
    }

    #[test]
    fn test_value_equality() {
        assert_eq!(Value::Integer(42), Value::Integer(42));
        assert_ne!(Value::Integer(42), Value::Integer(43));
        assert_eq!(Value::Text("hello".to_string()), Value::Text("hello".to_string()));
        assert_eq!(Value::Boolean(true), Value::Boolean(true));
    }
}
