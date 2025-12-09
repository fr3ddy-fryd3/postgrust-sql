# RustDB Roadmap

Долгосрочный план развития проекта после v1.9.0 (Composite Indexes).

---

## 🎯 v1.10.0 - SQL Expressions & Set Operations

**Цель:** Расширение SQL функциональности, быстрые победы
**Статус:** Planned
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

## 🔒 v1.11.0 - Multi-Connection Transaction Isolation

**Цель:** Production-ready транзакции с настоящей изоляцией
**Статус:** Planned
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
## 🔮 v2.0.0 - PostgreSQL Compatibility

**Цель:** Совместимость с psql и pg_dump + cleanup legacy
**Статус:** Planned
**Сложность:** Очень Высокая
**Breaking Changes:** Yes (authentication protocol)

### 1. Cleanup Legacy Code

**Remove:**
- ❌ `src/executor/legacy.rs` - старый монолитный executor
- ❌ `src/parser_old.rs` - старый парсер (если есть)
- ❌ `LegacyStorage` / `Vec<Row>` backend
- ❌ Все deprecated функции и warnings

**Refactor:**
- Полный переход на page-based storage везде
- Единый модульный executor
- Чистка неиспользуемых imports

### 2. PostgreSQL Wire Protocol - Authentication

**Current:** Password in StartupMessage (non-standard)
**Target:** Standard PostgreSQL authentication

```
Client → Server: StartupMessage (no password)
Server → Client: AuthenticationCleartextPassword / AuthenticationMD5Password
Client → Server: PasswordMessage
Server → Client: AuthenticationOk
```

**Implementation:**
- `AuthenticationCleartextPassword` (simple, start here)
- `AuthenticationMD5Password` (md5(md5(password + username) + salt))
- Optional: `AuthenticationSASL` (SCRAM-SHA-256)
- Files: `src/network/pg_protocol.rs`, `src/network/server.rs`

### 3. System Catalogs

Required for `pg_dump`, `\d` commands:

```sql
-- Metadata tables
pg_catalog.pg_class       -- Tables, indexes, views
pg_catalog.pg_attribute   -- Columns
pg_catalog.pg_index       -- Index definitions
pg_catalog.pg_type        -- Data types
pg_catalog.pg_namespace   -- Schemas

information_schema.tables
information_schema.columns
information_schema.views
```

**Implementation:**
- Virtual tables (like Views from v1.10)
- Populate from Database metadata
- Read-only
- Files: `src/executor/system_catalogs.rs` (new)

### 4. System Functions

```sql
pg_get_indexdef(index_oid)     -- Get index definition
pg_table_size(table_name)      -- Table size in bytes
pg_database_size(db_name)      -- Database size
version()                       -- Server version
current_database()             -- Current DB name
```

### 5. Extended Query Protocol (Optional)

**Current:** Simple Query Protocol only
**Target:** Prepared statements

```
Parse → Bind → Describe → Execute → Sync
```

Benefits:
- Prepared statements
- Parameter binding ($1, $2)
- Better performance

### 6. COPY Protocol (Optional)

```sql
COPY users FROM STDIN;
COPY users TO STDOUT;
```

Fast bulk import/export for `pg_dump`.

### 7. Breaking Changes Documentation

**Create:** `MIGRATION_v1_to_v2.md`
- Authentication protocol changes
- Connection string format changes
- Migration steps for existing databases
- Removed features

### 8. Production Checklist

**Before v2.0 release:**
- ✅ All known bugs fixed
- ✅ `psql` full compatibility
- ✅ `pg_dump` / `pg_restore` work correctly
- ✅ Performance benchmarks vs v1.9
- ✅ Full test coverage (150+ tests)
- ✅ Documentation complete

### Testing:
- `psql -h 127.0.0.1 -p 5432 -U user -d main` works
- `\d`, `\dt`, `\l` meta-commands work
- `pg_dump` → `pg_restore` round-trip
- Multi-client tests

### Documentation:
- Updated CLAUDE.md for v2.0
- PostgreSQL compatibility level
- Supported features vs real PostgreSQL
- Known limitations

---

## 🚀 v2.1.0 - Backup & Restore Tools

**Цель:** Собственные утилиты для бэкапа и восстановления (альтернатива pg_dump)
**Статус:** Planned
**Сложность:** Средняя

### 1. rustdb-dump

