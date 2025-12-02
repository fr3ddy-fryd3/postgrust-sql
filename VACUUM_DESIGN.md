# VACUUM Design (v1.5.1)

## Overview

VACUUM removes dead tuples (row versions) created by MVCC operations, reclaiming space and maintaining database performance.

## Problem Statement

### Current Behavior:
```rust
// UPDATE creates new version, old marked with xmax
UPDATE users SET name = 'Bob' WHERE id = 1;
// Old version: Row { values: [...], xmin: 100, xmax: Some(101) }  ← DEAD
// New version: Row { values: [...], xmin: 101, xmax: None }       ← ALIVE

// DELETE marks as deleted
DELETE FROM users WHERE id = 2;
// Row { values: [...], xmin: 50, xmax: Some(102) }  ← DEAD
```

**Problem:** Dead rows accumulate, wasting memory/disk space.

---

## PostgreSQL Reference

### Commands:
```sql
VACUUM;                    -- Vacuum all tables
VACUUM table_name;         -- Vacuum specific table
VACUUM FULL;               -- Aggressive vacuum with table rewrite (future)
VACUUM ANALYZE;            -- Vacuum + update statistics (future)
```

### What VACUUM Does:
1. Scans table for dead tuples (xmax set and committed)
2. Removes dead tuples from storage
3. Updates free space map (FSM) - we'll skip for v1.5.1
4. Updates statistics (optional, future)

---

## Architecture Design

### Phase 1: Basic VACUUM (v1.5.1)

**Goal:** Remove dead tuples from Vec<Row> or PagedTable.

#### 1.1 Parser Extension

```rust
// src/parser/statement.rs
pub enum Statement {
    // ... existing variants
    Vacuum { table: Option<String> },  // None = all tables
}

// src/parser/ddl.rs
pub fn parse_vacuum(input: &str) -> IResult<&str, Statement> {
    let (input, _) = tag_no_case("VACUUM")(input)?;
    let (input, _) = multispace0(input)?;

    // Optional table name
    let (input, table) = opt(parse_identifier)(input)?;

    Ok((input, Statement::Vacuum { table }))
}
```

#### 1.2 Dead Tuple Detection

```rust
// src/types/row.rs
impl Row {
    /// Check if row is dead (deleted and no longer visible to any transaction)
    pub fn is_dead(&self, oldest_active_tx: u64) -> bool {
        // Row is dead if:
        // 1. It has xmax set (deleted/updated)
        // 2. xmax < oldest_active_tx (committed before all active txs)
        match self.xmax {
            Some(xmax) => xmax < oldest_active_tx,
            None => false,
        }
    }
}
```

**Key insight:** Can only remove tuples invisible to ALL transactions.

#### 1.3 TransactionManager Extension

```rust
// src/transaction/mod.rs
impl TransactionManager {
    /// Get oldest active transaction ID
    /// Used by VACUUM to determine safe cleanup horizon
    pub fn get_oldest_active_tx(&self) -> u64 {
        let active = self.active_transactions.lock().unwrap();

        if active.is_empty() {
            // No active transactions - can clean up to current_tx_id
            self.current_tx_id.load(Ordering::SeqCst)
        } else {
            // Find minimum active tx_id
            *active.iter().min().unwrap_or(&self.current_tx_id.load(Ordering::SeqCst))
        }
    }
}
```

#### 1.4 VACUUM Executor

```rust
// src/executor/vacuum.rs
pub struct VacuumExecutor;

impl VacuumExecutor {
    /// Execute VACUUM command
    pub fn vacuum(
        db: &mut Database,
        table_name: Option<String>,
        tx_manager: &TransactionManager,
        database_storage: Option<&mut crate::storage::DatabaseStorage>,
    ) -> Result<QueryResult, DatabaseError> {
        let oldest_tx = tx_manager.get_oldest_active_tx();

        let tables_to_vacuum = if let Some(name) = table_name {
            vec![name]
        } else {
            db.tables.keys().cloned().collect()
        };

        let mut total_removed = 0;

        for table_name in tables_to_vacuum {
            let removed = Self::vacuum_table(
                db,
                &table_name,
                oldest_tx,
                database_storage
            )?;
            total_removed += removed;
        }

        Ok(QueryResult::Success(format!(
            "VACUUM complete. Removed {} dead tuples.",
            total_removed
        )))
    }

    /// Vacuum single table
    fn vacuum_table(
        db: &mut Database,
        table_name: &str,
        oldest_tx: u64,
        database_storage: Option<&mut crate::storage::DatabaseStorage>,
    ) -> Result<usize, DatabaseError> {
        if let Some(db_storage) = database_storage {
            // Page-based storage
            Self::vacuum_paged_table(db_storage, table_name, oldest_tx)
        } else {
            // Legacy storage
            Self::vacuum_legacy_table(db, table_name, oldest_tx)
        }
    }

    /// Vacuum legacy Vec<Row> storage
    fn vacuum_legacy_table(
        db: &mut Database,
        table_name: &str,
        oldest_tx: u64,
    ) -> Result<usize, DatabaseError> {
        let table = db.get_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        let before = table.rows.len();

        // Retain only alive rows
        table.rows.retain(|row| !row.is_dead(oldest_tx));

        let after = table.rows.len();
        Ok(before - after)
    }

    /// Vacuum page-based storage
    fn vacuum_paged_table(
        db_storage: &mut crate::storage::DatabaseStorage,
        table_name: &str,
        oldest_tx: u64,
    ) -> Result<usize, DatabaseError> {
        let paged_table = db_storage.get_paged_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        // Get all rows
        let all_rows = paged_table.get_all_rows()?;
        let before = all_rows.len();

        // Filter alive rows
        let alive_rows: Vec<_> = all_rows.into_iter()
            .filter(|row| !row.is_dead(oldest_tx))
            .collect();

        let after = alive_rows.len();

        // Rewrite table with only alive rows
        // This is inefficient but simple for v1.5.1
        // TODO v1.6: In-place page compaction
        paged_table.clear()?;
        for row in alive_rows {
            paged_table.insert(row)?;
        }

        Ok(before - after)
    }
}
```

