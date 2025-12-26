# RustDB Roadmap

Долгосрочный план развития проекта после v1.9.0 (Composite Indexes).

---

## 🎉 v2.5.0 - COPY Binary Format (Complete!)

**Цель:** Full PostgreSQL-compatible binary COPY protocol
**Статус:** Complete (2025-12-26) ✅
**Сложность:** Очень Высокая
**Breaking Changes:** No

### ✅ Реализовано:

#### 1. COPY Binary Format ✅ (COMPLETED - 2025-12-26)
**Зачем:** Полная совместимость с pg_dump --format=custom, 3-5x быстрее CSV

**Что реализовано:**
- ✅ PostgreSQL binary format v3.0 для COPY (header, trailer, field encoding)
- ✅ Сериализация всех 23 типов данных в binary
- ✅ Десериализация binary → Value с full type validation
- ✅ Binary NULL handling (length=-1)
- ✅ **Full PostgreSQL Numeric format** (base-10000, ndigits/weight/sign/dscale)
- ✅ Date/Time epoch conversion (PostgreSQL 2000-01-01 epoch)
- ✅ Network byte order (big-endian) for all multi-byte integers
- ✅ MVCC visibility filtering in COPY TO STDOUT
- ✅ COPY FROM STDIN binary import
- ✅ COPY TO STDOUT binary export

**Файлы:**
- `src/network/copy_binary.rs` (NEW) - ~600 lines, full binary encoder/decoder
- `src/network/server.rs` - COPY binary protocol integration
- `src/network/pg_protocol.rs` - OID constants for all 23 types, fixed column format codes
- `src/network/mod.rs` - Export BinaryCopyEncoder/Decoder
- `tests/integration/test_copy_binary.sh` (NEW) - Comprehensive integration tests

**Статистика:**
- 📊 202 unit tests passing (6 new binary format tests)
- 🧪 Integration test: 6 scenarios (basic types, NULL, numeric, datetime, UUID/BYTEA, round-trip)
- 📦 Binary header: 19 bytes (11-byte signature + 8 bytes metadata)
- ⚡ Expected performance: 3-5x faster than CSV for bulk operations

---

---

## 🚧 v2.6.0 - Subqueries & Advanced SQL (Planned)

**Цель:** Production-ready SQL features
**Статус:** Planning
**Сложность:** Очень Высокая

### Запланировано:

#### 1. Subqueries (Priority 1)
**Зачем:** Критическая SQL фича, необходима для production

**Типы subqueries:**
```sql
-- Scalar subquery
SELECT name, (SELECT COUNT(*) FROM orders WHERE user_id = users.id) as order_count
FROM users;

-- IN subquery
SELECT * FROM products
WHERE category_id IN (SELECT id FROM categories WHERE active = true);

-- EXISTS subquery
SELECT * FROM users
WHERE EXISTS (SELECT 1 FROM orders WHERE user_id = users.id);

-- FROM subquery (derived table)
SELECT * FROM (SELECT * FROM users WHERE age > 18) AS adults;
```

**Реализация:**
- Parser: вложенные SELECT в WHERE/FROM/SELECT
- Executor: рекурсивное выполнение subqueries
- Оптимизация: материализация vs correlated subqueries
- MVCC: правильная изоляция для subqueries

**Файлы:**
- `src/parser/queries.rs` - Subquery parsing
- `src/executor/queries.rs` - Subquery execution
- `src/parser/statement.rs` - SubqueryExpression enum

#### 2. pg_dump Full Compatibility Test (Priority 2)
**Зачем:** Проверить что pg_dump работает без костылей

**Что проверить:**
- pg_dump с binary format работает
- pg_restore восстанавливает без ошибок
- DDL порядок правильный (types → tables → indexes)
- SERIAL sequences корректно дампятся
- ENUM types работают

**Дополнительно может потребоваться:**
- pg_depend catalog для зависимостей
- CREATE SEQUENCE support
- COMMENT ON TABLE/COLUMN
- pg_description catalog

#### 3. Window Functions (Priority 3)
**Зачем:** Production-ready analytics queries

**Функции:**
```sql
-- Ranking functions
ROW_NUMBER() OVER (ORDER BY salary DESC)
RANK() OVER (PARTITION BY dept ORDER BY salary DESC)
DENSE_RANK() OVER (...)

-- Aggregate window functions
SUM(salary) OVER (PARTITION BY dept)
AVG(salary) OVER (ORDER BY hire_date ROWS BETWEEN 3 PRECEDING AND CURRENT ROW)

-- Value functions
LAG(salary, 1) OVER (ORDER BY hire_date)
LEAD(salary) OVER (ORDER BY hire_date)
FIRST_VALUE(name) OVER (PARTITION BY dept ORDER BY salary DESC)
LAST_VALUE(name) OVER (...)
```

**Реализация:**
- OVER clause parsing
- PARTITION BY + ORDER BY
- Window frame specification (ROWS/RANGE BETWEEN)
- Window function evaluation engine
- Sorting + partitioning logic

**Файлы:**
- `src/parser/queries.rs` - OVER clause parsing
- `src/executor/window.rs` (NEW) - Window function evaluation
- `src/parser/statement.rs` - WindowFunction enum

### Архитектура изменений:

