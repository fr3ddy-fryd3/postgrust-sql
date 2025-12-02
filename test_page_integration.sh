#!/bin/bash
# Test page-based storage integration

echo "=== Testing Page-Based Storage Integration ==="

# Start server with page storage
RUSTDB_USE_PAGE_STORAGE=1 timeout 10 cargo run --release &
SERVER_PID=$!
sleep 2

# Test CREATE TABLE
echo "Testing CREATE TABLE..."
printf "CREATE TABLE users (id INTEGER, name TEXT);\nquit\n" | nc 127.0.0.1 5432

# Test INSERT
echo "Testing INSERT..."
printf "INSERT INTO users (id, name) VALUES (1, 'Alice');\nquit\n" | nc 127.0.0.1 5432

# Test SELECT
echo "Testing SELECT..."
printf "SELECT * FROM users;\nquit\n" | nc 127.0.0.1 5432

# Cleanup
kill $SERVER_PID 2>/dev/null

echo "=== Test Complete ==="
