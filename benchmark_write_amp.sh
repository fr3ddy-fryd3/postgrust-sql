#!/bin/bash
# Benchmark write amplification: Legacy vs Page-Based Storage

set -e

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     Write Amplification Benchmark                        ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Build release
echo "Building release binary..."
cargo build --release --quiet

# Test parameters
NUM_ROWS=1000
NUM_UPDATES=100
ROW_SIZE=100  # bytes per row (approximate)
DATA_SIZE=$((NUM_ROWS * ROW_SIZE))  # Total logical data size

echo ""
echo "Test Parameters:"
echo "  • Rows: $NUM_ROWS"
echo "  • Updates: $NUM_UPDATES (updating 1 row each)"
echo "  • Logical data size: $DATA_SIZE bytes (~$(($DATA_SIZE / 1024))KB)"
echo ""

# Function to run benchmark
run_benchmark() {
    local MODE=$1
    local ENV_VAR=$2

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Testing: $MODE"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    # Clean data directory
    rm -rf data
    mkdir -p data

    # Start server
    echo "Starting server..."
    $ENV_VAR timeout 30 cargo run --release &>/dev/null &
    SERVER_PID=$!
    sleep 2

    # Record initial disk usage
    BEFORE_SIZE=$(du -sb data 2>/dev/null | cut -f1)
    echo "Initial disk usage: $BEFORE_SIZE bytes"

    # Create table and insert data
    echo "Creating table and inserting $NUM_ROWS rows..."
    printf "CREATE TABLE bench (id INTEGER, data TEXT);\n" | nc -q 1 127.0.0.1 5432 &>/dev/null

    for i in $(seq 1 $NUM_ROWS); do
        # Insert row with ~100 bytes of data
        printf "INSERT INTO bench (id, data) VALUES ($i, 'Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt');\n" | nc -q 1 127.0.0.1 5432 &>/dev/null
    done

    echo "Performing $NUM_UPDATES UPDATE operations..."
    for i in $(seq 1 $NUM_UPDATES); do
        # Update single row
        printf "UPDATE bench SET data = 'Updated data iteration $i' WHERE id = 1;\n" | nc -q 1 127.0.0.1 5432 &>/dev/null
    done

    # Give server time to checkpoint
    sleep 2

    # Record final disk usage
    AFTER_SIZE=$(du -sb data 2>/dev/null | cut -f1)
    echo "Final disk usage: $AFTER_SIZE bytes"

    # Calculate write amplification
    WRITTEN=$((AFTER_SIZE - BEFORE_SIZE))
    LOGICAL=$((NUM_UPDATES * ROW_SIZE))  # Only updated data

    if [ $LOGICAL -gt 0 ]; then
        AMPLIFICATION=$((WRITTEN / LOGICAL))
    else
        AMPLIFICATION=0
    fi

    echo ""
    echo "Results:"
    echo "  • Logical writes: $LOGICAL bytes (${NUM_UPDATES} updates × ${ROW_SIZE}B)"
    echo "  • Physical writes: $WRITTEN bytes ($(($WRITTEN / 1024))KB)"
    echo "  • Write amplification: ${AMPLIFICATION}x"
    echo ""

    # Stop server
    kill $SERVER_PID 2>/dev/null
    wait $SERVER_PID 2>/dev/null

    # Return amplification for comparison
    echo $AMPLIFICATION
}

# Run benchmarks
echo ""
LEGACY_AMP=$(run_benchmark "Legacy Storage (Vec<Row>)" "RUSTDB_USE_PAGE_STORAGE=0")

PAGE_AMP=$(run_benchmark "Page-Based Storage" "RUSTDB_USE_PAGE_STORAGE=1")

# Calculate improvement
if [ $PAGE_AMP -gt 0 ]; then
    IMPROVEMENT=$((LEGACY_AMP / PAGE_AMP))
else
    IMPROVEMENT="∞"
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║                    SUMMARY                               ║"
echo "╠══════════════════════════════════════════════════════════╣"
echo "║  Legacy Storage:      ${LEGACY_AMP}x amplification"
echo "║  Page-Based Storage:  ${PAGE_AMP}x amplification"
echo "║                                                          ║"
echo "║  🚀 Improvement:      ${IMPROVEMENT}x better!"
echo "╚══════════════════════════════════════════════════════════╝"

# Cleanup
rm -rf data