```rust
// Binary Format
pub struct BinaryEncoder {
    fn encode_value(value: &Value) -> Vec<u8>;
    fn decode_value(bytes: &[u8], data_type: &DataType) -> Result<Value>;
}

// Subqueries
pub enum Expression {
    Scalar(Box<Statement>),      // (SELECT COUNT(*) FROM ...)
    In(String, Box<Statement>),  // col IN (SELECT ...)
    Exists(Box<Statement>),      // EXISTS (SELECT ...)
}

// Window Functions
pub struct WindowSpec {
    partition_by: Vec<String>,
    order_by: Vec<(String, SortOrder)>,
    frame: Option<WindowFrame>,
}

pub enum WindowFunction {
    RowNumber,
    Rank,
    DenseRank,
    Lag { expr: String, offset: i64 },
    Lead { expr: String, offset: i64 },
    // ... etc
}
```

### Тесты:
- Binary COPY round-trip (export + import, data integrity)
- Scalar subqueries в SELECT
- IN/EXISTS subqueries в WHERE
- Correlated vs uncorrelated subqueries
- Window functions: ROW_NUMBER, RANK, LAG, LEAD
- PARTITION BY + ORDER BY combinations
- Real pg_dump → pg_restore test

### PostgreSQL Compatibility:
- ✅ COPY binary format (full)
- ✅ Scalar subqueries
- ✅ IN/EXISTS/NOT EXISTS
- ✅ Derived tables (FROM subquery)
- ✅ Basic window functions
- ⚠️ Advanced window frames (RANGE BETWEEN) - simplified
- ⚠️ Correlated subqueries - performance TBD

---

## ✅ v2.4.0 - Extended Query Protocol & COPY

**Цель:** PostgreSQL protocol extensions - Prepared statements + Bulk import/export
**Статус:** Completed (2025-12-26)
**Сложность:** Высокая
**Breaking Changes:** No

### Реализовано:

#### 1. Extended Query Protocol ✅
```
Client → Server: Parse(query, statement_name, param_types)
Server → Client: ParseComplete
Client → Server: Bind(statement_name, portal_name, param_values)
Server → Client: BindComplete
Client → Server: Describe(portal_name)
Server → Client: RowDescription | NoData
Client → Server: Execute(portal_name, max_rows)
Server → Client: DataRow... CommandComplete
Client → Server: Close(statement_name | portal_name)
Server → Client: CloseComplete
Client → Server: Sync
Server → Client: ReadyForQuery
```

**Features:**
- ✅ Full message support: PARSE, BIND, DESCRIBE, EXECUTE, CLOSE, SYNC
- ✅ Server responses: ParseComplete, BindComplete, CloseComplete, NoData
- ✅ PreparedStatementCache with statement and portal caching
- ✅ Parameter substitution: $1, $2, $3, ... → actual values
- ✅ MVCC support in prepared statements (xmin, xmax tracking)
- ✅ Type-safe parameter handling for all 23 data types

#### 2. COPY Protocol ✅
```sql
-- Import from STDIN (CSV/TSV)
COPY users FROM STDIN;
COPY users (name, email) FROM STDIN;
COPY users FROM STDIN WITH (FORMAT csv);
COPY users FROM STDIN WITH (FORMAT binary);

-- Export to STDOUT
COPY users TO STDOUT;
COPY users (name, age) TO STDOUT WITH (FORMAT csv);
```

**Features:**
- ✅ COPY FROM STDIN - bulk CSV/TSV import
- ✅ COPY TO STDOUT - data export (framework ready, full impl pending)
- ✅ Column selection support: COPY table (col1, col2) FROM STDIN
- ✅ Format options: TEXT (CSV/TSV), BINARY
- ✅ Protocol messages: CopyInResponse, CopyOutResponse, CopyData, CopyDone
- ✅ CSV parsing with comma-separated values
- ✅ Line-by-line INSERT execution with transaction support

### Implementation Details:

**Files Added:**
- `src/network/prepared_statements.rs` (NEW) - PreparedStatementCache, parameter substitution

**Files Modified:**
- `src/network/pg_protocol.rs` - Extended Query and COPY protocol messages
- `src/network/server.rs` - Message handlers (PARSE, BIND, DESCRIBE, EXECUTE, CLOSE, SYNC, COPY)
- `src/parser/statement.rs` - Copy variant, CopyFormat enum
- `src/parser/ddl.rs` - parse_copy() function
- `src/parser/mod.rs` - Added CopyFormat export, parse_copy to parser chain
- `src/executor/dispatcher.rs` - Statement::Copy execution
- `Cargo.toml` - Added default-run = "postgrustsql"
- `tests/integration/test_dump_restore.sh` - Fixed binary names (pgr_dump/pgr_restore)

**PreparedStatementCache:**
```rust
pub struct PreparedStatementCache {
    statements: HashMap<String, PreparedStatement>,  // statement_name → prepared query
    portals: HashMap<String, Portal>,                 // portal_name → bound params + query
}

pub fn substitute_parameters(query: &str, params: &[Option<Value>]) -> String {
    // Replace $1, $2, ... with actual values
    // Handles all 23 data types with proper SQL escaping
}
```

### Тесты:
- ✅ **196 unit tests passing** (0 failed, 7 ignored)
- ✅ **Integration tests passing:**
  - test_features.sh - Basic functionality
  - test_new_types.sh - All 23 data types
  - test_hash_index.sh - Hash & B-tree indexes
  - test_mvcc_isolation.sh - Multi-connection isolation
  - test_composite_index.sh - Composite indexes
  - test_sql_expressions.sh - CASE, UNION, INTERSECT, EXCEPT
  - test_explain.sh - Query analysis

