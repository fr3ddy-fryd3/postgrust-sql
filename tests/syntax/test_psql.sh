#!/bin/bash

echo "=== Testing RustDB with psql ==="
echo ""

# Start server in background
echo "Starting RustDB server..."
cargo run --release > /tmp/psql_test.log 2>&1 &
SERVER_PID=$!
sleep 3

echo "✓ Server started"
echo ""

# Test 1: SHOW TABLES
echo "Test 1: SHOW TABLES"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main -c "SHOW TABLES;"
echo ""

# Test 2: CREATE TABLE
echo "Test 2: CREATE TABLE"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main -c "CREATE TABLE demo (id INT, name TEXT);"
echo ""

# Test 3: INSERT
echo "Test 3: INSERT data"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main <<EOF
INSERT INTO demo VALUES (1, 'Alice');
INSERT INTO demo VALUES (2, 'Bob');
INSERT INTO demo VALUES (3, 'Charlie');
EOF
echo ""

# Test 4: SELECT
echo "Test 4: SELECT data"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main -c "SELECT * FROM demo;"
echo ""

# Test 5: Transaction
echo "Test 5: Transaction (BEGIN/COMMIT)"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main <<EOF
BEGIN;
INSERT INTO demo VALUES (4, 'Dave');
SELECT * FROM demo WHERE id = 4;
COMMIT;
EOF
echo ""

# Test 6: UPDATE
echo "Test 6: UPDATE"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main -c "UPDATE demo SET name = 'David' WHERE id = 4;"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main -c "SELECT * FROM demo WHERE id = 4;"
echo ""

# Test 7: DELETE
echo "Test 7: DELETE"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main -c "DELETE FROM demo WHERE id = 1;"
psql -h 127.0.0.1 -p 5432 -U postgrust -d main -c "SELECT * FROM demo;"
echo ""

echo "=== All tests completed! ==="
echo ""
echo "Stopping server..."
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null || true

echo "✓ Done"
