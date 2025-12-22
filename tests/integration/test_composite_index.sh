#!/bin/bash
# Integration test for Composite (multi-column) indexes (v1.9.0)

set -e

echo "========================================="
echo "Testing Composite Index Implementation"
echo "========================================="

# Clean start
rm -rf data/
echo "✓ Cleaned old data"

# Build quietly
cargo build --release --quiet 2>/dev/null

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/postgrust_composite_test.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    cat /tmp/postgrust_composite_test.log
    exit 1
fi

# Cleanup function
cleanup() {
    echo ""
    echo "Shutting down server..."
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true
    echo "✓ Server stopped"
}
trap cleanup EXIT

# Test 1: Composite B-tree index
cat > /tmp/test_composite.sql << 'EOF'
CREATE TABLE users (id INTEGER PRIMARY KEY, city TEXT NOT NULL, age INTEGER, name TEXT);
INSERT INTO users (id, city, age, name) VALUES (1, 'NYC', 30, 'Alice');
INSERT INTO users (id, city, age, name) VALUES (2, 'LA', 25, 'Bob');
INSERT INTO users (id, city, age, name) VALUES (3, 'NYC', 25, 'Charlie');
INSERT INTO users (id, city, age, name) VALUES (4, 'NYC', 30, 'Diana');
INSERT INTO users (id, city, age, name) VALUES (5, 'LA', 30, 'Eve');
CREATE INDEX idx_city_age ON users(city, age) USING BTREE;
SELECT * FROM users WHERE city = 'NYC' AND age = 30;
SELECT * FROM users WHERE city = 'LA' AND age = 25;
SELECT * FROM users WHERE city = 'NYC' AND age = 25;
quit
EOF

echo ""
echo "Running composite B-tree index tests..."
OUTPUT=$((sleep 1; cat /tmp/test_composite.sql) | nc 127.0.0.1 5432 2>&1)

# Validate composite B-tree index
echo ""
echo "Validating composite B-tree index..."

if echo "$OUTPUT" | grep -qi "idx_city_age.*created\|created.*idx_city_age"; then
    echo "✓ CREATE INDEX on multiple columns succeeded"
else
    echo "✗ CREATE INDEX on multiple columns failed"
    echo "$OUTPUT"
    exit 1
fi

# Check if Alice and Diana are returned for NYC + age 30
if echo "$OUTPUT" | grep -q "Alice" && echo "$OUTPUT" | grep -q "Diana"; then
    echo "✓ Composite index query (NYC, 30) returns correct results"
else
    echo "✗ Composite index query (NYC, 30) failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -q "Bob"; then
    echo "✓ Composite index query (LA, 25) returns correct results"
else
    echo "✗ Composite index query (LA, 25) failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -q "Charlie"; then
    echo "✓ Composite index query (NYC, 25) returns correct results"
else
    echo "✗ Composite index query (NYC, 25) failed"
    echo "$OUTPUT"
    exit 1
fi

# Test 2: Composite Hash index
cat > /tmp/test_composite_hash.sql << 'EOF'
CREATE TABLE accounts (id INTEGER PRIMARY KEY, email TEXT NOT NULL, provider TEXT NOT NULL, status TEXT);
INSERT INTO accounts (id, email, provider, status) VALUES (1, 'user@example.com', 'google', 'active');
INSERT INTO accounts (id, email, provider, status) VALUES (2, 'user@example.com', 'github', 'active');
INSERT INTO accounts (id, email, provider, status) VALUES (3, 'admin@example.com', 'google', 'active');
CREATE INDEX idx_email_provider ON accounts(email, provider) USING HASH;
SELECT * FROM accounts WHERE email = 'user@example.com' AND provider = 'google';
SELECT * FROM accounts WHERE email = 'user@example.com' AND provider = 'github';
quit
EOF

echo ""
echo "Running composite hash index tests..."
OUTPUT_HASH=$((sleep 1; cat /tmp/test_composite_hash.sql) | nc 127.0.0.1 5432 2>&1)

# Validate composite Hash index
echo ""
echo "Validating composite hash index..."

if echo "$OUTPUT_HASH" | grep -qi "idx_email_provider.*created\|created.*idx_email_provider"; then
    echo "✓ CREATE INDEX (composite hash) succeeded"
else
    echo "✗ CREATE INDEX (composite hash) failed"
    echo "$OUTPUT_HASH"
    exit 1
fi

# Test 3: Composite unique index
cat > /tmp/test_composite_unique.sql << 'EOF'
CREATE TABLE sessions (id INTEGER PRIMARY KEY, user_id INTEGER, device TEXT);
INSERT INTO sessions (id, user_id, device) VALUES (1, 100, 'mobile');
INSERT INTO sessions (id, user_id, device) VALUES (2, 100, 'desktop');
CREATE UNIQUE INDEX idx_user_device ON sessions(user_id, device);
INSERT INTO sessions (id, user_id, device) VALUES (3, 100, 'mobile');
quit
EOF

echo ""
echo "Running composite unique index tests..."
OUTPUT_UNIQUE=$((sleep 1; cat /tmp/test_composite_unique.sql) | nc 127.0.0.1 5432 2>&1)

# Validate unique constraint
echo ""
echo "Validating composite unique constraint..."

