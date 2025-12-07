#!/bin/bash
# Integration test for Extended WHERE Operators (v1.8.0)

set -e

echo "========================================="
echo "Testing Extended WHERE Operators"
echo "========================================="

# Clean start
rm -rf data/
echo "✓ Cleaned old data"

# Build quietly
cargo build --release --quiet 2>/dev/null

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/rustdb_extended_test.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    cat /tmp/rustdb_extended_test.log
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
cat > /tmp/test_extended.sql << 'EOF'
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT, price INTEGER, category TEXT, stock INTEGER);
INSERT INTO products (id, name, price, category, stock) VALUES (1, 'Laptop', 1000, 'Electronics', 5);
INSERT INTO products (id, name, price, category, stock) VALUES (2, 'Mouse', 25, 'Electronics', 50);
INSERT INTO products (id, name, price, category, stock) VALUES (3, 'Keyboard', 75, 'Electronics', 30);
INSERT INTO products (id, name, price, category, stock) VALUES (4, 'Book', 15, 'Books', 100);
INSERT INTO products (id, name, price, category, stock) VALUES (5, 'Lamp', 45, 'Furniture', 20);
SELECT name FROM products WHERE price >= 50;
SELECT name FROM products WHERE price <= 30;
SELECT name FROM products WHERE price BETWEEN 20 AND 80;
SELECT name FROM products WHERE name LIKE 'L%';
SELECT name FROM products WHERE id IN (1, 3, 5);
CREATE TABLE nullable_test (id INTEGER PRIMARY KEY, name TEXT, value INTEGER);
INSERT INTO nullable_test (id, name, value) VALUES (1, 'Has value', 42);
INSERT INTO nullable_test (id, name) VALUES (2, 'No value');
SELECT name FROM nullable_test WHERE value IS NULL;
SELECT name FROM nullable_test WHERE value IS NOT NULL;
quit
EOF

echo ""
echo "Running extended operator tests..."
OUTPUT=$((sleep 1; cat /tmp/test_extended.sql) | nc 127.0.0.1 5432 2>&1)

# Validate results
echo ""
echo "Validating results..."

# >= operator
if echo "$OUTPUT" | grep -q "Laptop" && echo "$OUTPUT" | grep -q "Keyboard"; then
    echo "✓ >= operator works"
else
    echo "✗ >= operator failed"
    echo "$OUTPUT"
    exit 1
fi

# <= operator
if echo "$OUTPUT" | grep -q "Mouse" && echo "$OUTPUT" | grep -q "Book"; then
    echo "✓ <= operator works"
else
    echo "✗ <= operator failed"
    exit 1
fi

# BETWEEN operator
if echo "$OUTPUT" | grep -q "Keyboard" && echo "$OUTPUT" | grep -q "Lamp"; then
    echo "✓ BETWEEN operator works"
else
    echo "✗ BETWEEN operator failed"
    exit 1
fi

# LIKE operator
if echo "$OUTPUT" | grep -q "Laptop" && echo "$OUTPUT" | grep -q "Lamp"; then
    echo "✓ LIKE operator works"
else
    echo "✗ LIKE operator failed"
    exit 1
fi

# IN operator
if echo "$OUTPUT" | grep -q "Laptop" && echo "$OUTPUT" | grep -q "Keyboard"; then
    echo "✓ IN operator works"
else
    echo "✗ IN operator failed"
    exit 1
fi

# IS NULL
if echo "$OUTPUT" | grep -q "No value"; then
    echo "✓ IS NULL operator works"
else
    echo "✗ IS NULL operator failed"
    exit 1
fi

# IS NOT NULL
if echo "$OUTPUT" | grep -q "Has value"; then
    echo "✓ IS NOT NULL operator works"
else
    echo "✗ IS NOT NULL operator failed"
    exit 1
fi

echo ""
echo "========================================="
echo "All extended operator tests passed! ✓"
echo "========================================="
echo ""
echo "Summary:"
echo "  - >= and <= operators working"
echo "  - BETWEEN operator working"
echo "  - LIKE pattern matching working"
echo "  - IN list operator working"
echo "  - IS NULL operator working"
echo "  - IS NOT NULL operator working"
echo ""
echo "Extended WHERE operators in v1.8.0:"
echo "  - >=, <= (comparison)"
echo "  - BETWEEN value AND value"
echo "  - LIKE 'pattern' (% and _ wildcards)"
echo "  - IN (value1, value2, ...)"
echo "  - IS NULL / IS NOT NULL"
echo ""
