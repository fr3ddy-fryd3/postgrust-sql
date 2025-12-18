#!/bin/bash
# Multi-Connection MVCC Isolation Test (v2.1.0)
#
# Tests that uncommitted changes from one connection are NOT visible to another connection
# This is the core feature of v2.1.0 GlobalTransactionManager

set -e

DATA_DIR="/tmp/rustdb_mvcc_test_$$"
SERVER_PID=""

cleanup() {
    if [ -n "$SERVER_PID" ]; then
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
    fi
    rm -rf "$DATA_DIR"
}

trap cleanup EXIT

echo "=== Multi-Connection MVCC Isolation Test (v2.1.0) ==="
echo

# Start server
echo "Starting server..."
./target/release/postgrustql --init --data-dir "$DATA_DIR" >/dev/null 2>&1 &
SERVER_PID=$!
sleep 2

# Setup: Create table
echo "1. Setup: Creating test table..."
echo -e "CREATE TABLE users (id INTEGER, name TEXT);\nquit" | nc 127.0.0.1 5432 >/dev/null 2>&1
echo "   ✓ Table created"
echo

# Connection 1: BEGIN transaction and INSERT (but don't commit yet)
echo "2. Connection 1: BEGIN + INSERT (uncommitted)..."
(
    echo "BEGIN;"
    echo "INSERT INTO users (id, name) VALUES (1, 'Alice');"
    sleep 5  # Keep connection open WITHOUT sending COMMIT
    # Don't send COMMIT yet!
) | nc 127.0.0.1 5432 >/dev/null 2>&1 &
CONN1_PID=$!

# Wait for Connection 1 to insert data
sleep 2

# Connection 2: SELECT while Connection 1's transaction is still uncommitted
echo "3. Connection 2: SELECT (should NOT see uncommitted row)..."
RESULT=$(echo -e "SELECT * FROM users;\nquit" | nc 127.0.0.1 5432 2>/dev/null | grep -c "Alice" || true)

if [ "$RESULT" -eq 0 ]; then
    echo "   ✓ PASS: Uncommitted row is NOT visible (correct isolation!)"
    echo
    echo "=== ✅ MVCC isolation test PASSED! ==="
    echo
    echo "Summary:"
    echo "  • Uncommitted changes are NOT visible to other connections ✓"
    echo "  • Multi-connection transaction isolation working correctly ✓"

    # Cleanup
    kill $CONN1_PID 2>/dev/null || true
    exit 0
else
    echo "   ✗ FAIL: Uncommitted row IS visible (isolation broken!)"
    kill $CONN1_PID 2>/dev/null || true
    exit 1
fi
