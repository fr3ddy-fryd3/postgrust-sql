#!/bin/bash
# Simplified dump/restore test
set -e

echo "=== Simple Dump/Restore Test ==="
echo

# Build
echo "[1] Building..."
cargo build --release --bin postgrustql --bin postgrust-dump --bin postgrust-restore 2>/dev/null
echo "✓ Built"

# Cleanup
TEST_DIR="./data_test_simple"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR/original"

# Start server
echo "[2] Starting server..."
POSTGRUSTQL_DATA_DIR="$TEST_DIR/original" \
POSTGRUSTQL_INITDB=true \
cargo run --release --bin postgrustql > /dev/null 2>&1 &
SERVER_PID=$!
sleep 3
echo "✓ Server started (PID: $SERVER_PID)"

# Create test data
echo "[3] Creating test data..."
printf "CREATE TABLE users (id SERIAL, name TEXT);\n" | nc 127.0.0.1 5432 > /dev/null
printf "INSERT INTO users (name) VALUES ('Alice');\n" | nc 127.0.0.1 5432 > /dev/null
printf "INSERT INTO users (name) VALUES ('Bob');\n" | nc 127.0.0.1 5432 > /dev/null
echo "✓ Data created"

# Stop server
kill $SERVER_PID
wait $SERVER_PID 2>/dev/null || true
echo "✓ Server stopped"

# Dump database
echo "[4] Dumping database..."
./target/release/postgrust-dump \
    --data-dir "$TEST_DIR/original" \
    --output "$TEST_DIR/dump.sql" \
    postgres
echo "✓ Dump created ($(wc -l < $TEST_DIR/dump.sql) lines)"
echo

# Show dump content
echo "--- Dump content ---"
cat "$TEST_DIR/dump.sql"
echo "---"
echo

# Restore database
echo "[5] Restoring database..."
mkdir -p "$TEST_DIR/restored"
./target/release/postgrust-restore \
    --data-dir "$TEST_DIR/restored" \
    --input "$TEST_DIR/dump.sql" \
    postgres_restored
echo "✓ Restore completed"
echo

# Verify
echo "[6] Starting server for verification..."
POSTGRUSTQL_DATA_DIR="$TEST_DIR/restored" \
POSTGRUSTQL_INITDB=false \
cargo run --release --bin postgrustql > /dev/null 2>&1 &
SERVER_PID=$!
sleep 3

echo "Querying restored data..."
printf "SELECT * FROM users;\nquit\n" | nc 127.0.0.1 5432

kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true
echo

# Cleanup
rm -rf "$TEST_DIR"
echo "✓ Test completed!"
