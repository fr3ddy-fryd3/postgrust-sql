#!/bin/bash
# Integration test for B-tree indexes (v1.6.0)

set -e

echo "========================================="
echo "Testing B-tree Index Implementation"
echo "========================================="

# Clean start
rm -rf data/
echo "✓ Cleaned old data"

# Build quietly
cargo build --release --quiet 2>/dev/null

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/rustdb_index_test.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    cat /tmp/rustdb_index_test.log
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

# Function to run SQL
run_sql() {
    printf "%s\n" "$1" | nc 127.0.0.1 5432 2>&1
}

echo ""
echo "1. Create test table with data"
run_sql "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);" > /dev/null
run_sql "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30);" > /dev/null
run_sql "INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25);" > /dev/null
run_sql "INSERT INTO users (id, name, age) VALUES (3, 'Charlie', 35);" > /dev/null
run_sql "INSERT INTO users (id, name, age) VALUES (4, 'David', 28);" > /dev/null
run_sql "INSERT INTO users (id, name, age) VALUES (5, 'Eve', 32);" > /dev/null
echo "✓ Table and data created"

echo ""
echo "2. Create B-tree index on age column"
RESULT=$(run_sql "CREATE INDEX idx_age ON users(age);" 2>&1)
if echo "$RESULT" | grep -qi "idx_age.*created\|created.*idx_age"; then
    echo "✓ CREATE INDEX succeeded"
else
    echo "✗ CREATE INDEX failed: $RESULT"
    exit 1
fi

echo ""
echo "3. Create UNIQUE index on name column"
RESULT=$(run_sql "CREATE UNIQUE INDEX idx_name ON users(name);" 2>&1)
if echo "$RESULT" | grep -qi "idx_name.*created\|created.*idx_name"; then
    echo "✓ CREATE UNIQUE INDEX succeeded"
else
    echo "✗ CREATE UNIQUE INDEX failed: $RESULT"
    exit 1
fi

echo ""
echo "4. Test duplicate index creation (should fail)"
RESULT=$(run_sql "CREATE INDEX idx_age ON users(age);" 2>&1)
if echo "$RESULT" | grep -q "already exists"; then
    echo "✓ Duplicate index creation correctly failed"
else
    echo "✗ Expected error for duplicate index: $RESULT"
    exit 1
fi

echo ""
echo "5. Drop index"
RESULT=$(run_sql "DROP INDEX idx_age;" 2>&1)
if echo "$RESULT" | grep -qi "idx_age.*dropped\|dropped.*idx_age"; then
    echo "✓ DROP INDEX succeeded"
else
    echo "✗ DROP INDEX failed: $RESULT"
    exit 1
fi

echo ""
echo "6. Drop non-existent index (should fail)"
RESULT=$(run_sql "DROP INDEX idx_nonexistent;" 2>&1)
if echo "$RESULT" | grep -q "does not exist"; then
    echo "✓ Drop non-existent index correctly failed"
else
    echo "✗ Expected error for non-existent index: $RESULT"
    exit 1
fi

echo ""
echo "========================================="
echo "All index tests passed! ✓"
echo "========================================="
echo ""
echo "Summary:"
echo "  - B-tree index creation working"
echo "  - UNIQUE index creation working"
echo "  - Index validation working"
echo "  - DROP INDEX working"
echo ""
echo "Next steps for v1.6.0:"
echo "  - Integrate indexes with SELECT query planner"
echo "  - Add index usage statistics"
echo "  - Implement EXPLAIN to show index usage"
