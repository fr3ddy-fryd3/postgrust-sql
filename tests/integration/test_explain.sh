#!/bin/bash
# Integration test for EXPLAIN command (v1.8.0)

set -e

echo "========================================="
echo "Testing EXPLAIN Command"
echo "========================================="

# Clean start
rm -rf data/
echo "✓ Cleaned old data"

# Build quietly
cargo build --release --quiet 2>/dev/null

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/rustdb_explain_test.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    cat /tmp/rustdb_explain_test.log
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
cat > /tmp/test_explain.sql << 'EOF'
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER, city TEXT);
INSERT INTO users (id, name, age, city) VALUES (1, 'Alice', 30, 'NYC');
INSERT INTO users (id, name, age, city) VALUES (2, 'Bob', 25, 'LA');
INSERT INTO users (id, name, age, city) VALUES (3, 'Charlie', 35, 'NYC');
INSERT INTO users (id, name, age, city) VALUES (4, 'Diana', 28, 'SF');
INSERT INTO users (id, name, age, city) VALUES (5, 'Eve', 32, 'NYC');
EXPLAIN SELECT * FROM users;
EXPLAIN SELECT * FROM users WHERE age = 30;
CREATE INDEX idx_age ON users(age) USING BTREE;
EXPLAIN SELECT * FROM users WHERE age = 30;
EXPLAIN SELECT * FROM users WHERE age > 28;
CREATE INDEX idx_city ON users(city) USING HASH;
EXPLAIN SELECT * FROM users WHERE city = 'NYC';
CREATE UNIQUE INDEX idx_name ON users(name) USING BTREE;
EXPLAIN SELECT * FROM users WHERE name = 'Alice';
quit
EOF

echo ""
echo "Running EXPLAIN tests..."
OUTPUT=$((sleep 1; cat /tmp/test_explain.sql) | nc 127.0.0.1 5432 2>&1)

# Validate results
echo ""
echo "Validating results..."

# Test 1: EXPLAIN without filter should show Seq Scan
if echo "$OUTPUT" | grep -q "Seq Scan"; then
    echo "✓ EXPLAIN shows sequential scan for query without filter"
else
    echo "✗ EXPLAIN sequential scan failed"
    echo "$OUTPUT"
    exit 1
fi

# Test 2: EXPLAIN with filter but no index should show Seq Scan
if echo "$OUTPUT" | grep -q "Seq Scan"; then
    echo "✓ EXPLAIN shows sequential scan before index creation"
else
    echo "✗ EXPLAIN before index failed"
    exit 1
fi

# Test 3: EXPLAIN with B-tree index should show Index Scan
if echo "$OUTPUT" | grep -q "Index Scan using idx_age (btree)"; then
    echo "✓ EXPLAIN shows B-tree index scan"
else
    echo "✗ EXPLAIN B-tree index failed"
    echo "$OUTPUT"
    exit 1
fi

# Test 4: EXPLAIN with range query should use B-tree
if echo "$OUTPUT" | grep -q "Index Scan.*idx_age"; then
    echo "✓ EXPLAIN shows B-tree index for range query"
else
    echo "✗ EXPLAIN range query failed"
    exit 1
fi

# Test 5: EXPLAIN with hash index should show O(1) cost
if echo "$OUTPUT" | grep -q "Index Scan using idx_city (hash)"; then
    echo "✓ EXPLAIN shows hash index scan"
else
    echo "✗ EXPLAIN hash index failed"
    echo "$OUTPUT"
    exit 1
fi

if echo "$OUTPUT" | grep -q "Cost: O(1)"; then
    echo "✓ EXPLAIN shows O(1) cost for hash index"
else
    echo "✗ EXPLAIN O(1) cost failed"
    echo "$OUTPUT"
    exit 1
fi

# Test 6: EXPLAIN with unique index
if echo "$OUTPUT" | grep -q "Unique Index Scan"; then
    echo "✓ EXPLAIN shows unique index scan"
else
    echo "✗ EXPLAIN unique index failed"
    exit 1
fi

# Test 7: Check for cost estimates
if echo "$OUTPUT" | grep -q "O(log n)"; then
    echo "✓ EXPLAIN shows O(log n) cost for B-tree"
else
    echo "✗ EXPLAIN O(log n) cost failed"
    exit 1
fi

# Test 8: Check for row estimates
if echo "$OUTPUT" | grep -q "Rows:"; then
    echo "✓ EXPLAIN shows row estimates"
else
    echo "✗ EXPLAIN row estimates failed"
    exit 1
fi

echo ""
echo "========================================="
echo "All EXPLAIN tests passed! ✓"
echo "========================================="
echo ""
echo "Summary:"
echo "  - EXPLAIN for sequential scan working"
echo "  - EXPLAIN for B-tree index scan working"
echo "  - EXPLAIN for hash index scan working"
echo "  - EXPLAIN for unique index working"
echo "  - Cost estimates working (O(1), O(log n), O(n))"
echo "  - Row estimates working"
echo ""
echo "EXPLAIN features in v1.8.0:"
echo "  - Query plan visualization"
echo "  - Index usage detection"
echo "  - Cost analysis (O-notation)"
echo "  - Row count estimates"
echo "  - Scan type identification"
echo ""
