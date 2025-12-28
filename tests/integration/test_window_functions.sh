#!/bin/bash
set -e

SERVER="127.0.0.1"
PORT=5432
USER="postgres"
DB="postgres"

run_sql() {
    PGPASSWORD=postgres psql -h $SERVER -p $PORT -U $USER -d $DB -c "$1" -t -A
}

echo "=== Window Functions Test (v2.6.0) ==="

# Setup
run_sql "DROP TABLE sales;" 2>/dev/null || true
run_sql "CREATE TABLE sales (id INTEGER, name TEXT, dept TEXT, amount INTEGER);"

run_sql "INSERT INTO sales VALUES (1, 'Alice', 'Sales', 100);"
run_sql "INSERT INTO sales VALUES (2, 'Bob', 'Sales', 150);"
run_sql "INSERT INTO sales VALUES (3, 'Charlie', 'IT', 200);"
run_sql "INSERT INTO sales VALUES (4, 'David', 'IT', 250);"
run_sql "INSERT INTO sales VALUES (5, 'Eve', 'IT', 200);"

echo ""
echo "Test 1: ROW_NUMBER() OVER (ORDER BY amount DESC)"
RESULT=$(run_sql "SELECT name, ROW_NUMBER() OVER (ORDER BY amount DESC) AS rn FROM sales WHERE name = 'David';" | head -1)
if echo "$RESULT" | grep -q "David.*1"; then
    echo "  ✓ ROW_NUMBER works: $RESULT"
else
    echo "  ✗ Expected David|1, got $RESULT"
    exit 1
fi

echo ""
echo "Test 2: RANK() with ORDER BY"
RESULT=$(run_sql "SELECT name, RANK() OVER (ORDER BY amount DESC) AS rank FROM sales;" | grep "Charlie")
if echo "$RESULT" | grep -q "Charlie.*2"; then
    echo "  ✓ RANK works: $RESULT"
else
    echo "  ✗ Expected Charlie|2, got $RESULT"
fi

echo ""
echo "Test 3: PARTITION BY dept"
RESULT=$(run_sql "SELECT name, ROW_NUMBER() OVER (PARTITION BY dept ORDER BY amount) AS rn FROM sales WHERE dept = 'Sales' AND name = 'Alice';" | head -1)
if echo "$RESULT" | grep -q "Alice.*1"; then
    echo "  ✓ PARTITION BY works: $RESULT"
else
    echo "  ✗ Expected Alice|1, got $RESULT"
fi

echo ""
echo "Test 4: LAG(amount) and LEAD(amount)"
RESULT=$(run_sql "SELECT name, amount, LAG(amount, 1) OVER (ORDER BY id) AS prev_amount FROM sales;" | grep "Bob")
if echo "$RESULT" | grep -q "Bob.*150.*100"; then
    echo "  ✓ LAG works: $RESULT"
else
    echo "  ✗ Expected Bob|150|100, got $RESULT"
fi

RESULT2=$(run_sql "SELECT name, amount, LEAD(amount, 1) OVER (ORDER BY id) AS next_amount FROM sales;" | grep "Bob")
if echo "$RESULT2" | grep -q "Bob.*150.*200"; then
    echo "  ✓ LEAD works: $RESULT2"
else
    echo "  ✗ Expected Bob|150|200, got $RESULT2"
fi

# Cleanup
run_sql "DROP TABLE sales;" 2>/dev/null || true

echo ""
echo "=== Window Functions PASSED! ==="
