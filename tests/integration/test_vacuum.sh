#!/bin/bash
# Integration test for VACUUM command

echo "=== VACUUM Integration Test ==="

# Build
cargo build --release --quiet 2>/dev/null

# Clean start
rm -rf data
mkdir -p data

# Start server
timeout 20 cargo run --release &>/dev/null &
SERVER_PID=$!
sleep 2

# Test 1: VACUUM on table with dead tuples
echo "Test 1: VACUUM removes dead tuples after UPDATE"
printf "CREATE TABLE users (id INTEGER, name TEXT);\n" | nc -q 1 127.0.0.1 5432

printf "INSERT INTO users VALUES (1, 'Alice');\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO users VALUES (2, 'Bob');\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO users VALUES (3, 'Charlie');\n" | nc -q 1 127.0.0.1 5432

# UPDATE creates dead tuples (old versions)
printf "UPDATE users SET name = 'Alice Updated' WHERE id = 1;\n" | nc -q 1 127.0.0.1 5432
printf "UPDATE users SET name = 'Bob Updated' WHERE id = 2;\n" | nc -q 1 127.0.0.1 5432

# Now we have: 3 alive + 2 dead tuples
# VACUUM should remove the 2 dead tuples

printf "VACUUM users;\n" | nc -q 1 127.0.0.1 5432 | grep -i "vacuum complete"
if [ $? -eq 0 ]; then
    echo "✓ VACUUM executed successfully"
else
    echo "✗ VACUUM failed"
fi

# Verify data still accessible
RESULT=$(printf "SELECT * FROM users;\n" | nc -q 1 127.0.0.1 5432 | grep -c "Alice Updated")
if [ "$RESULT" -eq 1 ]; then
    echo "✓ Data integrity maintained after VACUUM"
else
    echo "✗ Data corrupted after VACUUM"
fi

# Test 2: VACUUM after DELETE
echo ""
echo "Test 2: VACUUM after DELETE"
printf "CREATE TABLE products (id INTEGER, name TEXT);\n" | nc -q 1 127.0.0.1 5432

for i in {1..10}; do
    printf "INSERT INTO products VALUES ($i, 'Product$i');\n" | nc -q 1 127.0.0.1 5432
done

# Delete half the rows (creates dead tuples)
printf "DELETE FROM products WHERE id <= 5;\n" | nc -q 1 127.0.0.1 5432

printf "VACUUM products;\n" | nc -q 1 127.0.0.1 5432 | grep -i "removed"
if [ $? -eq 0 ]; then
    echo "✓ VACUUM after DELETE works"
else
    echo "✗ VACUUM after DELETE failed"
fi

# Verify only alive rows remain
REMAINING=$(printf "SELECT COUNT(*) FROM products;\n" | nc -q 1 127.0.0.1 5432 | grep -o "[0-9]\+" | head -1)
if [ "$REMAINING" -eq 5 ]; then
    echo "✓ Correct row count after DELETE + VACUUM"
else
    echo "✗ Incorrect row count: $REMAINING (expected 5)"
fi

# Test 3: VACUUM all tables
echo ""
echo "Test 3: VACUUM without table name (all tables)"
printf "VACUUM;\n" | nc -q 1 127.0.0.1 5432 | grep -i "vacuum complete"
if [ $? -eq 0 ]; then
    echo "✓ VACUUM (all tables) works"
else
    echo "✗ VACUUM (all tables) failed"
fi

# Test 4: VACUUM on non-existent table
echo ""
echo "Test 4: VACUUM on non-existent table"
OUTPUT=$(printf "VACUUM nonexistent;\n" | nc -q 1 127.0.0.1 5432)
if echo "$OUTPUT" | grep -qi "not found\|error"; then
    echo "✓ VACUUM handles non-existent table correctly"
else
    echo "✗ VACUUM should error on non-existent table"
fi

# Cleanup
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null
rm -rf data

echo ""
echo "=== VACUUM Integration Test Complete ==="