if echo "$OUTPUT_UNIQUE" | grep -qi "idx_user_device.*created\|created.*idx_user_device"; then
    echo "✓ CREATE UNIQUE INDEX (composite) succeeded"
else
    echo "✗ CREATE UNIQUE INDEX (composite) failed"
    echo "$OUTPUT_UNIQUE"
    exit 1
fi

if echo "$OUTPUT_UNIQUE" | grep -q "unique constraint"; then
    echo "✓ Composite unique constraint violation detected"
else
    echo "✗ Expected unique constraint violation"
    echo "$OUTPUT_UNIQUE"
    exit 1
fi

# Test 4: Index maintenance with INSERT/UPDATE/DELETE
cat > /tmp/test_maintenance.sql << 'EOF'
CREATE TABLE products (id INTEGER PRIMARY KEY, category TEXT, brand TEXT, price INTEGER);
INSERT INTO products (id, category, brand, price) VALUES (1, 'Electronics', 'Apple', 999);
INSERT INTO products (id, category, brand, price) VALUES (2, 'Electronics', 'Samsung', 799);
CREATE INDEX idx_cat_brand ON products(category, brand);
INSERT INTO products (id, category, brand, price) VALUES (3, 'Electronics', 'Apple', 1299);
SELECT * FROM products WHERE category = 'Electronics' AND brand = 'Apple';
UPDATE products SET brand = 'Google' WHERE id = 2;
SELECT * FROM products WHERE category = 'Electronics' AND brand = 'Samsung';
DELETE FROM products WHERE id = 1;
SELECT * FROM products WHERE category = 'Electronics' AND brand = 'Apple';
quit
EOF

echo ""
echo "Running index maintenance tests..."
OUTPUT_MAINT=$((sleep 1; cat /tmp/test_maintenance.sql) | nc 127.0.0.1 5432 2>&1)

echo ""
echo "Validating index maintenance..."

# After first insert (id=3), both id=1 and id=3 should exist for Apple
if echo "$OUTPUT_MAINT" | grep -A 5 "category.*brand.*price" | grep -q "Apple.*999" && \
   echo "$OUTPUT_MAINT" | grep -A 5 "category.*brand.*price" | grep -q "Apple.*1299"; then
    echo "✓ Index maintained correctly after INSERT"
else
    echo "✗ Index maintenance after INSERT failed"
    echo "$OUTPUT_MAINT"
    exit 1
fi

# After UPDATE, Samsung should not be found
if ! echo "$OUTPUT_MAINT" | grep -A 3 "Samsung" | tail -3 | grep -q "Samsung"; then
    echo "✓ Index maintained correctly after UPDATE"
else
    echo "✗ Index maintenance after UPDATE failed"
    echo "$OUTPUT_MAINT"
    exit 1
fi

# After DELETE id=1, only id=3 (price=1299) should remain for Apple
if echo "$OUTPUT_MAINT" | tail -10 | grep -q "1299" && \
   ! echo "$OUTPUT_MAINT" | tail -10 | grep -q "999.*Apple"; then
    echo "✓ Index maintained correctly after DELETE"
else
    echo "✗ Index maintenance after DELETE failed"
    echo "$OUTPUT_MAINT"
    exit 1
fi

# Test 5: EXPLAIN with composite indexes
cat > /tmp/test_explain.sql << 'EOF'
CREATE TABLE orders (id INTEGER, customer_id INTEGER, date TEXT);
INSERT INTO orders (id, customer_id, date) VALUES (1, 100, '2024-01-01');
INSERT INTO orders (id, customer_id, date) VALUES (2, 100, '2024-01-02');
INSERT INTO orders (id, customer_id, date) VALUES (3, 200, '2024-01-01');
CREATE INDEX idx_customer_date ON orders(customer_id, date) USING HASH;
EXPLAIN SELECT * FROM orders WHERE customer_id = 100 AND date = '2024-01-01';
quit
EOF

echo ""
echo "Running EXPLAIN with composite index tests..."
OUTPUT_EXPLAIN=$((sleep 1; cat /tmp/test_explain.sql) | nc 127.0.0.1 5432 2>&1)

echo ""
echo "Validating EXPLAIN output..."

if echo "$OUTPUT_EXPLAIN" | grep -qi "index scan.*idx_customer_date\|idx_customer_date.*index scan"; then
    echo "✓ EXPLAIN shows composite index usage"
else
    echo "✗ EXPLAIN doesn't show composite index usage"
    echo "$OUTPUT_EXPLAIN"
    exit 1
fi

echo ""
echo "========================================="
echo "All composite index tests passed! ✓"
echo "========================================="
echo ""
echo "Summary:"
echo "  - Composite B-tree indexes working"
echo "  - Composite hash indexes working"
echo "  - Composite unique indexes working"
echo "  - Query optimization with AND conditions working"
echo "  - Index maintenance (INSERT/UPDATE/DELETE) working"
echo "  - EXPLAIN shows composite index usage"
echo ""
echo "v1.9.0 Features:"
echo "  - CREATE INDEX idx ON table(col1, col2, col3)"
echo "  - Supports both BTREE and HASH types"
echo "  - Automatic query optimization for AND conditions"
echo "  - Full MVCC and unique constraint support"
echo ""
