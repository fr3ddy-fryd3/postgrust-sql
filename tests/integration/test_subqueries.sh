#!/bin/bash
# Integration test for Subqueries (v2.6.0)
# Tests scalar subqueries, IN/EXISTS subqueries

set -e

SERVER="127.0.0.1"
PORT=5432
USER="postgres"
DB="postgres"

echo "=== Subquery Integration Test Suite (v2.6.0) ==="

# Helper function to run SQL
run_sql() {
    PGPASSWORD=postgres psql -h $SERVER -p $PORT -U $USER -d $DB -c "$1" -t -A
}

# Setup: Create test tables
echo ""
echo "Setup: Creating test tables..."
run_sql "DROP TABLE orders;" 2>/dev/null || true
run_sql "DROP TABLE products;" 2>/dev/null || true
run_sql "DROP TABLE users;" 2>/dev/null || true

run_sql "CREATE TABLE users (
    id INTEGER,
    name TEXT,
    age INTEGER
);"

run_sql "CREATE TABLE orders (
    id INTEGER,
    user_id INTEGER,
    product_id INTEGER,
    amount INTEGER,
    status TEXT
);"

run_sql "CREATE TABLE products (
    id INTEGER,
    name TEXT,
    price INTEGER
);"

# Insert test data
run_sql "INSERT INTO users VALUES (1, 'Alice', 30);"
run_sql "INSERT INTO users VALUES (2, 'Bob', 25);"
run_sql "INSERT INTO users VALUES (3, 'Charlie', 35);"
run_sql "INSERT INTO users VALUES (4, 'David', 28);"

run_sql "INSERT INTO orders VALUES (1, 1, 1, 100, 'completed');"
run_sql "INSERT INTO orders VALUES (2, 1, 2, 200, 'completed');"
run_sql "INSERT INTO orders VALUES (3, 2, 1, 150, 'pending');"
run_sql "INSERT INTO orders VALUES (4, 3, 3, 300, 'completed');"

run_sql "INSERT INTO products VALUES (1, 'Laptop', 1000);"
run_sql "INSERT INTO products VALUES (2, 'Mouse', 25);"
run_sql "INSERT INTO products VALUES (3, 'Keyboard', 75);"

echo "  ✓ Test data inserted: 4 users, 4 orders, 3 products"

# Test 1: IN subquery
echo ""
echo "Test 1: IN subquery - users who have orders"
RESULT=$(run_sql "SELECT COUNT(*) FROM users WHERE id IN (SELECT user_id FROM orders);")
if [ "$RESULT" = "3" ]; then
    echo "  ✓ IN subquery works: 3 users have orders"
else
    echo "  ✗ Expected 3 users, got $RESULT"
    exit 1
fi

# Test 2: NOT IN subquery
echo ""
echo "Test 2: NOT IN subquery - users without orders"
RESULT=$(run_sql "SELECT COUNT(*) FROM users WHERE id NOT IN (SELECT user_id FROM orders);")
if [ "$RESULT" = "1" ]; then
    echo "  ✓ NOT IN subquery works: 1 user (David) has no orders"
else
    echo "  ✗ Expected 1 user, got $RESULT"
    exit 1
fi

# Test 3: EXISTS subquery
echo ""
echo "Test 3: EXISTS subquery - check if any completed orders exist"
RESULT=$(run_sql "SELECT COUNT(*) FROM users WHERE EXISTS (SELECT 1 FROM orders WHERE status = 'completed');")
if [ "$RESULT" = "4" ]; then
    echo "  ✓ EXISTS subquery works: all users (completed orders exist)"
else
    echo "  ✗ Expected 4 users, got $RESULT"
    exit 1
fi

# Test 4: NOT EXISTS subquery
echo ""
echo "Test 4: NOT EXISTS subquery - check if no cancelled orders exist"
RESULT=$(run_sql "SELECT COUNT(*) FROM users WHERE NOT EXISTS (SELECT 1 FROM orders WHERE status = 'cancelled');")
if [ "$RESULT" = "4" ]; then
    echo "  ✓ NOT EXISTS subquery works: all users (no cancelled orders)"
else
    echo "  ✗ Expected 4 users, got $RESULT"
    exit 1
fi

# Test 5: Scalar subquery in WHERE (equals)
echo ""
echo "Test 5: Scalar subquery - users older than average age"
RESULT=$(run_sql "SELECT COUNT(*) FROM users WHERE age > (SELECT AVG(age::integer) FROM users);")
if [ "$RESULT" = "2" ]; then
    echo "  ✓ Scalar subquery works: 2 users above average age"
else
    echo "  ✗ Expected 2 users, got $RESULT"
    exit 1
fi

# Test 6: Scalar subquery in SELECT list
echo ""
echo "Test 6: Scalar subquery in SELECT list"
RESULT=$(run_sql "SELECT name, (SELECT COUNT(*) FROM orders) AS total_orders FROM users WHERE name = 'Alice';")
EXPECTED="Alice|4"
if [ "$RESULT" = "$EXPECTED" ]; then
    echo "  ✓ Scalar subquery in SELECT works: total_orders = 4"
else
    echo "  ✗ Scalar subquery in SELECT failed"
    echo "    Expected: $EXPECTED"
    echo "    Got: $RESULT"
    exit 1
fi

# Test 7: Complex nested subquery
echo ""
echo "Test 7: Complex nested - orders for products with above-average price"
RESULT=$(run_sql "SELECT COUNT(*) FROM orders WHERE product_id IN (SELECT id FROM products WHERE price > (SELECT AVG(price::integer) FROM products));")
if [ "$RESULT" = "3" ]; then
    echo "  ✓ Nested subqueries work: 3 orders for expensive products"
else
    echo "  ✗ Expected 3 orders, got $RESULT"
    exit 1
fi

# Cleanup
echo ""
echo "Cleaning up test tables..."
run_sql "DROP TABLE orders;" 2>/dev/null || true
run_sql "DROP TABLE products;" 2>/dev/null || true
run_sql "DROP TABLE users;" 2>/dev/null || true

echo ""
echo "=== All Subquery tests PASSED! ✓ ===\"
echo ""
echo "Summary:"
echo "  • IN subquery works"
echo "  • NOT IN subquery works"
echo "  • EXISTS subquery works"
echo "  • NOT EXISTS subquery works"
echo "  • Scalar subquery in WHERE works"
echo "  • Scalar subquery in SELECT works"
echo "  • Nested subqueries work"
echo ""
echo "v2.6.0 Subquery support complete! 🎉"
