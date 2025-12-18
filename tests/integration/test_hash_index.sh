#!/bin/bash
# Integration test for Hash indexes (v1.7.0)

set -e

echo "========================================="
echo "Testing Hash Index Implementation"
echo "========================================="

# Clean start
rm -rf data/
echo "✓ Cleaned old data"

# Build quietly
cargo build --release --quiet 2>/dev/null

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/rustdb_hash_test.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    cat /tmp/rustdb_hash_test.log
    exit 1
fi

# Cleanup function
cleanup() {
    echo ""
    echo "Shutting down server..."
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true
    echo "✓ Server stopped"
}
trap cleanup EXIT

# Create test SQL file
cat > /tmp/test_hash.sql << 'EOF'
CREATE TABLE products (id INTEGER PRIMARY KEY, category TEXT NOT NULL, price INTEGER, name TEXT);
INSERT INTO products (id, category, price, name) VALUES (1, 'Electronics', 999, 'Laptop');
INSERT INTO products (id, category, price, name) VALUES (2, 'Books', 29, 'SQL Guide');
INSERT INTO products (id, category, price, name) VALUES (3, 'Electronics', 499, 'Phone');
INSERT INTO products (id, category, price, name) VALUES (4, 'Books', 19, 'Programming');
INSERT INTO products (id, category, price, name) VALUES (5, 'Electronics', 199, 'Headphones');
CREATE INDEX idx_category ON products(category) USING HASH;
CREATE INDEX idx_price ON products(price) USING BTREE;
CREATE INDEX idx_name ON products(name);
CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT);
INSERT INTO users (id, email) VALUES (1, 'alice@example.com');
INSERT INTO users (id, email) VALUES (2, 'bob@example.com');
CREATE UNIQUE INDEX idx_email ON users(email) USING HASH;
SELECT * FROM products WHERE category = 'Electronics';
SELECT * FROM products WHERE category = 'Books';
DROP INDEX idx_category;
SELECT * FROM products WHERE category = 'Books';
quit
EOF

echo ""
echo "Running hash index tests..."
OUTPUT=$((sleep 1; cat /tmp/test_hash.sql) | nc 127.0.0.1 5432 2>&1)

# Validate results
echo ""
echo "Validating results..."

if echo "$OUTPUT" | grep -qi "idx_category.*created.*hash\|created.*idx_category.*hash"; then
    echo "✓ CREATE INDEX USING HASH succeeded"
else
    echo "✗ CREATE INDEX USING HASH failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -qi "idx_price.*created.*btree\|created.*idx_price.*btree"; then
    echo "✓ CREATE INDEX USING BTREE succeeded"
else
    echo "✗ CREATE INDEX USING BTREE failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -qi "idx_name.*created.*btree\|created.*idx_name.*btree"; then
    echo "✓ CREATE INDEX without USING (default BTREE) succeeded"
else
    echo "✗ CREATE INDEX default failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -qi "idx_email.*created.*hash\|created.*idx_email.*hash"; then
    echo "✓ CREATE UNIQUE INDEX USING HASH succeeded"
else
    echo "✗ CREATE UNIQUE INDEX USING HASH failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -q "Laptop"; then
    echo "✓ Hash index equality query returns correct results"
else
    echo "✗ Hash index query failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -qi "idx_category.*dropped\|dropped.*idx_category"; then
    echo "✓ DROP INDEX on hash index succeeded"
else
    echo "✗ DROP INDEX failed"
    echo "$OUTPUT"
    exit 1
fi

# Test unique constraint violation separately
cat > /tmp/test_unique.sql << 'EOF'
INSERT INTO users (id, email) VALUES (3, 'alice@example.com');
quit
EOF

UNIQUE_TEST=$((sleep 1; cat /tmp/test_unique.sql) | nc 127.0.0.1 5432 2>&1)
if echo "$UNIQUE_TEST" | grep -q "unique constraint"; then
    echo "✓ Hash index unique constraint working"
else
    echo "✗ Expected unique constraint violation"
    echo "$UNIQUE_TEST"
    exit 1
fi

echo ""
echo "========================================="
echo "All hash index tests passed! ✓"
echo "========================================="
echo ""
echo "Summary:"
echo "  - HASH index creation working"
echo "  - BTREE index creation working"
echo "  - Default index type (BTREE) working"
echo "  - UNIQUE HASH index working"
echo "  - Hash index equality queries working"
echo "  - Hash index unique constraints working"
echo "  - DROP INDEX working for hash indexes"
echo ""
echo "Index types available in v1.7.0:"
echo "  - BTREE: O(log n) lookups, supports range queries"
echo "  - HASH:  O(1) average case, equality queries only"
echo ""
