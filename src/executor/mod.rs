/// Executor module - handles SQL statement execution
///
/// This module is being refactored from a monolithic executor.rs (3009 lines)
/// into organized submodules for better maintainability.
///
/// Structure:
/// - legacy: Original monolithic executor (temporary, will be split)
/// - storage_adapter: Abstraction over Vec<Row> and PagedTable ✅
/// - ddl: CREATE/DROP/ALTER TABLE operations (TODO)
/// - dml: INSERT/UPDATE/DELETE operations (TODO)
/// - queries: SELECT operations (regular, aggregate, join, group by) (TODO)
/// - conditions: WHERE clause evaluation (TODO)

// Legacy monolithic executor (3009 lines) - to be refactored
#[path = "legacy.rs"]
mod legacy_executor;

// New modular components
pub mod storage_adapter;

// Re-export legacy executor for backward compatibility
pub use legacy_executor::{QueryExecutor, QueryResult};

// Re-exports for convenience
pub use storage_adapter::{RowStorage, LegacyStorage};

#[cfg(feature = "page_storage")]
pub use storage_adapter::PagedStorage;
