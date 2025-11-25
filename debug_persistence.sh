#!/bin/bash

echo "=== Debug Persistence Test ==="

# Clean and rebuild
rm -rf data/
cargo build --release --quiet 2>&1 | head -3

# Start server
cargo run --release >/tmp/server.log 2>&1 &
SERVER_PID=$!
sleep 2
echo "Server started (PID: $SERVER_PID)"

# Execute operations
{
  echo "CREATE TABLE test (id INT, val TEXT);"
  for i in {1..105}; do
    echo "INSERT INTO test VALUES ($i, 'val$i');"
  done
  echo "quit"
} | nc 127.0.0.1 5432 > /tmp/result.txt

sleep 2

# Check data
echo ""
echo "=== Data Directory ==="
ls -lh data/
echo ""
echo "=== WAL Directory ==="
ls -lh data/wal/
echo ""
echo "=== Snapshot Files ==="
ls -lh data/*.db 2>&1 || echo "No .db snapshot found"
ls -lh data/*.json 2>&1 || echo "No .json snapshot found"

echo ""
echo "=== WAL File Sizes ==="
for f in data/wal/*.wal; do
  if [ -f "$f" ]; then
    size=$(stat -c%s "$f" 2>/dev/null || stat -f%z "$f" 2>/dev/null)
    echo "$f: $size bytes"
  fi
done

# Stop server
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null || true
echo ""
echo "Server stopped"