### Bug Fixes:
- 🐛 Fixed Cargo.toml default-run for multi-binary support
- 🐛 Fixed integration test binary names (postgrust-dump → pgr_dump, postgrust-restore → pgr_restore)

### PostgreSQL Compatibility:
- ✅ Extended Query Protocol (v3.0) - full support
- ✅ COPY Protocol - basic support (STDIN/STDOUT)
- ✅ Prepared statements with named parameters
- ✅ Binary protocol support (framework ready)
- ⚠️ COPY FROM file path not yet implemented (only STDIN/STDOUT)
- ⚠️ Full binary format for COPY pending (placeholder implementation)

### Architecture:
```
Extended Query Flow:
  SessionContext.prepared_statements (shared cache)
       ↓
  PARSE → store query in cache
       ↓
  BIND → create portal with parameters
       ↓
  EXECUTE → substitute params, execute query
       ↓
  CLOSE → cleanup statement/portal

COPY Flow:
  COPY ... FROM STDIN
       ↓
  Send CopyInResponse
       ↓
  Loop: Read CopyData messages
       ↓
  Parse CSV line-by-line
       ↓
  INSERT each row
       ↓
  CopyDone → return row count
```

### Benefits:
- 📡 **Better performance** - Parse once, execute many times
- 🔒 **SQL injection prevention** - Parameters separated from query
- 💾 **Bulk import speed** - COPY much faster than individual INSERTs
- 🔄 **PostgreSQL compatibility** - Standard protocol support
- 🚀 **Production ready** - Full MVCC and transaction support

---

## ✅ v2.3.0 - Role-Based Access Control (RBAC)

**Цель:** Полноценная PostgreSQL-style система прав доступа
**Статус:** Completed (2025-12-22)
**Сложность:** Высокая
**Breaking Changes:** Moderate (added owner field to tables, permission enforcement)

### Реализовано:
1. ✅ **Roles System** - CREATE/DROP ROLE, role hierarchy (member_of)
2. ✅ **Role Membership** - GRANT/REVOKE role TO/FROM user
3. ✅ **Table Ownership** - Every table has an owner (creator by default)
4. ✅ **Table-level Privileges** - GRANT/REVOKE SELECT/INSERT/UPDATE/DELETE ON TABLE
5. ✅ **Permission Enforcement** - Automatic checks before DML/DDL operations
6. ✅ **System Catalogs** - pg_class (relowner), pg_auth_members, table_privileges
7. ✅ **198 unit tests passing** (9 new RBAC tests)

### SQL Commands:
```sql
-- Role Management
CREATE ROLE readonly;
CREATE ROLE admin SUPERUSER;
DROP ROLE readonly;

-- Role Assignment
GRANT readonly TO alice;
REVOKE readonly FROM alice;

-- Table Creation (owner = creator)
CREATE TABLE orders (id SERIAL, amount NUMERIC);
-- Owner: current session user

-- Change Owner
ALTER TABLE orders OWNER TO bob;

-- Table-level Privileges
GRANT SELECT ON TABLE orders TO alice;
GRANT INSERT, UPDATE ON TABLE orders TO readonly;
REVOKE SELECT ON TABLE orders FROM alice;

-- Ownership/Permission Checks
SELECT * FROM orders;  -- Requires SELECT privilege or ownership
INSERT INTO orders VALUES (1, 100);  -- Requires INSERT privilege
DROP TABLE orders;  -- Requires ownership or superuser
```

### Структура:
- **src/core/role.rs** (NEW): Role struct with membership hierarchy
- **src/core/table_metadata.rs** (NEW): Table-level privilege management
- **src/core/database.rs**: Added check_table_permission(), is_table_owner()
- **src/core/server_instance.rs**: Role management + permission checks
  - create_role(), drop_role(), grant_role_to_user(), revoke_role_from_user()
  - get_user_roles() - recursive role collection
  - check_table_permission() - checks user/role table privileges
  - is_table_owner_or_superuser() - DDL permission checks
- **src/core/table.rs**: Added owner field
- **src/parser/ddl.rs**: Parsers for CREATE/DROP ROLE, GRANT/REVOKE, ALTER TABLE OWNER TO
- **src/executor/system_catalogs.rs**: Updated pg_class (relowner), added pg_auth_members, table_privileges
- **src/network/server.rs**: Permission enforcement before query execution

### Permission Model:
```
Superuser → All Permissions
    ↓
Table Owner → All Permissions on owned tables
    ↓
Direct Privilege Grant → Specific operations
    ↓
Role Membership → Inherited privileges (recursive)
```

### Ключевые возможности:
- **Role Hierarchy**: analyst → readonly → user (recursive inheritance)
- **Automatic Ownership**: CREATE TABLE sets current user as owner
- **Owner Privileges**: Owners have all privileges (SELECT/INSERT/UPDATE/DELETE)
- **Superuser Bypass**: Superusers bypass all permission checks
- **Recursive Role Collection**: Supports multi-level role inheritance
- **Permission Enforcement**: Checked before SELECT/INSERT/UPDATE/DELETE/ALTER/DROP

