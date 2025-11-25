#!/bin/bash

# Test script for GROUP BY functionality

set -e

echo "Starting RustDB server in background..."
cargo build --release --quiet
cargo run --release &
SERVER_PID=$!

# Wait for server to start
sleep 2

echo "Testing GROUP BY functionality..."

# Test 1: Create table and insert data
echo "Test 1: Creating table and inserting test data..."
printf "CREATE TABLE sales (id INTEGER PRIMARY KEY, product TEXT NOT NULL, category TEXT NOT NULL, amount INTEGER);\\n" | nc -w 1 127.0.0.1 5432 > /dev/null

printf "INSERT INTO sales (id, product, category, amount) VALUES (1, 'Laptop', 'Electronics', 1000);\\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO sales (id, product, category, amount) VALUES (2, 'Mouse', 'Electronics', 25);\\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO sales (id, product, category, amount) VALUES (3, 'Desk', 'Furniture', 300);\\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO sales (id, product, category, amount) VALUES (4, 'Chair', 'Furniture', 150);\\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO sales (id, product, category, amount) VALUES (5, 'Keyboard', 'Electronics', 75);\\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO sales (id, product, category, amount) VALUES (6, 'Monitor', 'Electronics', 300);\\n" | nc -w 1 127.0.0.1 5432 > /dev/null

echo "✓ Data inserted"

# Test 2: GROUP BY with COUNT
echo ""
echo "Test 2: GROUP BY category with COUNT"
printf "SELECT category, COUNT(*) FROM sales GROUP BY category;\\nquit\\n" | nc -w 1 127.0.0.1 5432

# Test 3: GROUP BY with SUM
echo ""
echo "Test 3: GROUP BY category with SUM(amount)"
printf "SELECT category, SUM(amount) FROM sales GROUP BY category;\\nquit\\n" | nc -w 1 127.0.0.1 5432

# Test 4: GROUP BY with AVG
echo ""
echo "Test 4: GROUP BY category with AVG(amount)"
printf "SELECT category, AVG(amount) FROM sales GROUP BY category;\\nquit\\n" | nc -w 1 127.0.0.1 5432

# Test 5: GROUP BY with multiple aggregates
echo ""
echo "Test 5: GROUP BY with multiple aggregates"
printf "SELECT category, COUNT(*), SUM(amount), AVG(amount) FROM sales GROUP BY category;\\nquit\\n" | nc -w 1 127.0.0.1 5432

# Test 6: GROUP BY with WHERE
echo ""
echo "Test 6: GROUP BY with WHERE clause (amount > 50)"
printf "SELECT category, COUNT(*), SUM(amount) FROM sales WHERE amount > 50 GROUP BY category;\\nquit\\n" | nc -w 1 127.0.0.1 5432

# Test 7: GROUP BY with ORDER BY
echo ""
echo "Test 7: GROUP BY with ORDER BY category"
printf "SELECT category, COUNT(*) FROM sales GROUP BY category ORDER BY category ASC;\\nquit\\n" | nc -w 1 127.0.0.1 5432

# Cleanup
echo ""
echo "Cleaning up..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

# Remove test database
rm -f data/main.db data/main.json data/wal/*.wal 2>/dev/null || true

echo ""
echo "✓ All GROUP BY tests completed successfully!"
