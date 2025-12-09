# CLAUDE.md

Guidance for Claude Code when working with this repository.

## Quick Start

**Run:**
```bash
cargo run --release                        # Server (port 5432)
cargo run --example cli                    # CLI client
cargo test                                 # 147 tests (4 known failures in storage)
./tests/integration/test_new_types.sh      # Test all 23 data types
./tests/integration/test_hash_index.sh     # Test hash & B-tree indexes
./tests/integration/test_composite_index.sh # Test composite indexes (v1.9.0)
./tests/integration/test_extended_operators.sh  # Test extended WHERE operators
./tests/integration/test_explain.sh        # Test EXPLAIN command
./tests/integration/test_sql_expressions.sh # Test CASE & set operations (v1.10.0)
printf "\\\\dt\nquit\n" | nc 127.0.0.1 5432  # Quick netcat test
```

**Features:**
- PostgreSQL-compatible wire protocol (port 5432)
- 23 data types (~45% PostgreSQL compatibility)
- FOREIGN KEY, JOIN (INNER/LEFT/RIGHT), SERIAL/BIGSERIAL
- Transactions (snapshot isolation), MVCC (xmin/xmax)
- Binary storage + WAL (checkpoint every 100 ops)
- Page-based storage (v1.5.0, 125x write amplification improvement)
- VACUUM command for MVCC cleanup (v1.5.1)
- B-tree & Hash indexes with automatic query optimization (v1.7.0)
- Extended WHERE operators (>=, <=, BETWEEN, LIKE, IN, IS NULL) + EXPLAIN (v1.8.0)
- Composite (multi-column) indexes with AND query optimization (v1.9.0)
- **CASE expressions, UNION/INTERSECT/EXCEPT set operations (v1.10.0)** ✨

## Architecture (v1.9.0)

### Модульная структура:
```
src/
├── core/          # Database, Table, Row, Value (23 types), Column, etc.
├── parser/        # SQL parser (nom) - ddl.rs, dml.rs, queries.rs
├── executor/      # Modular executor (v1.5.0) ✨
│   ├── storage_adapter.rs  # RowStorage trait (Vec<Row> | PagedTable)
│   ├── conditions.rs       # WHERE evaluation (=, !=, >, <, >=, <=, BETWEEN, LIKE, IN, IS NULL)
│   ├── dml.rs             # INSERT/UPDATE/DELETE (with index maintenance)
│   ├── ddl.rs             # CREATE/DROP/ALTER TABLE
│   ├── queries.rs         # SELECT (with query planner for indexes)
│   ├── vacuum.rs          # VACUUM cleanup (v1.5.1)
│   ├── index.rs           # CREATE/DROP INDEX (v1.7.0)
│   ├── explain.rs         # EXPLAIN query analyzer (v1.8.0)
│   └── legacy.rs          # Minimal dispatcher (146 lines)
├── index/         # Index implementations (v1.7.0, v1.9.0: composite support)
│   ├── btree.rs           # BTreeIndex O(log n) - single & composite
│   ├── hash.rs            # HashIndex O(1) - single & composite
│   └── mod.rs             # IndexType enum and Index wrapper
├── transaction/   # TransactionManager, Snapshot
├── storage/       # Binary save/load, WAL, Page-based (v1.5.0)
└── network/       # TCP server, PostgreSQL protocol

Total: ~2,400 lines of modular code (vs 3009 lines monolith before refactoring)
```

### Storage Architecture (v1.5.0):
```
Legacy (Vec<Row>):
  Database → Table.rows: Vec<Row>  (in-memory)
  Checkpoint: Serialize entire DB to .db file (~100MB → 10GB amplification!)

Page-based (Default):
  Database → PagedTable → PageManager → BufferPool → Page (8KB)
  Checkpoint: Write only dirty pages (~80x amplification, 125x improvement!)
```

**Status**: Page-based storage fully integrated (v1.5.0), 46 unit tests passing.

## Common Tasks