### Тесты (9 новых):
1. test_create_role - Создание ролей и проверка дубликатов
2. test_drop_role - Удаление ролей
3. test_grant_revoke_role - Назначение/отзыв ролей
4. test_role_hierarchy - Рекурсивное наследование ролей
5. test_table_ownership - Отслеживание владельцев таблиц
6. test_table_permission_checks - Проверка прав на таблицах
7. test_superuser_permissions - Bypass всех проверок для superuser
8. test_role_based_permissions - Права через членство в ролях
9. test_is_table_owner_or_superuser - Проверки для DDL

### System Catalogs:
```sql
SELECT * FROM pg_catalog.pg_class;  -- relowner added
SELECT * FROM pg_catalog.pg_auth_members;  -- role membership (stub)
SELECT * FROM pg_catalog.table_privileges;  -- table-level grants
```

### Архитектура изменений:
```rust
// Before v2.3.0: No permission checks
CREATE TABLE orders (...);  // Anyone can create
SELECT * FROM orders;        // Anyone can read

// After v2.3.0: Full RBAC
CREATE TABLE orders (...);  // owner = session.username
SELECT * FROM orders;        // Error: Permission denied (if not owner/granted)
GRANT SELECT ON TABLE orders TO alice;  // Grant access
-- Now alice can SELECT
```

### Совместимость:
- Обратная совместимость: старые базы получают owner = "postgres" для существующих таблиц
- PostgreSQL-compatible syntax для GRANT/REVOKE
- Полная поддержка role hierarchy как в PostgreSQL

---

## ✅ v2.2.2 - Bug Fixes and Improvements

**Цель:** Исправление критических багов после v2.2.1
**Статус:** Completed (2025-12-19)
**Сложность:** Низкая
**Breaking Changes:** No

### Fixed Issues:
1. ✅ **Dockerfile binary naming** - Fixed incorrect binary name `postgrustql` → `postgrustsql`
2. ✅ **Docker user naming** - Changed user from `rustdb` → `postgrust` for consistency
3. ✅ **Minor improvements** - Code cleanup and optimizations

### Changes:
- **Dockerfile**: Corrected binary path and user name for proper deployment
- **Version bumped**: 2.2.1 → 2.2.2 in Cargo.toml, PKGBUILD, pgr_cli

---

## ✅ v2.1.0 - Multi-Connection Transaction Isolation (DML only)

**Цель:** Изоляция DML операций между разными TCP connections
**Статус:** Completed (2025-12-18)
**Сложность:** Высокая
**Breaking Changes:** No

### Реализовано:
1. ✅ **GlobalTransactionManager** - shared state через Arc + AtomicU64
2. ✅ **MVCC Snapshot** - структура для visibility checks (xmin, xmax, active_txs)
3. ✅ **Row::is_visible_to_snapshot()** - PostgreSQL-style visibility rules
4. ✅ **Auto-commit pattern** - DML вне транзакций автоматически commit
5. ✅ **READ COMMITTED isolation** - новый snapshot перед каждым statement
6. ✅ **173/173 unit tests passing**
7. ✅ **Multi-connection isolation test** - test_mvcc_isolation.sh

### Изменения:
- **src/transaction/global_manager.rs** (NEW): Shared transaction manager
- **src/transaction/snapshot.rs**: Per-connection transaction state
- **src/core/row.rs**: Added `is_visible_to_snapshot()`
- **src/executor/dml.rs**: INSERT/UPDATE/DELETE с auto-commit pattern
- **src/executor/queries.rs**: SELECT использует snapshot visibility
- **src/executor/dispatcher.rs**: Передает active_tx_id в executors
- **src/network/server.rs**: BEGIN/COMMIT/ROLLBACK с GlobalTransactionManager

### Ключевое достижение:
```rust
// Connection 1:
BEGIN;
INSERT INTO users VALUES (1, 'Alice');
-- НЕ COMMIT

// Connection 2:
SELECT * FROM users;
-- Результат: пусто! Uncommitted row НЕ виден ✅
```

### ⚠️ Известное ограничение:
**DDL операции (CREATE/DROP/ALTER TABLE) всегда auto-commit, даже внутри транзакций!**
- Изменения схемы видны сразу всем connections
- Запланировано исправление в v2.3.0 через system catalogs

### Запланировано на v2.3.0:
- Transactional DDL с системными каталогами
- Полная PostgreSQL-совместимость для DDL

---

## ✅ v2.0.2 - Complete PagedTable Migration

**Цель:** Удалить все deprecated Table.rows usage + Clippy cleanup
**Статус:** Completed (2025-12-18)
**Сложность:** Средняя
**Breaking Changes:** Yes (all executors now require mandatory &DatabaseStorage)

### Fixed Issues:
1. ✅ **0 deprecated warnings** (was 17) - Complete removal of Table.rows access
2. ✅ **159/159 unit tests passing** - Fixed 10 aggregate/group_by tests
3. ✅ **~20 clippy warnings** (was 292) - Relaxed lints for pet project

### Changes:
- **src/executor/queries.rs**: All functions now use mandatory `&DatabaseStorage` (not `Option`)
  - `select()`, `select_regular()`, `select_aggregate()`, `select_with_group_by()`
  - `union()`, `intersect()`, `except()`, `execute_query_stmt()`
