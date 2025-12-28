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
| **v2.6.0** | **Advanced SQL** | **Subqueries + Window Functions** | **🚧 Planned** |

---

## 🎯 Current Status

**Recently Completed:**
- ✅ v2.5.0 - COPY Binary Format (2025-12-26)
  - Full PostgreSQL-compatible binary protocol for all 23 data types
  - 3-5x faster than CSV for bulk operations
  - COPY TO STDOUT + COPY FROM STDIN with binary format
- ✅ 202 unit tests passing (0 failed, 7 ignored)

**Foundation achieved:**
- PostgreSQL wire protocol v3.0 (Simple + Extended Query)
- Multi-connection MVCC isolation (DML)
- Page-based storage with WAL
- B-tree & Hash indexes (single + composite)
- Backup & Restore utilities (pgr_dump/pgr_restore)
- Role-Based Access Control (RBAC)
- Prepared statements (Extended Query Protocol)
- Bulk import/export (COPY protocol - CSV + Binary)

---

## 🚧 v2.6.0 - Subqueries & Advanced SQL (Next)

**Status:** Planning | **Complexity:** Very High

### Priority 1: Subqueries
Критическая SQL фича для production-ready использования.

```sql
-- Scalar subquery
SELECT name, (SELECT COUNT(*) FROM orders WHERE user_id = users.id)
FROM users;

-- IN subquery
SELECT * FROM products
WHERE category_id IN (SELECT id FROM categories WHERE active = true);

-- EXISTS subquery
SELECT * FROM users WHERE EXISTS (SELECT 1 FROM orders WHERE user_id = users.id);

-- FROM subquery (derived table)
SELECT * FROM (SELECT * FROM users WHERE age > 18) AS adults;
```

**Implementation:**
- Parser: nested SELECT in WHERE/FROM/SELECT
- Executor: recursive subquery execution
- MVCC: proper isolation for subqueries

### Priority 2: pg_dump Full Compatibility
Проверить что pg_dump/pg_restore работает без костылей.

**Может потребоваться:**
- pg_depend catalog для зависимостей
- CREATE SEQUENCE support
- COMMENT ON TABLE/COLUMN

### Priority 3: Window Functions
Production-ready analytics queries.

```sql
ROW_NUMBER() OVER (ORDER BY salary DESC)
RANK() OVER (PARTITION BY dept ORDER BY salary DESC)
LAG(salary, 1) OVER (ORDER BY hire_date)
SUM(salary) OVER (PARTITION BY dept)
```

**Implementation:**
- OVER clause parsing
- PARTITION BY + ORDER BY
- Window frame specification (ROWS BETWEEN)
- Window function evaluation engine

## 🚀 v2.7.0+ - Future Features (Long-term)

### Advanced SQL
- **Multiple JOINs** - More than one JOIN per query
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