### Add new SQL command:
1. `src/parser/statement.rs` - add to Statement enum
2. `src/parser/{ddl|dml|queries}.rs` - write parser
3. `src/parser/mod.rs` - add to parse_statement()
4. `src/executor/{ddl|dml|queries}.rs` - implement
5. Wire in `src/executor/legacy.rs` execute() dispatcher

### Supported SQL:
```sql
-- DDL
CREATE TABLE t (id SERIAL, name TEXT UNIQUE, age INTEGER);
ALTER TABLE t ADD COLUMN email VARCHAR(100);
DROP TABLE t;

-- DML
INSERT INTO t (name) VALUES ('Alice');
UPDATE t SET age = 30 WHERE name = 'Alice';
DELETE FROM t WHERE age < 18;

-- Queries
SELECT * FROM t WHERE age > 18 ORDER BY name LIMIT 10 OFFSET 5;
SELECT name, COUNT(*) FROM t GROUP BY name HAVING COUNT(*) > 1;
SELECT * FROM users INNER JOIN orders ON users.id = orders.user_id;

-- Extended WHERE (v1.8.0)
SELECT * FROM users WHERE age >= 25 AND age <= 35;
SELECT * FROM products WHERE price BETWEEN 100 AND 500;
SELECT * FROM users WHERE name LIKE 'A%';            -- % = any chars, _ = single char
SELECT * FROM orders WHERE status IN ('pending', 'shipped');
SELECT * FROM users WHERE email IS NOT NULL;

-- CASE Expressions (v1.10.0)
SELECT name, age,
    CASE
        WHEN age < 18 THEN 'minor'
        WHEN age < 65 THEN 'adult'
        ELSE 'senior'
    END AS category
FROM users;

-- Set Operations (v1.10.0)
SELECT name FROM customers UNION SELECT name FROM suppliers;       -- Remove duplicates
SELECT name FROM customers UNION ALL SELECT name FROM suppliers;   -- Keep duplicates
SELECT name FROM customers INTERSECT SELECT name FROM suppliers;   -- Common rows
SELECT name FROM customers EXCEPT SELECT name FROM suppliers;      -- In left, not in right

-- Views (v1.10.0)
CREATE VIEW active_users AS SELECT * FROM users WHERE status = 'active';
SELECT * FROM active_users;                                       -- Query from view
DROP VIEW active_users;

-- Types
CREATE TYPE mood AS ENUM ('happy', 'sad');
CREATE TABLE person (id SERIAL, m mood, data JSONB, uuid UUID);

-- Indexes (v1.7.0, v1.9.0: composite support)
CREATE INDEX idx_age ON users(age);                    -- Single-column B-tree
CREATE INDEX idx_category ON products(category) USING HASH;  -- Single-column hash
CREATE UNIQUE INDEX idx_email ON users(email) USING BTREE;   -- Unique single-column
CREATE INDEX idx_city_age ON users(city, age);         -- Composite B-tree (v1.9.0)
CREATE INDEX idx_name ON people(first_name, last_name) USING HASH;  -- Composite hash
DROP INDEX idx_age;

-- Query Analysis (v1.8.0)
EXPLAIN SELECT * FROM users WHERE age = 30;
-- Shows: Index Scan using idx_age (btree), Rows: ~1, Cost: O(log n)

-- Composite index queries (v1.9.0)
SELECT * FROM users WHERE city = 'NYC' AND age = 30;   -- Uses idx_city_age
EXPLAIN SELECT * FROM users WHERE city = 'LA' AND age = 25;
-- Shows: Index Scan using idx_city_age (btree)

-- Maintenance
VACUUM;              -- Cleanup all tables
VACUUM table_name;   -- Cleanup specific table
```

## Data Types (23 total)

**Numeric**: SMALLINT, INTEGER, BIGINT, SERIAL, BIGSERIAL, REAL, NUMERIC(p,s)
**String**: TEXT, VARCHAR(n), CHAR(n)
**Date/Time**: DATE, TIMESTAMP, TIMESTAMPTZ
**Special**: BOOLEAN, UUID, JSON, JSONB, BYTEA, ENUM