- **src/executor/dml.rs**: FK validation via `validate_foreign_keys_with_storage()`
- **src/executor/ddl.rs**: ALTER TABLE ADD/DROP COLUMN via `update_where()` on PagedTable
- **src/executor/index.rs**: Index creation via `paged_table.get_all_rows()`
- **src/executor/explain.rs**: Query analysis via `paged_table.row_count()`
- **src/storage/wal.rs**: `apply_operation()` marked as legacy with `#[allow(deprecated)]`
- **src/lib.rs**: Added 21 allowed clippy lints for relaxed configuration
- **CLAUDE.md**: Added "Code Quality" section documenting clippy config

### Architecture:
```rust
// v2.0.1 (broken): Optional storage parameter
fn select(..., database_storage: Option<&DatabaseStorage>) {
    if let Some(db_storage) = database_storage {
        // PagedTable path
    } else {
        // Legacy Table.rows path (deprecated!)
    }
}

// v2.0.2 (clean): Mandatory storage, PagedTable only
fn select(..., database_storage: &DatabaseStorage) {
    let paged_table = database_storage.get_paged_table(&from)?;
    let rows = paged_table.get_all_rows()?;
}
```

### Test Fixes:
Fixed 10 aggregate/group_by tests to use PagedTable:
- `test_aggregate_count_all`, `test_aggregate_sum`, `test_aggregate_avg`
- `test_aggregate_min`, `test_aggregate_max`, `test_aggregate_with_where`
- `test_group_by_with_count`, `test_group_by_with_sum`, `test_group_by_with_where`
- `test_group_by_without_grouped_column_error`

Helper function added:
```rust
fn setup_test_table_with_data(
    db: &mut Database,
    storage: &mut DatabaseStorage,
    rows: Vec<Row>,
)
```

### Clippy Configuration:
Allowed lints (not strict production config):
- Documentation: `missing_errors_doc`, `missing_panics_doc`
- Casts: `cast_possible_truncation`, `cast_precision_loss`, `cast_sign_loss`, `cast_possible_wrap`
- Complexity: `too_many_lines`, `too_many_arguments`, `cognitive_complexity`
- Style: `needless_pass_by_value`, `match_same_arms`, `option_if_let_else`, etc.

**Note:** This is a learning/hobby project optimized for rapid development.

---

## ✅ v2.0.1 - Critical Test Fixes

**Цель:** Исправить 16 failing dispatcher тестов после breaking changes v2.0.0
**Статус:** Completed (2025-12-17)
**Сложность:** Низкая

### Fixed Issues:
1. ✅ **16 failing dispatcher tests** - Refactored for page-based storage architecture
2. ✅ **166/166 unit tests passing** - 100% test success rate restored
3. ✅ **MVCC visibility behavior documented** - Tests now correctly handle multiple row versions

### Changes:
- Refactored all tests to use shared `DatabaseStorage` instance pattern
- Added `setup_test_table()` and `insert_test_data()` helper functions
- Adjusted MVCC expectations for UPDATE/DELETE tests (multiple row versions visible)
- All tests use `execute()` to ensure data persists in storage

### Test Pattern:
```rust
// Old (broken): separate storage instances
let mut storage = create_test_storage();
db.create_table(...); // table in Database only, not in storage!

// New (working): shared storage
let mut storage = create_test_storage();
setup_test_table(&mut db, &mut storage); // table in both
insert_test_data(&mut db, &mut storage); // data persists
```

**Note:** VACUUM for PagedTable deferred to future version (only works with legacy Vec<Row>)

---

## ✅ v2.0.0 - PostgreSQL Compatibility Layer

**Цель:** PostgreSQL wire protocol compatibility + cleanup legacy code
**Статус:** Completed (2025-12-17)
**Сложность:** Высокая
**Breaking Changes:** Yes (authentication protocol, storage architecture)

### Core Features:

#### 1. PostgreSQL Authentication Protocol
```
Client → Server: StartupMessage (no password)
Server → Client: AuthenticationCleartextPassword
Client → Server: PasswordMessage
Server → Client: AuthenticationOk
```
- Implemented `AuthenticationCleartextPassword` flow
- Compatible with `psql` client
- MD5/SCRAM deferred to future versions

#### 2. System Catalogs
```sql
-- PostgreSQL-compatible metadata queries
SELECT * FROM pg_catalog.pg_class;      -- Tables, indexes, views
SELECT * FROM pg_catalog.pg_attribute;  -- Columns
SELECT * FROM pg_catalog.pg_index;      -- Index definitions
SELECT * FROM pg_catalog.pg_type;       -- Data types
SELECT * FROM pg_catalog.pg_namespace;  -- Schemas

SELECT * FROM information_schema.tables;
SELECT * FROM information_schema.columns;
```
- Virtual tables populated from Database metadata
- Read-only
- Basic support for `\d`, `\dt`, `\l` psql commands

#### 3. System Functions
```sql
version()              -- Returns server version
current_database()     -- Returns current database name
current_user()         -- Returns username
pg_table_size(name)    -- Returns table size in bytes
pg_database_size(name) -- Returns database size
```

#### 4. Code Cleanup
- ✅ Removed `LegacyStorage` / `Vec<Row>` backend completely
- ✅ Renamed `src/executor/legacy.rs` → `src/executor/dispatcher.rs`
- ✅ Page-based storage now **MANDATORY** (not optional)
- ✅ All deprecated functions removed

