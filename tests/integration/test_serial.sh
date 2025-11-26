#!/bin/bash
# Integration test for SERIAL functionality

set -e

echo "=== Testing SERIAL (auto-increment) ==="

# Start server in background
echo "Starting server..."
cargo build --release 2>/dev/null
./target/release/postgrustql &
SERVER_PID=$!
sleep 2

echo "Testing SERIAL operations..."

# Test 1: Create table with SERIAL primary key
echo "1. Creating users table with SERIAL id..."
printf "CREATE TABLE users (id SERIAL, name TEXT NOT NULL, email TEXT);\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 2: Insert without specifying id (auto-increment should work)
echo "2. Inserting users without id (auto-increment)..."
printf "INSERT INTO users (name, email) VALUES ('Alice', 'alice@example.com');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO users (name, email) VALUES ('Bob', 'bob@example.com');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO users (name, email) VALUES ('Charlie', 'charlie@example.com');\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 3: View results - should have ids 1, 2, 3
echo "3. Viewing all users (should have auto-incremented ids)..."
printf "SELECT * FROM users;\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 4: Create products table with SERIAL
echo "4. Creating products table with SERIAL..."
printf "CREATE TABLE products (id SERIAL, name TEXT NOT NULL, price INTEGER NOT NULL);\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 5: Insert products
echo "5. Inserting products..."
printf "INSERT INTO products (name, price) VALUES ('Laptop', 1000);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO products (name, price) VALUES ('Mouse', 50);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO products (name, price) VALUES ('Keyboard', 100);\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 6: View products
echo "6. Viewing all products..."
printf "SELECT * FROM products;\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 7: Test SERIAL with FK (orders referencing users)
echo "7. Creating orders table with SERIAL and FK..."
printf "CREATE TABLE orders (id SERIAL, user_id INTEGER NOT NULL REFERENCES users(id), product_id INTEGER NOT NULL REFERENCES products(id), quantity INTEGER NOT NULL);\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 8: Insert orders
echo "8. Inserting orders..."
printf "INSERT INTO orders (user_id, product_id, quantity) VALUES (1, 1, 1);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO orders (user_id, product_id, quantity) VALUES (2, 2, 3);\nquit\n" | nc -q 1 127.0.0.1 5432
printf "INSERT INTO orders (user_id, product_id, quantity) VALUES (1, 3, 2);\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 9: JOIN with SERIAL tables
echo "9. Testing JOIN with SERIAL columns..."
printf "SELECT users.name, products.name, orders.quantity FROM orders JOIN users ON orders.user_id = users.id JOIN products ON orders.product_id = products.id;\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 10: Show all tables
echo "10. Showing all tables..."
printf "SHOW TABLES;\nquit\n" | nc -q 1 127.0.0.1 5432

# Cleanup
echo "Stopping server..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo "=== SERIAL test completed ==="
