// PostgrustSQL - PostgreSQL-compatible database in Rust
// Modular architecture for maintainability and extensibility

// Clippy configuration - allow non-critical warnings for pet project
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::match_same_arms)]
#![allow(clippy::significant_drop_tightening)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::cognitive_complexity)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::format_push_string)]
#![allow(clippy::wildcard_enum_match_arm)]
#![allow(clippy::inefficient_to_string)]
#![allow(clippy::items_after_statements)]
#![allow(clippy::ref_option_ref)]
#![allow(clippy::drop_non_drop)]
#![allow(clippy::clone_on_copy)]
#![allow(clippy::type_complexity)]
#![allow(clippy::mut_from_ref)]

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