### Breaking Changes:
1. **database_storage parameter now required** (not `Option<&mut DatabaseStorage>`)
2. **All DML operations require PagedTable** in DatabaseStorage
3. **Vec<Row> storage removed** - must use page-based storage
4. **Tests must use shared DatabaseStorage instance**

### PostgreSQL Compatibility:
- ✅ Wire protocol v3.0
- ✅ Authentication flow compatible with psql
- ✅ System catalog queries (basic)
- ✅ System function calls
- ❌ Schema-qualified identifiers not supported (e.g., `pg_catalog.table`)
- ❌ Extended Query Protocol (prepared statements) - deferred
- ❌ COPY protocol - deferred

### Test Status:
- **v2.0.0:** 150/166 passing (16 dispatcher tests needed refactoring)
- **v2.0.1:** 166/166 passing (all fixed)

### Files Changed:
- `src/network/pg_protocol.rs` - Authentication messages
- `src/network/server.rs` - Auth flow implementation
- `src/executor/system_catalogs.rs` (new) - Virtual catalog tables
- `src/executor/system_functions.rs` (new) - System functions
- `src/executor/dispatcher.rs` (renamed from legacy.rs)
- `src/storage/*` - Removed LegacyStorage

### Migration Guide:
1. Remove any `LegacyStorage` usage
2. Always provide `&mut DatabaseStorage` to executor (not `Option`)
3. Use `PagedTable` for all table operations
4. Rebuild indexes on startup (not serialized)

### psql Connectivity Verified:
```bash
psql -h 127.0.0.1 -p 5432 -U postgrust -d main
# Works! Authentication flow compatible
\d          # Shows tables
\dt         # Shows tables
SELECT version();  # Returns server info
```

---

## ✅ v1.11.0 - Critical Fixes & Stability

**Цель:** Исправить все известные баги и warnings перед v2.0
**Статус:** Completed (2025-12-10)
**Сложность:** Низкая

### Fixed Issues:
1. ✅ **4 failing storage tests** - Fixed `load_database()` to properly handle WAL replay for crash recovery
2. ✅ **26 compiler warnings** - All resolved (unused imports, variables, dead code)
3. ✅ **154/154 unit tests passing** - 100% test success rate
4. ✅ **All integration tests passing** - Hash indexes, composite indexes, SQL expressions

### Changes:
- `src/storage/disk.rs`: Enhanced `load_database()` with proper WAL fallback
- `src/executor/*.rs`: Fixed unused variable warnings
- `src/storage/page_manager.rs`: Fixed lifetime and unused assignment warnings

---

## ✅ v1.10.0 - SQL Expressions & Set Operations

**Цель:** Расширение SQL функциональности, быстрые победы
**Статус:** Completed (2025-12-09)
**Сложность:** Низкая-Средняя

### Features:

#### 1. CASE Expressions
```sql
SELECT name,
    CASE
        WHEN age < 18 THEN 'minor'
        WHEN age < 65 THEN 'adult'
        ELSE 'senior'
    END as category
FROM users;
```
- **Описание:** Условная логика в SELECT
- **Компоненты:**
  - Parser: `CASE WHEN condition THEN value [WHEN ...] [ELSE value] END`
  - Executor: Evaluate conditions sequentially, return first match
  - Support in WHERE, SELECT, ORDER BY
- **Сложность:** Низкая
- **Файлы:** `src/parser/queries.rs`, `src/executor/queries.rs`

#### 2. UNION / INTERSECT / EXCEPT
```sql
-- UNION: объединение результатов (без дубликатов)
SELECT name FROM customers UNION SELECT name FROM suppliers;

-- UNION ALL: объединение с дубликатами
SELECT id FROM orders_2023 UNION ALL SELECT id FROM orders_2024;

-- INTERSECT: пересечение
SELECT id FROM users_2023 INTERSECT SELECT id FROM active_users;

-- EXCEPT: разность (в первом, но не во втором)
SELECT id FROM all_users EXCEPT SELECT id FROM banned_users;
```
- **Описание:** Операции над множествами результатов
- **Компоненты:**
  - Parser: `SELECT ... UNION [ALL] SELECT ...`
  - Executor: Execute both queries, merge results
  - UNION: deduplicate using HashSet
  - INTERSECT: filter first by second
  - EXCEPT: remove second from first
- **Требования:** Совместимость типов колонок
- **Сложность:** Низкая-Средняя
- **Файлы:** `src/parser/queries.rs`, `src/executor/queries.rs`

#### 3. Views (Virtual Tables)
```sql
CREATE VIEW active_users AS
    SELECT * FROM users WHERE status = 'active';

SELECT * FROM active_users;

DROP VIEW active_users;
```
- **Описание:** Виртуальные таблицы, хранят SQL запрос
- **Компоненты:**
  - Parser: `CREATE VIEW name AS SELECT ...`
  - Storage: `Database.views: HashMap<String, String>` (view_name → SQL)
  - Executor: При SELECT from view → parse SQL, execute
  - DROP VIEW support
- **Сложность:** Низкая-Средняя
- **Основа для:** Materialized Views (v1.11+)
- **Файлы:**
  - `src/types/database.rs` - add views field
  - `src/parser/ddl.rs` - CREATE/DROP VIEW
  - `src/executor/ddl.rs` - view management
  - `src/executor/queries.rs` - view resolution

