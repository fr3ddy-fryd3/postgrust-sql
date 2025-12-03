// PostgrustSQL - PostgreSQL-compatible database in Rust
// Modular architecture for maintainability and extensibility

// Core database structures
pub mod core;

// Backward compatibility - re-export all core types as types module
pub mod types {
    pub use crate::core::*;
}

// SQL parser (DDL, DML, queries, meta-commands)
pub mod parser;

// Query executor (DDL, DML, SELECT, JOINs, aggregates, filters, meta)
pub mod executor;

// Transaction management (MVCC, snapshot isolation)
pub mod transaction;

// Storage layer (disk persistence, WAL)
pub mod storage;

// Index structures (B-tree, hash indexes)
pub mod index;

// Network protocols (TCP server, text protocol, PostgreSQL wire protocol)
pub mod network;

// Re-export commonly used types for convenience
pub use core::{Database, Table, Row, Value, Column, DataType, ForeignKey, DatabaseError, ServerInstance};
pub use parser::{Statement, parse_statement};
pub use executor::{QueryExecutor, QueryResult};
pub use transaction::{Transaction, TransactionManager};
pub use storage::StorageEngine;
pub use network::Server;