**Validation**: VARCHAR length, CHAR padding, ENUM values checked on INSERT.

## Key Features

### MVCC (Multi-Version Concurrency Control)
```rust
pub struct Row {
    values: Vec<Value>,
    xmin: u64,           // Transaction that created this row
    xmax: Option<u64>,   // Transaction that deleted this row
}
```
- UPDATE creates new row version
- DELETE marks xmax
- Visibility: `row.is_visible(current_tx_id)`

### WAL (Write-Ahead Log)
- Binary format (`data/wal/*.wal`)
- Automatic logging (CREATE/INSERT/UPDATE/DELETE)
- Checkpoint every 100 operations
- Crash recovery: load .db + replay WAL

### Transactions
```sql
BEGIN;
  UPDATE accounts SET balance = balance - 100 WHERE id = 1;
  UPDATE accounts SET balance = balance + 100 WHERE id = 2;
COMMIT;  -- or ROLLBACK
```
**Limitation**: Snapshot isolation works within single connection only.

### Indexes (v1.7.0, v1.9.0: composite support)
```sql
-- Single-column B-tree index: O(log n), supports range queries
CREATE INDEX idx_age ON users(age) USING BTREE;
SELECT * FROM users WHERE age > 30;  -- Can use index for range

-- Composite B-tree index: multiple columns (v1.9.0)
CREATE INDEX idx_city_age ON users(city, age);
SELECT * FROM users WHERE city = 'NYC' AND age = 30;  -- Uses composite index

-- Hash index: O(1) average case, equality only
CREATE INDEX idx_category ON products(category) USING HASH;
SELECT * FROM products WHERE category = 'Electronics';  -- O(1) lookup

-- Composite hash index (v1.9.0)
CREATE INDEX idx_name ON people(first_name, last_name) USING HASH;
SELECT * FROM people WHERE first_name = 'John' AND last_name = 'Doe';  -- O(1)

-- Index maintenance
INSERT/UPDATE/DELETE automatically maintain all indexes (single & composite)
```
**Features:**
- Two index types: **BTREE** (default) and **HASH**
- **Single-column and multi-column (composite) indexes (v1.9.0)**
- CREATE INDEX / CREATE UNIQUE INDEX / DROP INDEX
- Automatic query planner (chooses index scan vs seq scan)
- **Composite index optimization for AND conditions (v1.9.0)**
- MVCC-aware visibility checks
- Index maintenance on INSERT/UPDATE/DELETE
- B-tree: Supports Equals/GreaterThan/LessThan (range queries)
- Hash: Supports Equals only (faster for exact matches)

### Extended WHERE Operators (v1.8.0)
```sql
-- Comparison operators
WHERE age >= 25          -- Greater than or equal
WHERE age <= 35          -- Less than or equal

-- Range queries
WHERE age BETWEEN 25 AND 35  -- Inclusive range

-- Pattern matching
WHERE name LIKE 'A%'     -- Starts with A
WHERE name LIKE '%son'   -- Ends with 'son'
WHERE name LIKE '%li%'   -- Contains 'li'
WHERE name LIKE 'A____'  -- A followed by 4 chars

-- List membership
WHERE status IN ('pending', 'shipped', 'delivered')
WHERE id IN (1, 2, 3)

-- NULL checks
WHERE email IS NULL
WHERE email IS NOT NULL
```
**Features:**
- **LIKE pattern matching**: % (any chars), _ (single char)
- **BETWEEN**: Inclusive range (low AND high)
- **IN**: Membership in value list
- **IS NULL / IS NOT NULL**: Null checks
- Works with all data types
- Efficient recursive pattern matching for LIKE

