# Page-Based Storage Integration Plan

## Current Status (v1.5.0-WIP)

### ✅ Completed (100%)
1. **Page-based storage infrastructure** (46 tests, all passing)
   - Page (8KB storage unit)
   - BufferPool (LRU cache)
   - PageManager (disk I/O)
   - PagedTable (per-table rows)
   - DatabaseStorage (high-level API)

2. **Executor modularization** (1,888 lines modular code)
   - RowStorage trait abstraction
   - DmlExecutor, DdlExecutor, QueriesExecutor
   - ConditionEvaluator
   - Minimal dispatcher (146 lines)

3. **Feature flag infrastructure**
   - `page_storage` feature in Cargo.toml
   - DatabaseStorage initialized in Server
   - Compile-time backend selection

### ⏳ Remaining Work (Integration)

## Phase 1: Simplify Integration Approach

**Problem**: Current approach with `#[cfg(feature = "page_storage")]` everywhere is too complex.

**Solution**: Use runtime selection via Option instead of compile-time feature flags.

### Step 1.1: Change Server to always include DatabaseStorage
```rust
pub struct Server {
    instance: Arc<Mutex<ServerInstance>>,
    storage: Arc<Mutex<StorageEngine>>,          // Legacy backend
    database_storage: Option<Arc<Mutex<DatabaseStorage>>>,  // New backend (optional)
    tx_manager: TransactionManager,
}
```

### Step 1.2: Initialize based on environment variable
```rust
let use_page_storage = std::env::var("RUSTDB_USE_PAGE_STORAGE")
    .map(|v| v == "1" || v == "true")
    .unwrap_or(false);

let database_storage = if use_page_storage {
    Some(Arc::new(Mutex::new(DatabaseStorage::new(data_dir, 1000)?)))
} else {
    None
};
```

### Step 1.3: Pass to all handlers
```rust
async fn handle_client_auto(
    ...,
    database_storage: Option<Arc<Mutex<DatabaseStorage>>>,
) -> Result<...> {
    // Use database_storage if available, otherwise use legacy
}
```

## Phase 2: Wire CREATE TABLE

### Step 2.1: Modify DdlExecutor::create_table
```rust
pub fn create_table(
    db: &mut Database,
    database_storage: Option<&mut DatabaseStorage>,  // NEW
    name: String,
    columns: Vec<ColumnDef>,
    storage: Option<&mut StorageEngine>,
) -> Result<QueryResult, DatabaseError> {
    let table = Table::new(name.clone(), columns);
    
    // Create in database_storage if available
    if let Some(db_storage) = database_storage {
        db_storage.create_table(&name)?;
    }
    
    // Also create in memory (for now, both backends)
    db.create_table(table)?;
    
    Ok(QueryResult::Success(...))
}
```

### Step 2.2: Update legacy.rs dispatcher
```rust
Statement::CreateTable { name, columns } => {
    let mut db_storage_guard = database_storage.as_ref()
        .map(|ds| ds.lock().unwrap());
    let db_storage_mut = db_storage_guard.as_deref_mut();
    
    DdlExecutor::create_table(db, db_storage_mut, name, columns, storage)
}
```

## Phase 3: Wire INSERT

### Step 3.1: Use PagedStorage when database_storage available
```rust
Statement::Insert { table, columns, values } => {
    if let Some(db_storage) = database_storage.as_ref() {
        // Use PagedStorage
        let mut db_storage_guard = db_storage.lock().unwrap();
        let paged_table = db_storage_guard.get_paged_table_mut(&table)?;
        let mut paged_storage = PagedStorage::new(paged_table);
        
        DmlExecutor::insert_with_storage(
            &table_columns, &table_sequences, sequences_mut,
            &all_tables, &table, columns, values,
            &mut paged_storage, storage, tx_manager
        )
    } else {
        // Use LegacyStorage (Vec<Row>)
        let table_mut = db.get_table_mut(&table).unwrap();
        let mut legacy_storage = LegacyStorage::new(&mut table_mut.rows);
        
        DmlExecutor::insert_with_storage(
            &table_columns, &table_sequences, sequences_mut,
            &all_tables, &table, columns, values,
            &mut legacy_storage, storage, tx_manager
        )
    }
}
```

## Phase 4: Wire SELECT

### Step 4.1: Modify QueriesExecutor::select
```rust
pub fn select(
    db: &Database,
    database_storage: Option<&DatabaseStorage>,  // NEW
    ...
) -> Result<QueryResult, DatabaseError> {
    if let Some(db_storage) = database_storage {
        // Use PagedTable for reading rows
        let paged_table = db_storage.get_paged_table(table_name)?;
        let all_rows = paged_table.get_all_rows()?;
        // Filter, sort, return
    } else {
        // Use table.rows (legacy)
        let table = db.get_table(table_name)?;
        // Existing logic
    }
}
```

## Phase 5: Testing

### Step 5.1: Test with legacy backend (default)
```bash
cargo test
cargo run --release
# Everything should work as before
```

### Step 5.2: Test with page-based backend
```bash
RUSTDB_USE_PAGE_STORAGE=1 cargo run --release
# Create table, insert, select should work
```

### Step 5.3: Integration test
```bash
./tests/integration/test_page_storage_integration.sh
```

## Phase 6: Migration & Cleanup

### Step 6.1: Create migration tool
```bash
cargo run --bin migrate_to_pages
# Converts .db files to page-based format
```

### Step 6.2: Make page-based default
```rust
let use_page_storage = std::env::var("RUSTDB_USE_LEGACY")
    .map(|v| v != "1")
    .unwrap_or(true);  // Default to page-based
```

### Step 6.3: Eventually remove Vec<Row> backend (v2.0.0)
- Delete LegacyStorage
- Remove Table.rows field
- Only keep PagedTable

## Success Criteria

- [ ] Server starts with RUSTDB_USE_PAGE_STORAGE=1
- [ ] CREATE TABLE creates PagedTable
- [ ] INSERT writes to pages
- [ ] SELECT reads from pages
- [ ] Write amplification: ~100M x → ~80x (verified via benchmark)
- [ ] All existing tests pass with both backends

## Challenges

1. **Dual backend maintenance**: Keep both Vec<Row> and PagedTable working during transition
2. **Transaction isolation**: MVCC needs to work with PagedTable
3. **WAL compatibility**: Log page operations, not row operations
4. **Performance verification**: Benchmark to confirm 80x improvement

## Timeline Estimate

- Phase 1-2: Runtime selection + CREATE TABLE (1-2 days)
- Phase 3: INSERT integration (1 day)
- Phase 4: SELECT integration (1 day)
- Phase 5: Testing (1 day)
- Phase 6: Migration & cleanup (1 day)

**Total**: ~1 week for full integration

## Notes

- Keep feature flag for now, migrate to runtime selection incrementally
- Don't remove Vec<Row> backend until page-based is fully proven
- Benchmark at each phase to track progress
