# CLAUDE.md

Guidance for Claude Code when working with this repository.

## Quick Start

**Run:**
```bash
cargo run --release                        # Server (port 5432)
cargo run --example cli                    # CLI client
cargo test                                 # 117 tests (4 known failures in storage)
./tests/integration/test_new_types.sh      # Test all 23 data types
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

## Architecture (v1.5.1)

### Модульная структура:
```
src/
├── core/          # Database, Table, Row, Value (23 types), Column, etc.
├── parser/        # SQL parser (nom) - ddl.rs, dml.rs, queries.rs
├── executor/      # Modular executor (v1.5.0) ✨
│   ├── storage_adapter.rs  # RowStorage trait (Vec<Row> | PagedTable)
│   ├── conditions.rs       # WHERE evaluation
│   ├── dml.rs             # INSERT/UPDATE/DELETE
│   ├── ddl.rs             # CREATE/DROP/ALTER TABLE
│   ├── queries.rs         # SELECT (regular/aggregate/join/group by)
│   ├── vacuum.rs          # VACUUM cleanup (v1.5.1)
│   └── legacy.rs          # Minimal dispatcher (146 lines)
├── transaction/   # TransactionManager, Snapshot
├── storage/       # Binary save/load, WAL, Page-based (v1.5.0)
└── network/       # TCP server, PostgreSQL protocol

Total: 1,888 lines of modular code (vs 3009 lines monolith before refactoring)
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

-- Types
CREATE TYPE mood AS ENUM ('happy', 'sad');
CREATE TABLE person (id SERIAL, m mood, data JSONB, uuid UUID);

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

### PostgreSQL Protocol
- Auto-detection (peek first 8 bytes)
- Messages: StartupMessage, Query, RowDescription, DataRow, etc.
- Test: `psql -h 127.0.0.1 -p 5432 -U rustdb -d main`

## Testing

**Unit tests**: 117 tests (4 known storage failures)
**Integration**:
```bash
./tests/integration/test_features.sh      # Full feature test
./tests/integration/test_fk_join.sh       # FK + JOIN
./tests/integration/test_new_types.sh     # All 23 types
./tests/integration/test_page_storage.sh  # Page-based (46 tests)
./tests/integration/test_vacuum.sh        # VACUUM cleanup (v1.5.1)
```

## Limitations

- No indexes (sequential scan only)
- Single JOIN per query
- WHERE with JOIN not fully supported
- DELETE/UPDATE not MVCC-aware (physically modify rows instead of marking with xmax)
  - VACUUM works but has nothing to clean (v1.5.1)
  - True MVCC DELETE/UPDATE will be added in v1.6.0
- Transactions not isolated between connections
- Parser only supports =, !=, >, < operators (no <=, >=, LIKE, IN, etc.)

## Версионирование

**Current**: v1.5.1 (VACUUM command for MVCC cleanup)
**Previous**:
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
