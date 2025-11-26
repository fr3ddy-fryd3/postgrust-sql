#!/bin/bash
# Quick SERIAL test

echo "Building..."
cargo build --release 2>/dev/null

echo "Starting server..."
./target/release/postgrustql &
SERVER_PID=$!
sleep 2

echo ""
echo "=== Creating table with SERIAL ==="
printf "CREATE TABLE test (id SERIAL, name TEXT);\nquit\n" | nc -q 1 127.0.0.1 5432
echo ""

echo "=== Inserting 3 rows without id ==="
printf "INSERT INTO test (name) VALUES ('First');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test (name) VALUES ('Second');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO test (name) VALUES ('Third');\nquit\n" | nc -q 1 127.0.0.1 5432
echo ""

echo "=== Selecting all (should show ids 1, 2, 3) ==="
printf "SELECT * FROM test;\nquit\n" | nc -q 1 127.0.0.1 5432
echo ""

kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null

echo "Done!"