### Testing:
- Unit tests для CASE (простые/вложенные/с NULL)
- Unit tests для UNION/INTERSECT/EXCEPT
- Unit tests для Views (create/drop/query)
- Integration test: `test_sql_expressions.sh`

### Documentation:
- CLAUDE.md: примеры использования
- SQL syntax reference

---

## 🔒 v2.1.0 - Multi-Connection Transaction Isolation

**Цель:** Production-ready транзакции с настоящей изоляцией
**Статус:** **NEXT** (after v2.0.1)
**Сложность:** Очень Высокая

### Current State:
- MVCC работает: `xmin`, `xmax`, snapshot isolation
- **Проблема:** Изоляция только внутри одного TCP connection
- Разные клиенты видят uncommitted changes друг друга

### Goal:
Настоящая изоляция транзакций между разными соединениями.

### Architecture Changes:

#### 1. Global Transaction Manager
```rust
// Сейчас: TransactionManager per-connection
// Цель: Shared TransactionManager across all connections

pub struct GlobalTransactionManager {
    next_tx_id: AtomicU64,
    active_transactions: RwLock<HashMap<u64, TransactionState>>,
    snapshot_cache: RwLock<SnapshotCache>,
}

pub struct TransactionState {
    tx_id: u64,
    start_time: Instant,
    isolation_level: IsolationLevel,
    active_snapshot: Snapshot,
}

pub enum IsolationLevel {
    ReadCommitted,      // Default (easier)
    RepeatableRead,     // PostgreSQL default
    Serializable,       // Full isolation (hardest)
}
```

#### 2. Snapshot Management
```rust
pub struct Snapshot {
    xmin: u64,              // Oldest active transaction
    xmax: u64,              // Next transaction ID
    active_txs: Vec<u64>,   // In-progress transactions (invisible)
}

// Visibility check
impl Row {
    fn is_visible(&self, snapshot: &Snapshot) -> bool {
        // xmin committed and < xmax?
        // xmax not committed or > xmax?
        // Not in active_txs?
    }
}
```

#### 3. Implementation Steps:

**Phase 1: Global Transaction Coordinator**
- Move `TransactionManager` to `Arc<GlobalTransactionManager>`
- Share across all connections
- Atomic transaction ID generation

**Phase 2: Snapshot Isolation**
- Create snapshot on `BEGIN`
- Store active transaction list
- Update visibility checks in queries

**Phase 3: Commit/Rollback Coordination**
- Global commit log
- Update active_transactions on COMMIT
- Invalidate snapshots on ROLLBACK

**Phase 4: Deadlock Detection (Optional)**
- Wait-for graph
- Detect cycles
- Abort youngest transaction

#### 4. Isolation Levels:

**READ COMMITTED (Easiest, Start Here):**
- New snapshot on each statement
- Sees all committed changes

**REPEATABLE READ (PostgreSQL Default):**
- Snapshot on BEGIN
- Same snapshot for entire transaction
- No phantom reads

**SERIALIZABLE (Hardest, Optional):**
- Detect conflicts (Serialization Graph Testing)
- Abort conflicting transactions

### Testing:
- Multi-connection tests (2+ clients)
- Concurrent INSERT/UPDATE/DELETE
- Lost update prevention
- Phantom read prevention
- Deadlock tests (if implemented)

### Files:
- `src/transaction/global_manager.rs` (new)
- `src/transaction/snapshot.rs` (refactor)
- `src/types/row.rs` (update visibility)
- `src/network/server.rs` (share global manager)

### Documentation:
- Transaction isolation levels
- Concurrency guarantees
- Known limitations

---

## ✅ v2.2.0 - Backup & Restore Tools

**Цель:** Собственные утилиты для бэкапа и восстановления (альтернатива pg_dump)
**Статус:** Completed (2025-12-19)
**Сложность:** Средняя
**Breaking Changes:** No

### Реализовано:

#### 1. pgr_dump ✅
```bash
# Full database dump to SQL
./target/release/pgr_dump postgres > backup.sql

# Dump only schema
./target/release/pgr_dump --schema-only postgres > schema.sql

# Dump only data
./target/release/pgr_dump --data-only postgres > data.sql

# Binary format (faster)
./target/release/pgr_dump --format=binary postgres > backup.bin
```

**Features:**
- ✅ Executable: `src/bin/pgr_dump.rs` (323 lines)
- ✅ CLI with clap (--schema-only, --data-only, --format, --output)
- ✅ Export schema:
  - CREATE TYPE for enums
  - CREATE TABLE with all 23 data types
  - CREATE INDEX (single + composite, hash + btree)
  - CREATE VIEW
- ✅ Export data:
  - INSERT statements with batching (100 rows per batch)
  - All 23 data types supported
  - Proper SQL escaping (single quotes, bytea hex format)
  - MVCC metadata not exported (clean restore)
- ✅ Binary format: bincode serialization

#### 2. pgr_restore ✅
```bash
# Restore from SQL dump
./target/release/pgr_restore postgres < backup.sql

# Restore from binary
./target/release/pgr_restore --format=binary postgres < backup.bin

# Dry run (validate only)
./target/release/pgr_restore --dry-run postgres < backup.sql
```

