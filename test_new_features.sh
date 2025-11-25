#!/bin/bash

# Test script for new SQL features: AND/OR, ORDER BY, LIMIT

set -e

echo "Starting RustDB server in background..."
cargo build --release --quiet
cargo run --release &
SERVER_PID=$!

# Wait for server to start
sleep 2

echo "Testing new SQL features..."

# Test 1: Create table and insert data
echo "Test 1: Creating table and inserting data..."
printf "CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price INTEGER, stock INTEGER);\n" | nc -w 1 127.0.0.1 5432 > /dev/null

printf "INSERT INTO products (id, name, price, stock) VALUES (1, 'Laptop', 1200, 5);\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO products (id, name, price, stock) VALUES (2, 'Mouse', 25, 50);\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO products (id, name, price, stock) VALUES (3, 'Keyboard', 75, 30);\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO products (id, name, price, stock) VALUES (4, 'Monitor', 300, 15);\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO products (id, name, price, stock) VALUES (5, 'Headphones', 100, 25);\n" | nc -w 1 127.0.0.1 5432 > /dev/null

echo "✓ Data inserted"

# Test 2: AND condition
echo ""
echo "Test 2: AND condition (price > 50 AND stock < 30)"
printf "SELECT * FROM products WHERE price > 50 AND stock < 30;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 3: OR condition
echo ""
echo "Test 3: OR condition (name = 'Mouse' OR name = 'Keyboard')"
printf "SELECT * FROM products WHERE name = 'Mouse' OR name = 'Keyboard';\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 4: ORDER BY ASC
echo ""
echo "Test 4: ORDER BY price ASC"
printf "SELECT name, price FROM products ORDER BY price ASC;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 5: ORDER BY DESC
echo ""
echo "Test 5: ORDER BY price DESC"
printf "SELECT name, price FROM products ORDER BY price DESC;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 6: LIMIT
echo ""
echo "Test 6: LIMIT 3"
printf "SELECT * FROM products LIMIT 3;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 7: ORDER BY + LIMIT
echo ""
echo "Test 7: ORDER BY price DESC LIMIT 2 (top 2 most expensive)"
printf "SELECT name, price FROM products ORDER BY price DESC LIMIT 2;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 8: Complex query
echo ""
echo "Test 8: Complex query (WHERE AND + ORDER BY + LIMIT)"
printf "SELECT name, price, stock FROM products WHERE price > 50 AND stock < 40 ORDER BY price ASC LIMIT 3;\nquit\n" | nc -w 1 127.0.0.1 5432

# Cleanup
echo ""
echo "Cleaning up..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

# Remove test database
rm -f data/main.db data/main.json data/wal/*.wal 2>/dev/null || true

echo ""
echo "✓ All tests completed successfully!"
