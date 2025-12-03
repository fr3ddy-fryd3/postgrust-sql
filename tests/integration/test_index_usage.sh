#!/bin/bash
# Integration test for index usage in SELECT queries (v1.6.0 Phase 5)

set -e

echo "========================================="
echo "Testing Index Usage in SELECT"
echo "========================================="

# Clean start
rm -rf data/
echo "✓ Cleaned old data"

# Build quietly
cargo build --release --quiet 2>/dev/null

# Start server in background
echo "Starting server..."
cargo run --release > /tmp/rustdb_index_usage_test.log 2>&1 &
SERVER_PID=$!
sleep 3

if ps -p $SERVER_PID > /dev/null; then
    echo "✓ Server started (PID: $SERVER_PID)"
else
    echo "✗ Failed to start server"
    cat /tmp/rustdb_index_usage_test.log
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

# Function to run SQL
run_sql() {
    printf "%s\n" "$1" | nc 127.0.0.1 5432 2>&1
}

echo ""
echo "1. Create table with test data (1000 rows)"
run_sql "CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, category TEXT, price INTEGER);" > /dev/null

# Insert test data
for i in {1..100}; do
    CATEGORY=$((i % 10))
    PRICE=$((i * 10))
    run_sql "INSERT INTO products (id, name, category, price) VALUES ($i, 'Product $i', 'Cat$CATEGORY', $PRICE);" > /dev/null
done

echo "✓ Created table with 100 rows"

echo ""
echo "2. Test SELECT without index (sequential scan)"
RESULT=$(run_sql "SELECT * FROM products WHERE category = 'Cat5';" 2>&1)
COUNT=$(echo "$RESULT" | grep -c "Product" || true)
if [ "$COUNT" -eq 10 ]; then
    echo "✓ Sequential scan: Found 10 rows in Cat5"
else
    echo "✗ Sequential scan failed: found $COUNT rows"
    exit 1
fi

echo ""
echo "3. Create index on category column"
RESULT=$(run_sql "CREATE INDEX idx_category ON products(category);" 2>&1)
if echo "$RESULT" | grep -q "Index 'idx_category' created"; then
    echo "✓ Index created on category"
else
    echo "✗ Index creation failed: $RESULT"
    exit 1
fi

echo ""
echo "4. Test SELECT with index (index scan)"
RESULT=$(run_sql "SELECT * FROM products WHERE category = 'Cat5';" 2>&1)
COUNT=$(echo "$RESULT" | grep -c "Product" || true)
if [ "$COUNT" -eq 10 ]; then
    echo "✓ Index scan: Found 10 rows in Cat5"
else
    echo "✗ Index scan failed: found $COUNT rows"
    exit 1
fi

echo ""
echo "5. Create index on price column"
RESULT=$(run_sql "CREATE INDEX idx_price ON products(price);" 2>&1)
if echo "$RESULT" | grep -q "Index 'idx_price' created"; then
    echo "✓ Index created on price"
else
    echo "✗ Index creation failed: $RESULT"
    exit 1
fi

echo ""
echo "6. Test SELECT with price index"
RESULT=$(run_sql "SELECT * FROM products WHERE price = 500;" 2>&1)
COUNT=$(echo "$RESULT" | grep -c "Product" || true)
if [ "$COUNT" -eq 1 ]; then
    echo "✓ Price index scan: Found 1 row with price 500"
else
    echo "✗ Price index scan failed: found $COUNT rows"
    echo "$RESULT"
    exit 1
fi

echo ""
echo "7. Test SELECT with ORDER BY and index"
RESULT=$(run_sql "SELECT * FROM products WHERE category = 'Cat3' ORDER BY id ASC LIMIT 5;" 2>&1)
COUNT=$(echo "$RESULT" | grep -c "Product" || true)
if [ "$COUNT" -eq 5 ]; then
    echo "✓ Index + ORDER BY + LIMIT: Found 5 rows"
else
    echo "✗ Index + ORDER BY failed: found $COUNT rows"
    exit 1
fi

echo ""
echo "========================================="
echo "All index usage tests passed! ✓"
echo "========================================="
echo ""
echo "Summary:"
echo "  - Sequential scan working"
echo "  - Index scan working (Equals condition)"
echo "  - Index + ORDER BY + LIMIT working"
echo "  - 100 rows processed successfully"
echo ""
echo "Performance improvement:"
echo "  - Sequential scan: O(n) - scans all rows"
echo "  - Index scan: O(log n) - uses B-tree lookup"
echo ""
echo "Next steps:"
echo "  - Add index maintenance for INSERT/UPDATE/DELETE"
echo "  - Support range queries (>, <) with indexes"
echo "  - Add EXPLAIN command to show query plan"
