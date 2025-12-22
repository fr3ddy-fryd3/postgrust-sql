#!/bin/bash

pkill postgrust 2>/dev/null
sleep 1

rm -rf data/
cargo build --release --quiet 2>&1 | tail -2

echo "Starting server..."
cargo run --release > /tmp/wal_test.log 2>&1 &
SERVER_PID=$!
sleep 3

echo "Sending queries..."
{
  echo "CREATE TABLE t (id INT);"
  echo "INSERT INTO t VALUES (1);"
  echo "INSERT INTO t VALUES (2);"
  echo "quit"
} | nc 127.0.0.1 5432

sleep 2
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null || true

echo ""
echo "=== Server Debug Log ==="
grep DEBUG /tmp/wal_test.log

echo ""
echo "=== Data Files ==="
ls -lh data/
ls -lh data/wal/
