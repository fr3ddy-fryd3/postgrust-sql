#!/bin/bash
# Quick test for all new data types

set -e

echo "=== Testing New Data Types ==="

# Build and start server
echo "Building..."
cargo build --release 2>/dev/null
./target/release/postgrustql &
SERVER_PID=$!
sleep 2

echo ""
echo "=== Testing SMALLINT ==="
printf "CREATE TABLE test_small (id SMALLINT, val INTEGER);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_small VALUES (100, 50000);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_small;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing BIGSERIAL ==="
printf "CREATE TABLE test_bigserial (id BIGSERIAL, name TEXT);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_bigserial (name) VALUES ('Alice');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_bigserial (name) VALUES ('Bob');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_bigserial;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing NUMERIC ==="
printf "CREATE TABLE test_numeric (price NUMERIC(10, 2));\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_numeric VALUES (123.45);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_numeric;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing VARCHAR(n) ==="
printf "CREATE TABLE test_varchar (name VARCHAR(10));\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_varchar VALUES ('Short');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_varchar;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing CHAR(n) with padding ==="
printf "CREATE TABLE test_char (code CHAR(5));\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_char VALUES ('ABC');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_char;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing DATE ==="
printf "CREATE TABLE test_date (event_date DATE);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_date VALUES ('2025-01-15');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_date;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing TIMESTAMP ==="
printf "CREATE TABLE test_ts (created_at TIMESTAMP);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_ts VALUES ('2025-01-15 14:30:00');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_ts;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing UUID ==="
printf "CREATE TABLE test_uuid (user_id UUID);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_uuid VALUES ('550e8400-e29b-41d4-a716-446655440000');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_uuid;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing JSON ==="
printf 'CREATE TABLE test_json (data JSON);\nquit\n' | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_json VALUES ('{\"key\":\"value\"}');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_json;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== Testing ENUM ==="
printf "CREATE TYPE mood AS ENUM ('happy', 'sad', 'neutral');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "CREATE TABLE test_enum (person_name TEXT, current_mood mood);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test_enum VALUES ('Alice', 'happy');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM test_enum;\nquit\n" | nc -q 1 127.0.0.1 5432

echo ""
echo "=== All tables created ==="
printf "SHOW TABLES;\nquit\n" | nc -q 1 127.0.0.1 5432

# Cleanup
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null

echo ""
echo "=== Test completed successfully! ==="
