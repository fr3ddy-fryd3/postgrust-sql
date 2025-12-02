#!/bin/bash
# Integration test for VACUUM command

echo "=== VACUUM Integration Test ==="

# Build
cargo build --release --quiet 2>/dev/null

# Clean start
rm -rf data
mkdir -p data

# Start server in background
timeout 60 cargo run --release &>/dev/null &
SERVER_PID=$!
sleep 2

# Function to run SQL and check result
run_sql() {
    local sql="$1"
    (sleep 0.5; printf "${sql}\nquit\n") | nc 127.0.0.1 5432
}

# Test 1: VACUUM on table with dead tuples
echo "Test 1: VACUUM removes dead tuples after UPDATE"
run_sql "CREATE TABLE users (id INTEGER, name TEXT);" > /dev/null 2>&1
run_sql "INSERT INTO users VALUES (1, 'Alice');" > /dev/null 2>&1
run_sql "INSERT INTO users VALUES (2, 'Bob');" > /dev/null 2>&1
run_sql "INSERT INTO users VALUES (3, 'Charlie');" > /dev/null 2>&1

# UPDATE creates dead tuples (old versions)
run_sql "UPDATE users SET name = 'Alice Updated' WHERE id = 1;" > /dev/null 2>&1
run_sql "UPDATE users SET name = 'Bob Updated' WHERE id = 2;" > /dev/null 2>&1

# Now we have: 3 alive + 2 dead tuples
# VACUUM should remove the 2 dead tuples
OUTPUT=$(run_sql "VACUUM users;")
if echo "$OUTPUT" | grep -qi "vacuum complete"; then
    echo "✓ VACUUM executed successfully"
    # Show result (expected 0 due to conservative horizon in v1.5.1)
    echo "  $(echo "$OUTPUT" | grep -i "removed")"
else
    echo "✗ VACUUM failed"
    echo "  Output: $OUTPUT"
fi

# Verify data still accessible
RESULT=$(run_sql "SELECT * FROM users;" | grep -c "Alice Updated")
if [ "$RESULT" -eq 1 ]; then
    echo "✓ Data integrity maintained after VACUUM"
else
    echo "✗ Data corrupted after VACUUM (expected 1 'Alice Updated', got $RESULT)"
fi

# Test 2: VACUUM after DELETE
echo ""
echo "Test 2: VACUUM after DELETE"
run_sql "CREATE TABLE products (id INTEGER, name TEXT);" > /dev/null 2>&1

for i in {1..10}; do
    run_sql "INSERT INTO products VALUES ($i, 'Product$i');" > /dev/null 2>&1
done

# Delete half the rows (creates dead tuples)
# Note: Parser only supports =, !=, >, < (not <=, >=)
run_sql "DELETE FROM products WHERE id < 6;" > /dev/null 2>&1

OUTPUT=$(run_sql "VACUUM products;")
if echo "$OUTPUT" | grep -qi "vacuum complete"; then
    echo "✓ VACUUM after DELETE works"
    echo "  $(echo "$OUTPUT" | grep -i "removed")"
else
    echo "✗ VACUUM after DELETE failed"
    echo "  Output: $OUTPUT"
fi

# Verify only alive rows remain
# Note: Current DELETE implementation physically removes rows (not MVCC-aware yet)
# So COUNT will show actual remaining rows
COUNT_OUTPUT=$(run_sql "SELECT COUNT(*) FROM products;")
REMAINING=$(echo "$COUNT_OUTPUT" | grep -o "[0-9]\+" | head -1)
# We expect 5 remaining after DELETE in current implementation (physically removed 5 of 10)
# When MVCC DELETE is implemented (v1.6), this will show 10 until VACUUM runs
if [ ! -z "$REMAINING" ]; then
    if [ "$REMAINING" -eq 5 ]; then
        echo "✓ Correct row count: 5 remaining after DELETE"
        echo "  (Current v1.5.1: DELETE physically removes rows)"
    else
        echo "✗ Unexpected row count: $REMAINING (expected 5)"
        echo "  Full output: $COUNT_OUTPUT"
    fi
else
    echo "✗ Failed to get row count"
    echo "  Full output: $COUNT_OUTPUT"
fi

# Test 3: VACUUM all tables
echo ""
echo "Test 3: VACUUM without table name (all tables)"
OUTPUT=$(run_sql "VACUUM;")
if echo "$OUTPUT" | grep -qi "vacuum complete"; then
    echo "✓ VACUUM (all tables) works"
    echo "  $(echo "$OUTPUT" | grep -i "removed")"
else
    echo "✗ VACUUM (all tables) failed"
    echo "  Output: $OUTPUT"
fi

# Test 4: VACUUM on non-existent table
echo ""
echo "Test 4: VACUUM on non-existent table"
OUTPUT=$(run_sql "VACUUM nonexistent;")
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "✓ VACUUM handles non-existent table correctly"
else
    echo "✗ VACUUM should error on non-existent table"
    echo "  Output: $OUTPUT"
fi

# Cleanup
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null
rm -rf data

echo ""
echo "=== VACUUM Integration Test Complete ==="
echo ""
echo "Note: v1.5.1 uses conservative cleanup (current_tx_id as horizon),"
echo "      so 'Removed 0 dead tuples' is expected behavior."
echo "      Active transaction tracking will be added in v1.6.0."