#### 1.5 Integration Points

**Wire into executor:**
```rust
// src/executor/legacy.rs
match stmt {
    // ... existing cases
    Statement::Vacuum { table } => {
        VacuumExecutor::vacuum(db, table, tx_manager, database_storage)
    }
}
```

**Add to parser:**
```rust
// src/parser/mod.rs
pub fn parse_statement(input: &str) -> Result<Statement, ParseError> {
    // ... existing parsers

    if input.trim_start().to_lowercase().starts_with("vacuum") {
        return parse_vacuum(input).map(|(_, stmt)| stmt);
    }

    // ... rest
}
```

---

## Implementation Plan

### Step 1: Extend Transaction Manager ✅
- Add `get_oldest_active_tx()` method
- Track active transactions properly
- Unit tests

### Step 2: Add Row::is_dead() ✅
- Implement dead tuple detection
- Unit tests with various xmin/xmax scenarios

### Step 3: Create VACUUM Executor ✅
- vacuum_legacy_table() - simple Vec::retain()
- vacuum_paged_table() - clear + reinsert (inefficient but works)
- Unit tests

### Step 4: Parser Integration ✅
- Add Statement::Vacuum
- parse_vacuum() implementation
- Integration tests

### Step 5: End-to-End Testing ✅
- Test VACUUM after UPDATE
- Test VACUUM after DELETE
- Test VACUUM with active transactions (should NOT remove visible tuples)
- Benchmark: space reclamation

---

## Testing Strategy

### Unit Tests:
```rust
#[test]
fn test_row_is_dead() {
    let dead_row = Row {
        values: vec![],
        xmin: 100,
        xmax: Some(150),
    };

    assert!(dead_row.is_dead(200));   // Deleted before oldest tx
    assert!(!dead_row.is_dead(140));  // Still visible to tx 140
}

#[test]
fn test_vacuum_removes_dead_tuples() {
    // Create table, insert, update, vacuum
    // Assert: old version removed, new version retained
}
```

### Integration Tests:
```bash
# tests/integration/test_vacuum.sh
CREATE TABLE t (id INTEGER, value TEXT);
INSERT INTO t VALUES (1, 'v1'), (2, 'v2'), (3, 'v3');

UPDATE t SET value = 'updated' WHERE id = 1;
DELETE FROM t WHERE id = 2;

# Now have: 3 alive + 2 dead tuples = 5 total

VACUUM t;

# Should remove 2 dead tuples, keep 3 alive
```

---

## Performance Considerations

### v1.5.1 (Simple Implementation):
- Legacy: `O(n)` - single pass with Vec::retain()
- Paged: `O(n)` - clear + reinsert (inefficient but correct)

### Future Optimizations (v1.6+):
- In-place page compaction (move tuples within page)
- Free Space Map (FSM) for tracking empty space
- Background auto-vacuum daemon
- Partial VACUUM (only modified pages)

---

## Limitations (v1.5.1)

1. **No VACUUM FULL** - doesn't rewrite pages for compaction
2. **No statistics update** - no ANALYZE integration
3. **Blocking operation** - holds exclusive lock during vacuum
4. **Inefficient for paged storage** - full table rewrite
5. **No FSM** - doesn't track free space for future inserts

**These are acceptable for v1.5.1 - focus is correctness, not optimization.**

---

## Usage Examples

```sql
-- Vacuum all tables
VACUUM;

-- Vacuum specific table
VACUUM users;

-- Check effect
SELECT COUNT(*) FROM users;  -- Counts only alive rows

-- Typical workflow
BEGIN;
  UPDATE users SET status = 'active' WHERE id < 1000;
COMMIT;

-- Creates dead tuples (old status versions)
-- Reclaim space:
VACUUM users;
```

---

## Success Criteria

✅ VACUUM command parses correctly
✅ Dead tuples removed from Vec<Row>
✅ Dead tuples removed from PagedTable
✅ Active transactions block VACUUM horizon
✅ All tests passing
✅ Documentation complete

---

## Files to Create/Modify

**New:**
- `src/executor/vacuum.rs` - VacuumExecutor implementation
- `tests/integration/test_vacuum.sh` - E2E tests

**Modified:**
- `src/types/row.rs` - Add is_dead() method
- `src/transaction/mod.rs` - Add get_oldest_active_tx()
- `src/parser/statement.rs` - Add Statement::Vacuum
- `src/parser/ddl.rs` - Add parse_vacuum()
- `src/parser/mod.rs` - Wire vacuum parser
- `src/executor/legacy.rs` - Wire VacuumExecutor
- `src/executor/mod.rs` - Export vacuum module
- `CLAUDE.md` - Update with VACUUM docs

---

## Timeline Estimate

- **Step 1-2:** 30 min (extend TransactionManager + Row)
- **Step 3:** 1 hour (implement VacuumExecutor)
- **Step 4:** 30 min (parser integration)
- **Step 5:** 1 hour (testing + debug)

**Total: ~3 hours for v1.5.1 VACUUM**

---

## Next: v1.6.0 Indexes

After VACUUM is complete, v1.6.0 will add:
- B-tree indexes for fast lookups
- CREATE INDEX / DROP INDEX commands
- Query planner integration
- Index-only scans

Then v1.7.0: Comprehensive logging system.

---

**Status:** Design complete, ready to implement! 🚀
