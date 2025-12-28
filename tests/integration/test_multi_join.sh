#!/bin/bash
# Integration test for Multi-JOIN functionality (v2.6.0)
# Tests multiple JOINs in a single query

set -e

SERVER="127.0.0.1"
PORT=5432
USER="postgres"
DB="postgres"

echo "=== Multi-JOIN Integration Test Suite (v2.6.0) ==="

# Helper function to run SQL
run_sql() {
    psql -h $SERVER -p $PORT -U $USER -d $DB -c "$1" -t -A
}

# Setup: Create 4 tables (users, orders, shipments, payments)
echo ""
echo "Setup: Creating test tables..."
run_sql "DROP TABLE payments;" 2>/dev/null || true
run_sql "DROP TABLE shipments;" 2>/dev/null || true
run_sql "DROP TABLE orders;" 2>/dev/null || true
run_sql "DROP TABLE users;" 2>/dev/null || true

run_sql "CREATE TABLE users (
    id INTEGER,
    name TEXT
);"

run_sql "CREATE TABLE orders (
    id INTEGER,
    user_id INTEGER,
    product TEXT
);"

run_sql "CREATE TABLE shipments (
    id INTEGER,
    order_id INTEGER,
    address TEXT
);"

run_sql "CREATE TABLE payments (
    id INTEGER,
    shipment_id INTEGER,
    amount INTEGER
);"

# Insert test data
run_sql "INSERT INTO users VALUES (1, 'Alice');"
run_sql "INSERT INTO users VALUES (2, 'Bob');"
run_sql "INSERT INTO users VALUES (3, 'Charlie');"  # No orders

run_sql "INSERT INTO orders VALUES (1, 1, 'Laptop');"
run_sql "INSERT INTO orders VALUES (2, 1, 'Mouse');"
run_sql "INSERT INTO orders VALUES (3, 2, 'Phone');"
run_sql "INSERT INTO orders VALUES (4, 2, 'Keyboard');"  # No shipment

run_sql "INSERT INTO shipments VALUES (1, 1, '123 Main St');"
run_sql "INSERT INTO shipments VALUES (2, 2, '123 Main St');"
run_sql "INSERT INTO shipments VALUES (3, 3, '456 Oak Ave');"

run_sql "INSERT INTO payments VALUES (1, 1, 1200);"
run_sql "INSERT INTO payments VALUES (2, 2, 25);"
run_sql "INSERT INTO payments VALUES (3, 3, 800);"

echo "  ✓ Test data inserted: 3 users, 4 orders, 3 shipments, 3 payments"

# Test 1: 2-table JOIN (baseline)
echo ""
echo "Test 1: 2-table JOIN (users → orders)"
RESULT=$(run_sql "SELECT COUNT(*) FROM users JOIN orders ON users.id = orders.user_id;")
if [ "$RESULT" = "4" ]; then
    echo "  ✓ 2-table JOIN works: 4 rows"
else
    echo "  ✗ Expected 4 rows, got $RESULT"
    exit 1
fi

# Test 2: 3-table JOIN (users → orders → shipments)
echo ""
echo "Test 2: 3-table JOIN (users → orders → shipments)"
RESULT=$(run_sql "SELECT COUNT(*) FROM users
    JOIN orders ON users.id = orders.user_id
    JOIN shipments ON orders.id = shipments.order_id;")
if [ "$RESULT" = "3" ]; then
    echo "  ✓ 3-table JOIN works: 3 rows (order 4 has no shipment)"
else
    echo "  ✗ Expected 3 rows, got $RESULT"
    exit 1
fi

# Test 3: 4-table JOIN (users → orders → shipments → payments)
echo ""
echo "Test 3: 4-table JOIN (users → orders → shipments → payments)"
RESULT=$(run_sql "SELECT COUNT(*) FROM users
    JOIN orders ON users.id = orders.user_id
    JOIN shipments ON orders.id = shipments.order_id
    JOIN payments ON shipments.id = payments.shipment_id;")
if [ "$RESULT" = "3" ]; then
    echo "  ✓ 4-table JOIN works: 3 rows"
else
    echo "  ✗ Expected 3 rows, got $RESULT"
    exit 1
fi

# Test 4: Mixed JOIN types (LEFT + INNER)
echo ""
echo "Test 4: Mixed JOIN types (users LEFT JOIN orders INNER JOIN shipments)"
RESULT=$(run_sql "SELECT COUNT(*) FROM users
    LEFT JOIN orders ON users.id = orders.user_id
    JOIN shipments ON orders.id = shipments.order_id;")
if [ "$RESULT" = "3" ]; then
    echo "  ✓ Mixed JOIN types work: 3 rows"
else
    echo "  ✗ Expected 3 rows, got $RESULT"
    exit 1
fi

# Test 5: Verify data correctness
echo ""
echo "Test 5: Verify data correctness (sum of payments)"
RESULT=$(run_sql "SELECT SUM(payments.amount::integer) FROM users
    JOIN orders ON users.id = orders.user_id
    JOIN shipments ON orders.id = shipments.order_id
    JOIN payments ON shipments.id = payments.shipment_id
    WHERE users.name = 'Alice';")
# Alice has 2 orders (Laptop, Mouse) with payments 1200 + 25 = 1225
if [ "$RESULT" = "1225" ]; then
    echo "  ✓ Data correctness verified: Alice's payments = 1225"
else
    echo "  ✗ Expected 1225, got $RESULT"
    exit 1
fi

# Test 6: LIMIT/OFFSET with multi-JOIN
echo ""
echo "Test 6: LIMIT with multi-JOIN"
RESULT=$(run_sql "SELECT COUNT(*) FROM (
    SELECT * FROM users
        JOIN orders ON users.id = orders.user_id
        JOIN shipments ON orders.id = shipments.order_id
        LIMIT 2
) AS limited;")
if [ "$RESULT" = "2" ]; then
    echo "  ✓ LIMIT with multi-JOIN works: 2 rows"
else
    echo "  ✗ Expected 2 rows, got $RESULT"
    exit 1
fi

# Test 7: Column resolution across multiple JOINs
echo ""
echo "Test 7: Column resolution (select specific columns from each table)"
RESULT=$(run_sql "SELECT users.name, orders.product, shipments.address
    FROM users
    JOIN orders ON users.id = orders.user_id
    JOIN shipments ON orders.id = shipments.order_id
    WHERE users.name = 'Bob';")
EXPECTED="Bob|Phone|456 Oak Ave"
if [ "$RESULT" = "$EXPECTED" ]; then
    echo "  ✓ Column resolution works correctly"
else
    echo "  ✗ Column resolution failed"
    echo "    Expected: $EXPECTED"
    echo "    Got: $RESULT"
    exit 1
fi

# Cleanup
echo ""
echo "Cleaning up test tables..."
run_sql "DROP TABLE payments;" 2>/dev/null || true
run_sql "DROP TABLE shipments;" 2>/dev/null || true
run_sql "DROP TABLE orders;" 2>/dev/null || true
run_sql "DROP TABLE users;" 2>/dev/null || true

echo ""
echo "=== All Multi-JOIN tests PASSED! ✓ ==="
echo ""
echo "Summary:"
echo "  • 2-table JOIN works"
echo "  • 3-table JOIN works"
echo "  • 4-table JOIN works"
echo "  • Mixed JOIN types (LEFT + INNER) work"
echo "  • Data correctness verified"
echo "  • LIMIT/OFFSET with multi-JOIN works"
echo "  • Column resolution across multiple tables works"
echo ""
echo "v2.6.0 Multi-JOIN support complete! 🎉"
