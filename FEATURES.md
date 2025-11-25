# RustDB - New Features

## 1. FOREIGN KEY Constraints

### Description
Full support for foreign key constraints to maintain referential integrity between tables.

### Syntax
```sql
CREATE TABLE parent_table (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE child_table (
    id INTEGER PRIMARY KEY,
    parent_id INTEGER NOT NULL REFERENCES parent_table(id),
    data TEXT
);
```

### Features
- ✅ `REFERENCES table(column)` syntax
- ✅ Validation at table creation (referenced table/column must exist)
- ✅ Validation at INSERT (referenced value must exist)
- ✅ Support for NULL values in FK columns (if column is nullable)
- ✅ Referenced column must be a PRIMARY KEY

### Examples
```sql
-- Create parent table
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);

-- Create child table with FK
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    product TEXT NOT NULL
);

-- Insert into parent
INSERT INTO users VALUES (1, 'Alice');
INSERT INTO users VALUES (2, 'Bob');

-- Insert into child (valid FK)
INSERT INTO orders VALUES (1, 1, 'Laptop');  -- ✓ Works

-- Insert with invalid FK
INSERT INTO orders VALUES (2, 99, 'Mouse');  -- ✗ Error: FK violation
```

## 2. JOIN Operations

### Description
Support for INNER JOIN, LEFT JOIN, and RIGHT JOIN operations.

### Syntax
```sql
SELECT * FROM table1
[INNER|LEFT|RIGHT] JOIN table2
ON table1.column = table2.column;
```

### Features
- ✅ INNER JOIN - returns matching rows from both tables
- ✅ LEFT JOIN - returns all rows from left table + matching from right (NULLs for non-matching)
- ✅ RIGHT JOIN - returns all rows from right table + matching from left (NULLs for non-matching)
- ✅ JOIN (alias for INNER JOIN)
- ✅ MVCC support - respects transaction visibility

### Examples
```sql
-- INNER JOIN (only matching rows)
SELECT * FROM users
INNER JOIN orders ON users.id = orders.user_id;

-- LEFT JOIN (all users, even without orders)
SELECT * FROM users
LEFT JOIN orders ON users.id = orders.user_id;

-- RIGHT JOIN (all orders, even if user doesn't exist)
SELECT * FROM orders
RIGHT JOIN users ON orders.user_id = users.id;

-- Simple JOIN (same as INNER JOIN)
SELECT * FROM users
JOIN orders ON users.id = orders.user_id;
```

### Current Limitations
- Only one JOIN per query (no chaining yet)
- WHERE filtering not yet supported with JOINs
- Column selection not yet implemented (returns all columns)

## 3. SERIAL (Auto-increment)

### Description
PostgreSQL-like SERIAL type for auto-incrementing integer primary keys.

### Syntax
```sql
CREATE TABLE table_name (
    id SERIAL,
    other_column TEXT
);
```

### Features
- ✅ Automatic PRIMARY KEY and NOT NULL
- ✅ Auto-increment on INSERT (starts from 1)
- ✅ No need to specify id in INSERT
- ✅ Sequence updates correctly when explicit values are inserted
- ✅ Compatible with FOREIGN KEY constraints

### Examples
```sql
-- Create table with SERIAL
CREATE TABLE users (
    id SERIAL,
    name TEXT NOT NULL,
    email TEXT
);

-- Insert without specifying id (auto-increments)
INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com');  -- id=1
INSERT INTO users (name, email) VALUES ('Bob', 'bob@example.com');      -- id=2
INSERT INTO users (name, email) VALUES ('Charlie', 'charlie@example.com');  -- id=3

-- View results
SELECT * FROM users;
-- Output:
-- ┌────┬─────────┬─────────────────────┐
-- │ id │ name    │ email               │
-- ├────┼─────────┼─────────────────────┤
-- │ 1  │ Alice   │ alice@example.com   │
-- │ 2  │ Bob     │ bob@example.com     │
-- │ 3  │ Charlie │ charlie@example.com │
-- └────┴─────────┴─────────────────────┘
```

### Combined Example (SERIAL + FK + JOIN)
```sql
-- Users with SERIAL id
CREATE TABLE users (
    id SERIAL,
    name TEXT NOT NULL
);

-- Products with SERIAL id
CREATE TABLE products (
    id SERIAL,
    name TEXT NOT NULL,
    price INTEGER NOT NULL
);

-- Orders with SERIAL id and FKs
CREATE TABLE orders (
    id SERIAL,
    user_id INTEGER NOT NULL REFERENCES users(id),
    product_id INTEGER NOT NULL REFERENCES products(id),
    quantity INTEGER NOT NULL
);

-- Insert data (no ids needed!)
INSERT INTO users (name) VALUES ('Alice');
INSERT INTO users (name) VALUES ('Bob');

INSERT INTO products (name, price) VALUES ('Laptop', 1000);
INSERT INTO products (name, price) VALUES ('Mouse', 50);

INSERT INTO orders (user_id, product_id, quantity) VALUES (1, 1, 1);
INSERT INTO orders (user_id, product_id, quantity) VALUES (2, 2, 3);

-- Join all tables
SELECT users.name, products.name, orders.quantity
FROM orders
JOIN users ON orders.user_id = users.id
JOIN products ON orders.product_id = products.id;
```

## Testing

### Unit Tests
```bash
cargo test  # 66+ tests passing
```

### Integration Tests
```bash
# Test FOREIGN KEY and JOIN
./test_fk_join.sh

# Test SERIAL
./test_serial.sh

# Quick SERIAL test
./test_serial_quick.sh
```

## Architecture Notes

### Foreign Keys
- Validation happens in `executor.rs`:
  - `create_table()` - validates referenced table/column exists
  - `insert()` - validates referenced value exists
- Uses MVCC visibility for FK checks

### JOIN
- Implementation in `executor.rs::select_with_join()`
- Nested loop join algorithm
- MVCC-aware (respects transaction visibility)
- Returns combined columns: `table1.col1, table1.col2, table2.col1, ...`

### SERIAL
- Stored as `DataType::Serial` in schema
- Each table maintains `sequences: HashMap<String, i64>`
- Sequence updated on INSERT in `executor.rs`
- Automatically converted to INTEGER with NOT NULL and PRIMARY KEY constraints
- Sequence value = max(current_seq, last_inserted_value + 1)