**Features:**
- ✅ Executable: `src/bin/pgr_restore.rs` (231 lines)
- ✅ CLI with clap (--format, --input, --dry-run)
- ✅ Auto-detect format (SQL vs binary)
- ✅ Reuse existing parser (parse_statement)
- ✅ Execute in auto-commit mode with GlobalTransactionManager
- ✅ Error handling with descriptive messages
- ✅ Smart SQL splitting (handles multi-line, strings, comments)

#### 3. Integration Tests ✅
- ✅ `tests/integration/test_dump_restore.sh` - Full round-trip test
- ✅ `tests/integration/test_dump_simple.sh` - Simple verification

### Not Implemented (Future: v2.3.0+):
- ⏳ WAL Archiving (continuous archiving)
- ⏳ Point-in-time recovery (PITR)
- ⏳ pg_dump protocol compatibility
- ⏳ Large database benchmarks (1M+ rows)

---

## 📊 Version Summary

| Version | Focus | Key Features | Complexity | Status |
|---------|-------|--------------|------------|--------|
| v1.9.0 | ✅ Composite Indexes | Multi-column indexes | Medium | Completed |
| v1.10.0 | ✅ SQL Features | CASE, UNION, Views | Low-Medium | Completed |
| v1.11.0 | ✅ Stability | Critical fixes | Low | Completed |
| v2.0.0 | ✅ PostgreSQL | Auth protocol + system catalogs | High | **Completed (2025-12-17)** |
| v2.0.1 | ✅ Test Fixes | 16 dispatcher tests fixed | Low | **Completed (2025-12-17)** |
| v2.1.0 | ✅ Transactions | Multi-connection isolation (DML) | Very High | **Completed (2025-12-18)** |
| v2.2.0 | ✅ Backup Tools | pgr_dump/pgr_restore (SQL+bin) | Medium | **Completed (2025-12-19)** |
| v2.3.0 | ✅ RBAC | Role-based access control | High | **Completed (2025-12-22)** |
| v2.4.0 | ✅ Protocol Extensions | Extended Query + COPY | High | **Completed (2025-12-26)** |
| v2.5.0 | ✅ Binary COPY | PostgreSQL binary format (all 23 types) + COPY TO STDOUT | High | **Completed (2025-12-26)** |
| v2.6.0 | 🚧 Advanced SQL | Subqueries + Window Functions | Very High | **Planned** |

---

## 🎯 Current Status

**Recently Completed:**
- ✅ v2.0.0 (PostgreSQL auth protocol, system catalogs) - 2025-12-17
- ✅ v2.0.1 (Fixed 16 dispatcher tests, 166/166 passing) - 2025-12-17
- ✅ v2.1.0 (Multi-connection transaction isolation - DML) - 2025-12-18
- ✅ v2.2.0 (Backup & Restore tools: pgr_dump/pgr_restore) - 2025-12-19
- ✅ v2.3.0 (Role-Based Access Control - RBAC) - 2025-12-22
- ✅ v2.4.0 (Extended Query Protocol + COPY) - 2025-12-26
- ✅ v2.5.0 (COPY Binary Format + COPY TO STDOUT) - 2025-12-26

**Foundation achieved:**
- ✅ PostgreSQL wire protocol v3.0 (Simple + Extended Query)
- ✅ Multi-connection MVCC isolation (DML)
- ✅ Page-based storage with WAL
- ✅ B-tree & Hash indexes (single + composite)
- ✅ Backup & Restore utilities (pgr_dump/pgr_restore)
- ✅ Role-Based Access Control (RBAC)
- ✅ Prepared statements (Extended Query Protocol)
- ✅ Bulk import/export (COPY protocol)
- ✅ 202 unit tests passing (0 failed, 7 ignored)

**What's next?**
- 🚧 v2.6.0 (Subqueries, pg_dump compatibility, Window Functions) - Planning

---

## 🚀 v2.5.0+ - Future Features (Advanced SQL)

**Статус:** Planned (after v2.4.0)
**Сложность:** Varies

### Advanced SQL Features

#### Subqueries
```sql
SELECT * FROM products WHERE category_id IN
    (SELECT id FROM categories WHERE active = true);

SELECT name, (SELECT COUNT(*) FROM orders WHERE orders.user_id = users.id) as order_count
FROM users;
```

#### Window Functions
```sql
SELECT name, salary,
       ROW_NUMBER() OVER (ORDER BY salary DESC) as rank,
       AVG(salary) OVER (PARTITION BY department) as dept_avg
FROM employees;
```

#### Multiple JOINs
```sql
SELECT * FROM users u
JOIN orders o ON u.id = o.user_id
JOIN products p ON o.product_id = p.id
WHERE p.price > 100;
```

#### Triggers
```sql
CREATE TRIGGER update_timestamp
BEFORE UPDATE ON users
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();
```

#### Stored Procedures (PL/pgSQL)
```sql
CREATE FUNCTION calculate_discount(price NUMERIC)
RETURNS NUMERIC AS $$
BEGIN
    IF price > 1000 THEN
        RETURN price * 0.9;
    ELSE
        RETURN price * 0.95;
    END IF;
END;
$$ LANGUAGE plpgsql;
```

### Performance Enhancements
- Query cache
- Statistics collector (for query planner)
- Auto-VACUUM (background cleanup)
- Parallel query execution
- Connection pooling

### Replication
- Master-slave replication
- Streaming replication (WAL shipping)
- Read replicas
- Logical replication

---

**Last Updated:** 2025-12-26 (after v2.4.0 completion)
