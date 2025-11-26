# Future Updates Roadmap

This document outlines planned features and improvements for PostgrustSQL.

## Current Version: v1.3.1

**Features:**
- 23 data types (~45% PostgreSQL compatibility)
- PostgreSQL-compatible meta-commands (\dt, \l, \du)
- FOREIGN KEY constraints with validation
- JOIN operations (INNER, LEFT, RIGHT)
- SERIAL/BIGSERIAL auto-increment
- Aggregate functions (COUNT, SUM, AVG, MIN, MAX)
- ORDER BY + LIMIT
- AND/OR in WHERE clauses
- GROUP BY
- Transactions (BEGIN/COMMIT/ROLLBACK)
- MVCC (Multi-Version Concurrency Control)
- WAL (Write-Ahead Logging)

---

## v1.4.0 - Quick Wins (1-2 hours)

### 1. OFFSET support ⏱️ 15 min
```sql
SELECT * FROM users LIMIT 10 OFFSET 20;
```
**Status:** Not implemented
**Why:** Pagination (LIMIT exists, OFFSET doesn't)
**Complexity:** Trivial (`.skip()` in iterator)
**Files:** `src/parser.rs`, `src/executor.rs`

### 2. DISTINCT ⏱️ 30 min
```sql
SELECT DISTINCT city FROM users;
SELECT COUNT(DISTINCT category) FROM products;
```
**Status:** Not implemented
**Why:** Get unique values
**Complexity:** Trivial (HashSet)
**Files:** `src/parser.rs`, `src/executor.rs`

### 3. UNIQUE constraint ⏱️ 1-2 hours
```sql
CREATE TABLE users (
    email VARCHAR(255) UNIQUE,
    username VARCHAR(50) UNIQUE NOT NULL
);
```
**Status:** Not implemented
**Why:** Email/username uniqueness validation
**Complexity:** Low (check on INSERT/UPDATE)
**Files:** `src/types.rs` (Column.unique), `src/parser.rs`, `src/executor.rs`

---

## v1.4.1 - ALTER TABLE (2-3 hours)

### 4. ALTER TABLE operations
```sql
ALTER TABLE users ADD COLUMN phone VARCHAR(20);
ALTER TABLE users DROP COLUMN age;
ALTER TABLE users RENAME COLUMN name TO full_name;
ALTER TABLE users RENAME TO customers;
```
**Status:** Not implemented
**Why:** Schema migrations without data loss
**Complexity:** Medium (modify Table structure, WAL logging)
**Files:** `src/parser.rs`, `src/executor.rs`, `src/types.rs`, `src/wal.rs`

**Implementation plan:**
- Add `Statement::AlterTable` enum variant
- Parser for ADD/DROP/RENAME syntax
- Executor logic to modify Table.columns
- WAL operations for ALTER TABLE
- Handle SERIAL sequences on column changes

---

## v1.4.2 - Constraints & Defaults (3-4 hours)

### 5. DEFAULT values ⏱️ 2-3 hours
```sql
CREATE TABLE logs (
    id SERIAL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    status VARCHAR(20) DEFAULT 'pending',
    uuid UUID DEFAULT gen_random_uuid()
);
```
**Status:** Not implemented
**Why:** Auto-fill columns, less boilerplate in INSERT
**Complexity:** Medium (parse DEFAULT, implement NOW(), gen_random_uuid())
**Files:** `src/types.rs` (Column.default), `src/parser.rs`, `src/executor.rs`

**Functions to implement:**
- `CURRENT_TIMESTAMP` / `NOW()`
- `CURRENT_DATE`
- `gen_random_uuid()`
- Literal defaults (numbers, strings)

### 6. CHECK constraints ⏱️ 2 hours
```sql
CREATE TABLE products (
    price NUMERIC(10,2) CHECK (price > 0),
    stock INTEGER CHECK (stock >= 0),
    category TEXT CHECK (category IN ('electronics', 'books', 'food'))
);
```
**Status:** Not implemented
**Why:** Data validation at database level
**Complexity:** Medium (parse conditions, validate on INSERT/UPDATE)
**Files:** `src/types.rs` (Column.check), `src/parser.rs`, `src/executor.rs`

---

## v1.5.0 - Indexes & Performance (6-8 hours)

### 7. CREATE/DROP INDEX ⏱️ 6-8 hours **PRIORITY #1**
```sql
CREATE INDEX idx_users_email ON users(email);
CREATE UNIQUE INDEX idx_products_sku ON products(sku);
DROP INDEX idx_users_email;
```
**Status:** Not implemented
**Why:** Performance! All queries are O(n) full table scan. With indexes: O(log n)
**Complexity:** High (BTreeMap per index, update on INSERT/UPDATE/DELETE)
**Impact:** 10-100x speedup on large tables

**Implementation plan:**
- `Table.indexes: HashMap<String, BTreeMap<Value, Vec<usize>>>` (index_name -> value -> row_ids)
- Parser: CREATE INDEX / DROP INDEX
- Executor: build index on CREATE, use in WHERE clauses
- Maintain indexes on INSERT/UPDATE/DELETE
- UNIQUE indexes (prevent duplicates)

**Files:** `src/types.rs`, `src/parser.rs`, `src/executor.rs`, `src/storage.rs`

### 8. EXPLAIN ⏱️ 2-3 hours
```sql
EXPLAIN SELECT * FROM users WHERE email = 'test@test.com';
```
**Status:** Not implemented
**Why:** Debug performance, show query plan (index usage)
**Complexity:** Medium
**Output:**
```
Query Plan:
 -> Index Scan using idx_users_email (cost=0.1 rows=1)
    Filter: email = 'test@test.com'
```

### 9. Multiple JOINs ⏱️ 3-4 hours
```sql
SELECT * FROM users
    JOIN orders ON users.id = orders.user_id
    JOIN products ON orders.product_id = products.id
    LEFT JOIN reviews ON products.id = reviews.product_id;
```
**Status:** Only single JOIN supported
**Why:** Complex queries with multiple tables
**Complexity:** Medium (recursive JOIN execution)
**Files:** `src/parser.rs`, `src/executor.rs`

---

## v1.5.1 - VACUUM & Cleanup (2-3 hours)

### 10. VACUUM ⏱️ 2-3 hours **CRITICAL for production**
```sql
VACUUM;
VACUUM FULL;
VACUUM ANALYZE;
```
**Status:** Not implemented
**Why:** Remove old row versions (MVCC cleanup). Currently memory leak!
**Complexity:** Medium (iterate tables, remove rows where xmax < oldest_active_tx)
**Impact:** Critical for long-running servers

**Implementation plan:**
- Track oldest active transaction ID
- Remove rows where `xmax.is_some() && xmax < oldest_tx`
- Compact table storage
- Update statistics for query planner

**Files:** `src/types.rs`, `src/executor.rs`, `src/transaction_manager.rs`

### 11. String functions ⏱️ 2 hours
```sql
SELECT UPPER(name), LOWER(email), LENGTH(description) FROM users;
SELECT CONCAT(first_name, ' ', last_name) AS full_name FROM users;
SELECT SUBSTRING(email, 1, POSITION('@' IN email) - 1) AS username FROM users;
SELECT TRIM(BOTH ' ' FROM name) FROM users;
```
**Status:** Not implemented
**Why:** Data processing in queries
**Complexity:** Low (simple functions)

### 12. Math functions ⏱️ 1 hour
```sql
SELECT ABS(balance), ROUND(price, 2), CEIL(rating), FLOOR(score) FROM products;
SELECT SQRT(area), POWER(base, 2), MOD(id, 10) FROM data;
```

### 13. Date/Time functions ⏱️ 3-4 hours
```sql
SELECT NOW(), CURRENT_DATE, CURRENT_TIMESTAMP;
SELECT DATE_PART('year', created_at), EXTRACT(MONTH FROM birth_date) FROM users;
SELECT DATE_TRUNC('day', created_at) FROM events;
SELECT AGE(birth_date), created_at + INTERVAL '1 day' FROM logs;
```
**Status:** Not implemented
**Why:** Date filtering, grouping, calculations
**Complexity:** Medium (use chrono crate)

---

## v1.6.0 - Advanced Features (8-12 hours)

### 14. Subqueries ⏱️ 6-8 hours
```sql
SELECT * FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > 1000);
SELECT * FROM products WHERE price > (SELECT AVG(price) FROM products);
SELECT name, (SELECT COUNT(*) FROM orders WHERE orders.user_id = users.id) AS order_count FROM users;
```
**Status:** Not implemented
**Why:** Complex queries
**Complexity:** High (nested execution, optimization)

### 15. Views ⏱️ 2-3 hours
```sql
CREATE VIEW active_users AS
    SELECT * FROM users WHERE active = true;

SELECT * FROM active_users WHERE age > 18;

DROP VIEW active_users;
```
**Status:** Not implemented
**Why:** Reusable queries, abstraction
**Complexity:** Medium (store as Statement, execute on SELECT)

### 16. Connection pooling ⏱️ 4-6 hours
**Status:** Single-threaded
**Why:** Handle concurrent connections properly
**Complexity:** High (Arc<Mutex<Database>>, transaction isolation)
**Impact:** Production readiness

**Implementation plan:**
- Wrap Database in Arc<RwLock<Database>>
- Read locks for SELECT
- Write locks for INSERT/UPDATE/DELETE
- Per-connection transaction state
- Deadlock detection

---

## v1.7.0 - Optimization & Polish

### 17. Query optimizer
- Cost-based query planning
- Index selection
- Join order optimization
- Statistics collection

### 18. COPY command
```sql
COPY users FROM '/path/to/users.csv' WITH (FORMAT csv, HEADER true);
COPY users TO '/path/to/export.csv' WITH (FORMAT csv);
```

### 19. Triggers
```sql
CREATE TRIGGER update_timestamp
    BEFORE UPDATE ON users
    FOR EACH ROW
    EXECUTE FUNCTION update_modified_column();
```

### 20. Stored procedures
```sql
CREATE FUNCTION get_user_stats(user_id INTEGER)
RETURNS TABLE(order_count INTEGER, total_spent NUMERIC)
AS $$
    SELECT COUNT(*), SUM(total) FROM orders WHERE orders.user_id = $1;
$$ LANGUAGE SQL;
```

---

## Low Priority (Maybe Later)

### ARRAY types
```sql
CREATE TABLE tags (post_id INTEGER, tags TEXT[]);
INSERT INTO tags VALUES (1, ARRAY['rust', 'database', 'sql']);
SELECT * FROM tags WHERE 'rust' = ANY(tags);
```

### JSON functions
```sql
SELECT data->>'name', data->'address'->>'city' FROM users;
SELECT * FROM events WHERE metadata @> '{"type": "click"}';
```

### Full-text search
```sql
CREATE INDEX idx_posts_content ON posts USING GIN(to_tsvector('english', content));
SELECT * FROM posts WHERE to_tsvector('english', content) @@ to_tsquery('rust & database');
```

### Geometric types
```sql
CREATE TABLE locations (name TEXT, point POINT, polygon POLYGON);
SELECT * FROM locations WHERE point <-> POINT(0,0) < 100;
```

### Network types
```sql
CREATE TABLE servers (hostname TEXT, ip INET, mac MACADDR);
SELECT * FROM servers WHERE ip << '192.168.1.0/24';
```

---

## Testing Strategy

For each version:
1. Write integration test script: `test_v1.4.0.sh`
2. Update unit tests in relevant modules
3. Update CLAUDE.md with new features
4. Create git commit with detailed changelog
5. Tag release: `git tag -a v1.4.0 -m "..."`

---

## Performance Targets

- **Without indexes:** Handle 10K rows comfortably
- **With indexes (v1.5.0):** handle 100K+ rows
- **With connection pooling (v1.6.0):** handle 100+ concurrent connections
- **With query optimizer (v1.7.0):** compete with SQLite

---

## Notes

- Always maintain backward compatibility
- Keep PostgreSQL syntax compatibility where possible
- Document breaking changes clearly
- Test with real-world workloads
- Benchmark before/after major changes
