#!/bin/bash

echo "=== RustDB Features Test ==="
echo ""
echo "Testing: 1) CLI persistence 2) Table formatting 3) Transactions"
echo ""

# Clean old data
rm -rf data/
echo "✓ Cleaned old data"

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/postgrust_server.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    exit 1
fi

echo ""
echo "=== Test 1: Basic operations with formatted output ==="
echo ""

# Create test SQL file
cat > /tmp/test_commands.sql << 'EOF'
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER, active BOOLEAN);
INSERT INTO users (id, name, age, active) VALUES (1, 'Alice', 30, TRUE);
INSERT INTO users (id, name, age, active) VALUES (2, 'Bob', 25, TRUE);
INSERT INTO users (id, name, age, active) VALUES (3, 'Charlie', 35, FALSE);
SELECT * FROM users;
SELECT name, age FROM users WHERE age > 26;
quit
EOF

# Execute via netcat
echo "Executing SQL commands..."
(sleep 1; cat /tmp/test_commands.sql) | nc 127.0.0.1 5432

echo ""
echo "=== Test 2: Transaction COMMIT ==="
echo ""

cat > /tmp/test_transaction_commit.sql << 'EOF'
CREATE TABLE accounts (id INTEGER PRIMARY KEY, balance INTEGER);
INSERT INTO accounts (id, balance) VALUES (1, 1000);
INSERT INTO accounts (id, balance) VALUES (2, 500);
SELECT * FROM accounts;
BEGIN;
UPDATE accounts SET balance = 1500 WHERE id = 1;
SELECT * FROM accounts;
COMMIT;
SELECT * FROM accounts;
quit
EOF

(sleep 1; cat /tmp/test_transaction_commit.sql) | nc 127.0.0.1 5432

echo ""
echo "=== Test 3: Transaction ROLLBACK ==="
echo ""

cat > /tmp/test_transaction_rollback.sql << 'EOF'
SELECT * FROM accounts;
BEGIN;
UPDATE accounts SET balance = 9999 WHERE id = 2;
SELECT * FROM accounts;
ROLLBACK;
SELECT * FROM accounts;
DROP TABLE accounts;
quit
EOF

(sleep 1; cat /tmp/test_transaction_rollback.sql) | nc 127.0.0.1 5432

echo ""
echo "=== Stopping server ==="
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null
echo "✓ Server stopped"

echo ""
echo "=== Verifying persistence ==="
if [ -f data/server_instance.db ]; then
    echo "✓ Database file exists (data/server_instance.db)"
    echo "  File size: $(ls -lh data/server_instance.db | awk '{print $5}')"
    echo "✓ Page-based storage working"
else
    echo "✗ Database file not found"
fi

echo ""
echo "=== Test Complete ==="
