#!/bin/bash
# Integration test for FOREIGN KEY, JOIN, and SERIAL functionality

set -e

echo "=== Testing FOREIGN KEY, JOIN, and SERIAL ==="

# Start server in background
echo "Starting server..."
cargo build --release 2>/dev/null
./target/release/postgrustql &
SERVER_PID=$!
sleep 2

echo "Testing FOREIGN KEY, JOIN and SERIAL operations..."

# Test 1: Create parent table with SERIAL
echo "1. Creating users table with SERIAL..."
printf "CREATE TABLE users (id SERIAL, name TEXT NOT NULL);\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 2: Create child table with SERIAL and foreign key
echo "2. Creating orders table with SERIAL and foreign key..."
printf "CREATE TABLE orders (id SERIAL, user_id INTEGER NOT NULL REFERENCES users(id), product TEXT NOT NULL);\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 3: Insert data into parent table (using SERIAL - no id needed)
echo "3. Inserting users (SERIAL auto-increment)..."
printf "INSERT INTO users (name) VALUES ('Alice');\nINSERT INTO users (name) VALUES ('Bob');\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 4: Insert data into child table (using SERIAL - no id needed)
echo "4. Inserting orders for existing users (SERIAL auto-increment)..."
printf "INSERT INTO orders (user_id, product) VALUES (1, 'Laptop');\nINSERT INTO orders (user_id, product) VALUES (2, 'Phone');\nINSERT INTO orders (user_id, product) VALUES (1, 'Mouse');\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 5: Try to insert invalid foreign key (should fail)
echo "5. Testing foreign key violation (should fail)..."
printf "INSERT INTO orders (user_id, product) VALUES (99, 'Keyboard');\nquit\n" | nc -q 1 127.0.0.1 5432 2>&1 | grep -i "foreign key" && echo "✓ FK violation detected" || echo "✗ FK violation not detected"

# Test 6: INNER JOIN
echo "6. Testing INNER JOIN..."
printf "SELECT * FROM users JOIN orders ON users.id = orders.user_id;\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 7: LEFT JOIN
echo "7. Testing LEFT JOIN..."
printf "INSERT INTO users (name) VALUES ('Charlie');\nquit\n" | nc -q 1 127.0.0.1 5432
printf "SELECT * FROM users LEFT JOIN orders ON users.id = orders.user_id;\nquit\n" | nc -q 1 127.0.0.1 5432

# Test 8: Check tables
echo "8. Checking tables..."
printf "SHOW TABLES;\nquit\n" | nc -q 1 127.0.0.1 5432

# Cleanup
echo "Stopping server..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

echo "=== Test completed ==="
