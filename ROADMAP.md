# PostgRustSQL Roadmap

Долгосрочный план развития проекта.

---

## 📊 Version Summary

| Version | Focus | Key Features | Status |
|---------|-------|--------------|--------|
| v1.9.0 | Composite Indexes | Multi-column indexes | ✅ Complete |
| v1.10.0 | SQL Features | CASE, UNION, Views | ✅ Complete |
| v1.11.0 | Stability | Critical fixes | ✅ Complete |
| v2.0.0 | PostgreSQL | Auth protocol + system catalogs | ✅ Complete |
| v2.1.0 | Transactions | Multi-connection isolation (DML) | ✅ Complete |
| v2.2.0 | Backup Tools | pgr_dump/pgr_restore | ✅ Complete |
| v2.3.0 | RBAC | Role-based access control | ✅ Complete |
| v2.4.0 | Protocol Extensions | Extended Query + COPY | ✅ Complete |
| v2.5.0 | Binary COPY | PostgreSQL binary format | ✅ Complete (2025-12-26) |
| **v2.6.0** | **Advanced SQL** | **Subqueries + Window Functions + Multi-JOIN** | **✅ Complete (2025-12-29)** |

---

## 🎯 Current Status

**Recently Completed:**
- ✅ v2.6.0 - Window Functions + Subqueries + Multi-JOIN (2025-12-29)
  - Window functions: ROW_NUMBER, RANK, DENSE_RANK, LAG, LEAD
  - Subqueries: IN/NOT IN, EXISTS/NOT EXISTS, scalar subqueries, nested queries
  - Multi-JOIN: Multiple JOINs in single query
  - Fixed: Aggregates (COUNT, SUM, AVG, MIN, MAX) now work with JOIN
- ✅ v2.5.0 - COPY Binary Format (2025-12-26)
  - Full PostgreSQL-compatible binary protocol for all 23 data types
  - 3-5x faster than CSV for bulk operations
- ✅ 213 unit tests passing (0 failed, 7 ignored)

**Foundation achieved:**
- PostgreSQL wire protocol v3.0 (Simple + Extended Query)
- Advanced SQL: Window functions, Subqueries, Multi-JOIN
- Multi-connection MVCC isolation (DML)
- Page-based storage with WAL
- B-tree & Hash indexes (single + composite)
- Backup & Restore utilities (pgr_dump/pgr_restore)
- Role-Based Access Control (RBAC)
- Prepared statements (Extended Query Protocol)
- Bulk import/export (COPY protocol - CSV + Binary)

---

## ✅ v2.6.0 - Window Functions + Subqueries + Multi-JOIN (Complete)

**Status:** Complete (2025-12-29) | **Complexity:** Very High

### ✅ Completed Features:

**Subqueries:**
- ✅ IN / NOT IN subqueries in WHERE
- ✅ EXISTS / NOT EXISTS subqueries
- ✅ Scalar subqueries in WHERE and SELECT
- ✅ Nested subqueries (any depth)
- ✅ MVCC isolation for subqueries

**Window Functions:**
- ✅ ROW_NUMBER() OVER (...)
- ✅ RANK() / DENSE_RANK() OVER (...)
- ✅ LAG(col, offset) / LEAD(col, offset) OVER (...)
- ✅ PARTITION BY support
- ✅ ORDER BY within window spec

**Multi-JOIN:**
- ✅ Multiple JOINs in single query
- ✅ Mixed JOIN types (INNER/LEFT/RIGHT)
- ✅ Aggregates with JOIN (COUNT, SUM, AVG, MIN, MAX)

**Tests:**
- ✅ 7 subquery integration tests
- ✅ 4 multi-JOIN tests
- ✅ 4 window function tests
- ✅ 213 unit tests (all passing)

---

## 🚧 v2.7.0 - Advanced Query Features (Next)

**Status:** Planning | **Complexity:** High

### Priority 1: Subqueries in FROM clause
Derived tables для сложных аналитических запросов.

```sql
-- FROM subquery (derived table)
SELECT * FROM (SELECT * FROM users WHERE age > 18) AS adults;

-- Subquery JOIN
SELECT u.name, s.total
FROM users u
JOIN (SELECT user_id, SUM(amount) as total FROM orders GROUP BY user_id) s
ON u.id = s.user_id;
```

### Priority 2: CTEs (Common Table Expressions)
WITH clause для читаемых сложных запросов.

```sql
WITH active_users AS (
    SELECT * FROM users WHERE status = 'active'
),
recent_orders AS (
    SELECT * FROM orders WHERE date > '2024-01-01'
)
SELECT u.name, COUNT(o.id)
FROM active_users u
LEFT JOIN recent_orders o ON u.id = o.user_id
GROUP BY u.name;
```

### Priority 3: Advanced Window Functions
Расширенные аналитические функции.

```sql
-- Window frames
SUM(salary) OVER (PARTITION BY dept ROWS BETWEEN 1 PRECEDING AND 1 FOLLOWING)

-- FIRST_VALUE / LAST_VALUE
FIRST_VALUE(salary) OVER (PARTITION BY dept ORDER BY hire_date)

-- NTILE for percentiles
NTILE(4) OVER (ORDER BY salary DESC)
```

### Priority 4: pg_dump Full Compatibility
Проверить что pg_dump/pg_restore работает без костылей.

**Может потребоваться:**
- pg_depend catalog для зависимостей
- CREATE SEQUENCE support
- COMMENT ON TABLE/COLUMN

## 🚀 v2.8.0+ - Future Features (Long-term)

### Advanced SQL
- **Triggers** - Automatic actions on events (BEFORE/AFTER INSERT/UPDATE/DELETE)
- **Stored Procedures (PL/pgSQL)** - Server-side functions with control flow

### Performance
- Query cache
- Statistics collector (query planner optimization)
- Auto-VACUUM (background cleanup)
- Parallel query execution
- Connection pooling

### High Availability
- Master-slave replication
- Streaming replication (WAL shipping)
- Read replicas
- Logical replication

---

## 📚 Version History

Detailed information about completed versions (v1.9.0 - v2.5.0) can be found in:
- `git log` - Full commit history
- CLAUDE.md - Current features and architecture

**Last Updated:** 2025-12-28
