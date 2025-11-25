#!/bin/bash

set -e

echo "=== Automatic WAL Test with Checkpoint ==="
echo ""

# Clean up
rm -rf data/
mkdir -p data

# Build project
echo "Building project..."
cargo build --release --quiet 2>&1 | grep -v "Compiling" | grep -v "Finished" || true
echo "✓ Build complete"
echo ""

# Start server in background
echo "Starting server..."
cargo run --release 2>&1 >/dev/null &
SERVER_PID=$!
sleep 2
echo "✓ Server started (PID: $SERVER_PID)"
echo ""

# Test 1: Execute 10 INSERT operations (< 100, no checkpoint expected)
echo "=== Test 1: WAL without checkpoint (10 operations) ==="
{
    echo "CREATE TABLE test (id INT, value TEXT);"
    for i in {1..10}; do
        echo "INSERT INTO test VALUES ($i, 'value$i');"
    done
    echo "quit"
} | nc 127.0.0.1 5432 > /dev/null 2>&1

sleep 1

# Check that WAL exists but no snapshot
if [ -d "data/wal" ] && [ "$(ls -A data/wal)" ]; then
    echo "✓ WAL directory has files"
    WAL_SIZE=$(du -sb data/wal | cut -f1)
    echo "  WAL size: $WAL_SIZE bytes"
else
    echo "✗ WAL directory is empty"
    kill $SERVER_PID
    exit 1
fi

if [ -f "data/main.db" ]; then
    echo "✗ Snapshot created too early (expected no snapshot)"
    DB_SIZE=$(stat -c%s "data/main.db")
    echo "  Snapshot size: $DB_SIZE bytes"
else
    echo "✓ No snapshot yet (as expected)"
fi
echo ""

# Test 2: Execute 95 more INSERT operations (total 106, should trigger checkpoint)
echo "=== Test 2: WAL with checkpoint (106 total operations) ==="
{
    for i in {11..106}; do
        echo "INSERT INTO test VALUES ($i, 'value$i');"
    done
    echo "quit"
} | nc 127.0.0.1 5432 > /dev/null 2>&1

sleep 1

# Check that snapshot was created
if [ -f "data/main.db" ]; then
    echo "✓ Checkpoint triggered (snapshot created)"
    DB_SIZE=$(stat -c%s "data/main.db")
    echo "  Snapshot size: $DB_SIZE bytes"
else
    echo "✗ No snapshot found (checkpoint didn't trigger)"
    kill $SERVER_PID
    exit 1
fi
echo ""

# Test 3: Verify data is correct
echo "=== Test 3: Verify data integrity ==="
RESULT=$(printf "SELECT * FROM test WHERE id = 50;\nquit\n" | nc 127.0.0.1 5432 2>&1 | grep -c "50" || true)
if [ "$RESULT" -ge 1 ]; then
    echo "✓ Data retrieved successfully"
else
    echo "✗ Failed to retrieve data"
    kill $SERVER_PID
    exit 1
fi

# Count total rows
TOTAL=$(printf "SELECT * FROM test;\nquit\n" | nc 127.0.0.1 5432 2>&1)
echo "$TOTAL" | tail -20
echo ""

# Stop server
echo "Stopping server for crash recovery test..."
kill $SERVER_PID
wait $SERVER_PID 2>/dev/null || true
sleep 1
echo "✓ Server stopped"
echo ""

# Test 4: Crash recovery
echo "=== Test 4: Crash Recovery Test ==="
echo "Restarting server (should recover from WAL + snapshot)..."
cargo run --release 2>&1 >/dev/null &
SERVER_PID=$!
sleep 2
echo "✓ Server restarted (PID: $SERVER_PID)"
echo ""

# Verify all data is still there
RECOVERED=$(printf "SELECT * FROM test WHERE id = 100;\nquit\n" | nc 127.0.0.1 5432 2>&1 | grep -c "100" || true)
if [ "$RECOVERED" -ge 1 ]; then
    echo "✓ Data recovered successfully after restart"
else
    echo "✗ Failed to recover data"
    kill $SERVER_PID
    exit 1
fi

# Stop server
kill $SERVER_PID
wait $SERVER_PID 2>/dev/null || true
echo "✓ Server stopped"
echo ""

echo "=== All WAL tests passed! ==="
echo ""
echo "Summary:"
echo "- WAL logging: ✓"
echo "- Conditional checkpoint (at 100 ops): ✓"
echo "- Data integrity: ✓"
echo "- Crash recovery: ✓"
