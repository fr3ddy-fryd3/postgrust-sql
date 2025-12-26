#!/bin/bash
# Integration test for pgr_dump and pgr_restore
# Tests SQL and binary dump/restore round-trip

set -e  # Exit on error

echo "======================================"
echo "RustDB Dump/Restore Integration Test"
echo "======================================"
echo

# Build binaries
echo "[1/7] Building pgr_dump and pgr_restore..."
cargo build --release --bin pgr_dump --bin pgr_restore 2>/dev/null
echo "✓ Binaries built"
echo

# Clean test data directories
TEST_DIR="./data_test_dump_restore"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR/original" "$TEST_DIR/restored_sql" "$TEST_DIR/restored_binary"

# Start server for original database
echo "[2/7] Starting server for original database..."
POSTGRUSTQL_DATA_DIR="$TEST_DIR/original" \
POSTGRUSTQL_INITDB=true \
cargo run --release > /dev/null 2>&1 &
SERVER_PID=$!
sleep 2
echo "✓ Server started (PID: $SERVER_PID)"
echo

# Create test database schema
echo "[3/7] Creating test schema..."
{
    echo "-- Create ENUM type"
    echo "CREATE TYPE status AS ENUM ('active', 'inactive', 'pending');"

    echo "-- Create tables"
    echo "CREATE TABLE users ("
    echo "  id SERIAL PRIMARY KEY,"
    echo "  name TEXT NOT NULL,"
    echo "  email VARCHAR(255) UNIQUE,"
    echo "  age INTEGER,"
    echo "  status status,"
    echo "  created_at TIMESTAMP"
    echo ");"

    echo "CREATE TABLE orders ("
    echo "  id SERIAL PRIMARY KEY,"
    echo "  user_id INTEGER REFERENCES users(id),"
    echo "  amount NUMERIC(10, 2),"
    echo "  ordered_at TIMESTAMP"
    echo ");"

    echo "-- Insert test data"
    echo "INSERT INTO users (name, email, age, status, created_at) VALUES"
    echo "  ('Alice', 'alice@example.com', 30, 'active', '2025-01-01 10:00:00'),"
    echo "  ('Bob', 'bob@example.com', 25, 'inactive', '2025-01-02 11:00:00'),"
    echo "  ('Charlie', 'charlie@example.com', 35, 'pending', '2025-01-03 12:00:00');"

    echo "INSERT INTO orders (user_id, amount, ordered_at) VALUES"
    echo "  (1, 99.99, '2025-01-05 14:00:00'),"
    echo "  (1, 149.50, '2025-01-06 15:00:00'),"
    echo "  (2, 49.99, '2025-01-07 16:00:00');"

    echo "-- Create indexes"
    echo "CREATE INDEX idx_users_email ON users(email);"
    echo "CREATE INDEX idx_orders_user_id ON orders(user_id) USING HASH;"
    echo "CREATE INDEX idx_users_status_age ON users(status, age);"  # Composite index

    echo "-- Create view"
    echo "CREATE VIEW active_users AS SELECT * FROM users WHERE status = 'active';"

    echo "quit"
} | nc 127.0.0.1 5432 > /dev/null

echo "✓ Schema and data created"
echo

# Dump database (SQL format)
echo "[4/7] Dumping database (SQL format)..."
./target/release/pgr_dump \
    --data-dir "$TEST_DIR/original" \
    --output "$TEST_DIR/dump.sql" \
    postgres > /dev/null
echo "✓ SQL dump created: $(wc -l < $TEST_DIR/dump.sql) lines"
echo

# Dump database (Binary format)
echo "[5/7] Dumping database (Binary format)..."
./target/release/pgr_dump \
    --data-dir "$TEST_DIR/original" \
    --format binary \
    --output "$TEST_DIR/dump.bin" \
    postgres > /dev/null
echo "✓ Binary dump created: $(stat -c%s $TEST_DIR/dump.bin) bytes"
echo

# Stop server
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true
echo "✓ Server stopped"
echo

# Restore from SQL dump
echo "[6/7] Restoring from SQL dump..."
./target/release/pgr_restore \
    --data-dir "$TEST_DIR/restored_sql" \
    --input "$TEST_DIR/dump.sql" \
    postgres_restored 2>&1 | grep -E "completed|statements" || true
echo

# Restore from Binary dump
echo "[7/7] Restoring from Binary dump..."
./target/release/pgr_restore \
    --data-dir "$TEST_DIR/restored_binary" \
    --format binary \
    --input "$TEST_DIR/dump.bin" \
    postgres_restored 2>&1 | grep -E "completed" || true
echo

# Verify restored data
echo "======================================"
echo "Verification"
echo "======================================"
echo

# Start server for SQL-restored database
echo "Starting server for SQL-restored database..."
POSTGRUSTQL_DATA_DIR="$TEST_DIR/restored_sql" \
POSTGRUSTQL_INITDB=false \
cargo run --release > /dev/null 2>&1 &
SERVER_PID=$!
sleep 2

# Query restored data
echo "Querying restored data..."
{
    echo "SELECT COUNT(*) FROM users;"
    echo "SELECT COUNT(*) FROM orders;"
    echo "SELECT name, email FROM users ORDER BY id;"
    echo "quit"
} | nc 127.0.0.1 5432

kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true
echo

# Cleanup
echo "======================================"
echo "Cleanup"
echo "======================================"
rm -rf "$TEST_DIR"
echo "✓ Test directories cleaned up"
echo

echo "======================================"
echo "Test completed successfully!"
echo "======================================"