```bash
# Full database dump to SQL
rustdb-dump main > backup.sql

# Dump only schema
rustdb-dump --schema-only main > schema.sql

# Dump only data
rustdb-dump --data-only main > data.sql

# Binary format (faster)
rustdb-dump --format=binary main > backup.rustdb
```

**Implementation:**
- Executable: `src/bin/rustdb-dump.rs`
- Export schema:
  - `CREATE TABLE` statements
  - `CREATE INDEX` statements (single + composite)
  - `CREATE VIEW` statements (v1.10+)
  - `CREATE TYPE` for enums
- Export data:
  - `INSERT` statements (batched for performance)
  - Handle all 23 data types
  - Proper escaping for TEXT/VARCHAR
  - MVCC metadata (xmin/xmax) optional flag
- Optional: Binary format for speed

### 2. rustdb-restore

```bash
# Restore from SQL dump
rustdb-restore main < backup.sql

# Restore from binary
rustdb-restore --format=binary main < backup.rustdb

# Dry run (validate only)
rustdb-restore --dry-run main < backup.sql
```

**Implementation:**
- Executable: `src/bin/rustdb-restore.rs`
- Parse SQL dump (reuse existing parser!)
- Execute statements in transaction
- Rollback on error
- Progress reporting
- Conflict resolution options (skip/overwrite/fail)

### 3. WAL Archiving

```bash
# Continuous WAL archiving
rustdb-archive --continuous --wal-dir data/wal --archive-dir /backup/wal/

# Create base backup
rustdb-archive --base-backup --output /backup/base/

# Point-in-time recovery
rustdb-restore --pitr --target-time "2025-12-09 10:30:00" main
```

**Implementation:**
- Watch `data/wal/` directory
- Copy completed WAL files to archive
- Base backup = full dump + WAL start position
- PITR = restore base + replay WAL до target time

### 4. Testing:
- Dump/restore round-trip (data integrity)
- Large database tests (1M+ rows)
- Binary format performance vs SQL
- WAL archiving and PITR scenarios

### 5. Documentation:
- Backup/Restore best practices guide
- Production deployment guide
- Disaster recovery procedures
- Performance tuning for large databases

---
### Materialized Views
```sql
CREATE MATERIALIZED VIEW daily_stats AS
    SELECT date, COUNT(*) as orders, SUM(total) as revenue
    FROM orders GROUP BY date;

REFRESH MATERIALIZED VIEW daily_stats;
```

### Subqueries
```sql
SELECT * FROM products WHERE category_id IN
    (SELECT id FROM categories WHERE active = true);
```

### Window Functions
```sql
SELECT name, salary,
       ROW_NUMBER() OVER (ORDER BY salary DESC) as rank
FROM employees;
```

### Multiple JOINs
```sql
SELECT * FROM users u
JOIN orders o ON u.id = o.user_id
JOIN products p ON o.product_id = p.id;
```

### Triggers
```sql
CREATE TRIGGER update_timestamp
BEFORE UPDATE ON users
FOR EACH ROW
EXECUTE FUNCTION update_modified_column();
```

### Stored Procedures
```sql
CREATE FUNCTION calculate_discount(price NUMERIC)
RETURNS NUMERIC AS $$
BEGIN
    RETURN price * 0.9;
END;
$$ LANGUAGE plpgsql;
```

### Replication
- Master-slave replication
- Streaming replication
- Read replicas

### Performance
- Query cache
- Statistics collector
- Auto-vacuum
- Parallel query execution

---

## 📊 Version Summary

| Version | Focus | Key Features | Complexity | ETA |
|---------|-------|--------------|------------|-----|
| v1.9.0 | ✅ Done | Composite Indexes | Medium | Completed |
| v1.10.0 | SQL | CASE, UNION, Views | Low-Medium | Next |
| v1.11.0 | Transactions | Multi-connection isolation | Very High | After 1.10 |
| v2.0.0 | Compatibility | Cleanup + PostgreSQL protocol | High | Major |
| v2.1.0 | Production | Backup/Restore tools | Medium | After 2.0 |
| v2.2+ | Advanced | Subqueries, Windows, etc | Varies | TBD |

---

## 🎯 Current Priority: v1.10.0

**Next Steps:**
1. Start with CASE expressions (easiest)
2. Implement UNION/INTERSECT/EXCEPT
3. Add Views support
4. Write tests
5. Update documentation
6. Tag v1.10.0

**See TODO list in current session for detailed tasks.**

---

**Last Updated:** 2025-12-09 (after v1.9.0 completion)