### EXPLAIN Command (v1.8.0)
```sql
EXPLAIN SELECT * FROM users WHERE age = 30;
```
**Output:**
```
QUERY PLAN
──────────────────────────────────────────────────
→ Index Scan using idx_age (btree)
  on users
  Index Cond: age = Integer(30)
  Rows: ~1
  Cost: O(log n)
──────────────────────────────────────────────────
```
**Features:**
- Shows execution plan for SELECT queries
- Identifies scan type: Sequential | Index | Unique Index
- Displays index usage (name + type: hash/btree)
- Cost estimates: O(1), O(log n), O(n)
- Row count estimates
- Helps optimize queries and identify missing indexes

### PostgreSQL Protocol
- Auto-detection (peek first 8 bytes)
- Messages: StartupMessage, Query, RowDescription, DataRow, etc.
- Test: `psql -h 127.0.0.1 -p 5432 -U rustdb -d main`

## Testing

**Unit tests**: 144 tests (4 known storage failures)
**Integration**:
```bash
./tests/integration/test_features.sh      # Full feature test
./tests/integration/test_fk_join.sh       # FK + JOIN
./tests/integration/test_new_types.sh     # All 23 types
./tests/integration/test_page_storage.sh  # Page-based (46 tests)
./tests/integration/test_vacuum.sh        # VACUUM cleanup (v1.5.1)
./tests/integration/test_index.sh         # CREATE/DROP INDEX (v1.6.0)
./tests/integration/test_index_usage.sh   # Index query optimization (v1.6.0)
./tests/integration/test_hash_index.sh    # Hash & B-tree indexes (v1.7.0)
./tests/integration/test_composite_index.sh  # Composite indexes (v1.9.0)
./tests/integration/test_extended_operators.sh  # Extended WHERE (v1.8.0)
./tests/integration/test_explain.sh       # EXPLAIN command (v1.8.0)
```

## Limitations

- Composite indexes require exact match of all columns (partial prefix matching not yet supported)
- Hash indexes only support equality (=) - use B-tree for range queries
- Single JOIN per query
- WHERE with JOIN not fully supported
- Transactions not isolated between connections
- EXPLAIN only supports SELECT (not INSERT/UPDATE/DELETE)

## Версионирование

**Current**: v1.10.0 (CASE expressions & Set operations)
**Previous**:
- v1.9.0 - Composite multi-column indexes
- v1.8.0 - Extended WHERE operators + EXPLAIN command
- v1.7.0 - Hash indexes with USING clause
- v1.6.0 - B-tree indexes with query optimization
- v1.5.1 - VACUUM command for MVCC cleanup
- v1.5.0 - Page-based storage (125x write amplification improvement)
- v1.4.1 - ALTER TABLE
- v1.4.0 - OFFSET, DISTINCT, UNIQUE
- v1.3.2 - Modular architecture
- v1.3.1 - 18 new data types

**Git tags**: `git tag -a v1.X.Y -m "message"`

## Зависимости

```toml
tokio = "1.41"           # async runtime
nom = "7.1"              # SQL parsing
serde/bincode = "1.0"    # serialization
comfy-table = "7.1"      # table formatting
rustyline = "14.0"       # CLI history
chrono = "0.4"           # Date/Time
uuid = "1.6"             # UUID
rust_decimal = "1.33"    # NUMERIC
```

## Полезные команды

```bash
# Development
cargo build --release && cargo run --release
cargo test --lib
cargo test --lib storage  # Test specific module

# Integration
./tests/integration/test_features.sh
./tests/integration/test_new_types.sh

# Debug
RUST_LOG=debug cargo run --release
git log --oneline --graph
git diff HEAD~1

# Benchmarking
hyperfine './target/release/postgrustql'
```

## Известные проблемы

1. **Storage disk tests fail** (4 tests) - pre-existing, low priority
2. **Write amplification** - current Vec<Row> backend rewrites entire DB (~100M x)
   - Solution ready but not integrated: page-based storage (~80x)
3. **Transaction isolation** - only works within single connection
4. **Parser limitations** - single quotes only, no escape sequences
5. **CLI pipe issues** - fixed in v1.3.1 (rustyline)

---

**For detailed history**: See git log, FUTURE_UPDATES.md, and test scripts.
