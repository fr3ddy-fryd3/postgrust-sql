#!/bin/bash
# Integration test for SQL Expressions & Set Operations (v1.10.0)

set -e

echo "========================================="
echo "Testing v1.10.0: CASE, UNION, INTERSECT, EXCEPT"
echo "========================================="

# Clean start
rm -rf data/
echo "✓ Cleaned old data"

# Build quietly
cargo build --release --quiet 2>/dev/null

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/postgrust_v110_test.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    cat /tmp/postgrust_v110_test.log
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

# Create test SQL file
cat > /tmp/test_v110.sql << 'EOF'
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER, status TEXT);
INSERT INTO users (id, name, age, status) VALUES (1, 'Alice', 15, 'active');
INSERT INTO users (id, name, age, status) VALUES (2, 'Bob', 25, 'active');
INSERT INTO users (id, name, age, status) VALUES (3, 'Charlie', 70, 'inactive');
INSERT INTO users (id, name, age, status) VALUES (4, 'David', 45, 'active');
SELECT name, age, CASE WHEN age < 18 THEN 'minor' WHEN age < 65 THEN 'adult' ELSE 'senior' END AS category FROM users;
CREATE TABLE customers (name TEXT);
CREATE TABLE suppliers (name TEXT);
INSERT INTO customers VALUES ('Alice');
INSERT INTO customers VALUES ('Bob');
INSERT INTO customers VALUES ('Charlie');
INSERT INTO suppliers VALUES ('Bob');
INSERT INTO suppliers VALUES ('Charlie');
INSERT INTO suppliers VALUES ('David');
SELECT name FROM customers UNION SELECT name FROM suppliers;
SELECT name FROM customers UNION ALL SELECT name FROM suppliers;
SELECT name FROM customers INTERSECT SELECT name FROM suppliers;
SELECT name FROM customers EXCEPT SELECT name FROM suppliers;
CREATE VIEW active_users AS SELECT name, age FROM users WHERE status = 'active';
SELECT * FROM active_users;
DROP VIEW active_users;
quit
EOF

echo ""
echo "Running v1.10.0 feature tests..."
OUTPUT=$((sleep 1; cat /tmp/test_v110.sql) | nc 127.0.0.1 5432 2>&1)

# Validate results
echo ""
echo "Validating results..."

# CASE expression
if echo "$OUTPUT" | grep -q "minor" && echo "$OUTPUT" | grep -q "adult" && echo "$OUTPUT" | grep -q "senior"; then
    echo "✓ CASE expressions work"
else
    echo "✗ CASE expressions failed"
    echo "$OUTPUT"
    exit 1
fi

# UNION (should have 4 distinct names: Alice, Bob, Charlie, David)
if echo "$OUTPUT" | grep -q "Alice" && echo "$OUTPUT" | grep -q "David"; then
    echo "✓ UNION works"
else
    echo "✗ UNION failed"
    echo "$OUTPUT"
    exit 1
fi

# INTERSECT (should have 2 names: Bob, Charlie)
if echo "$OUTPUT" | grep -q "Bob" && echo "$OUTPUT" | grep -q "Charlie"; then
    echo "✓ INTERSECT works"
else
    echo "✗ INTERSECT failed"
    echo "$OUTPUT"
    exit 1
fi

# EXCEPT (should have 1 name: Alice)
if echo "$OUTPUT" | grep -q "Alice"; then
    echo "✓ EXCEPT works"
else
    echo "✗ EXCEPT failed"
    echo "$OUTPUT"
    exit 1
fi

# Views (should show Bob and David - active users with age)
if echo "$OUTPUT" | grep -q "Bob" && echo "$OUTPUT" | grep -q "25"; then
    echo "✓ Views work"
else
    echo "✗ Views failed"
    echo "$OUTPUT"
    exit 1
fi

echo ""
echo "========================================="
echo "All v1.10.0 tests passed! ✓"
echo "========================================="
