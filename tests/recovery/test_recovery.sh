#!/bin/bash

echo "=== Testing WAL Recovery ==="
echo "Starting server WITHOUT clearing data/..."
cargo run --release > /tmp/recovery.log 2>&1 &
SERVER_PID=$!
sleep 3

echo "Querying data (should recover from WAL)..."
printf "SELECT * FROM t;\nquit\n" | nc 127.0.0.1 5432

sleep 1
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null || true

echo ""
echo "=== If SELECT returned 2 rows - recovery successful! ==="
