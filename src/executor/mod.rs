/// Executor module - handles SQL statement execution
///
/// This module is being refactored from a monolithic executor.rs (3009 lines)
/// into organized submodules for better maintainability.
///
/// Structure:
/// - legacy: Original monolithic executor (temporary, will be split)
/// - storage_adapter: Abstraction over Vec<Row> and PagedTable ✅
/// - conditions: WHERE clause evaluation ✅
/// - dml: INSERT/UPDATE/DELETE operations ✅
/// - ddl: CREATE/DROP/ALTER TABLE operations ✅
/// - queries: SELECT operations (regular, aggregate, join, group by) (TODO)

// Legacy monolithic executor (3009 lines) - to be refactored
#[path = "legacy.rs"]
mod legacy_executor;

// New modular components
pub mod storage_adapter;
pub mod conditions;
pub mod dml;
pub mod ddl;
pub mod queries;
pub mod vacuum;
pub mod index;
pub mod explain;  // v1.8.0

// Re-export legacy executor for backward compatibility
pub use legacy_executor::{QueryExecutor, QueryResult};

// Re-export new modular components
pub use storage_adapter::{RowStorage, LegacyStorage};
pub use conditions::ConditionEvaluator;
pub use dml::DmlExecutor;
pub use ddl::DdlExecutor;
pub use queries::QueryExecutor as QueriesExecutor;
pub use vacuum::VacuumExecutor;
pub use index::IndexExecutor;
pub use explain::ExplainExecutor;  // v1.8.0

#[cfg(feature = "page_storage")]
pub use storage_adapter::PagedStorage;
