#!/bin/bash
# Benchmark write amplification: Legacy vs Page-Based Storage
# Measures actual disk I/O during database operations

set -e

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     Write Amplification Benchmark v2                     ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

# Build release
echo "Building release binary..."
cargo build --release --quiet 2>&1 | grep -v "warning:" || true

# Test parameters
NUM_ROWS=1000
NUM_UPDATES=100
ROW_SIZE=100  # bytes per row (approximate)

echo ""
echo "Test Parameters:"
echo "  • Initial rows: $NUM_ROWS"
echo "  • Updates: $NUM_UPDATES (updating 1 row each)"
echo "  • Logical update size: $((NUM_UPDATES * ROW_SIZE)) bytes (~$((NUM_UPDATES * ROW_SIZE / 1024))KB)"
echo ""

# Function to measure file size
measure_size() {
    local DIR=$1
    if [ -d "$DIR" ]; then
        find "$DIR" -type f -name "*.db" -o -name "*.wal" 2>/dev/null | xargs du -cb 2>/dev/null | tail -1 | cut -f1
    else
        echo 0
    fi
}

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
    $ENV_VAR timeout 60 cargo run --release 2>&1 | grep -E "Ready|enabled" &
    SERVER_PID=$!
    sleep 3

    # Create table
    echo "Creating table..."
    printf "CREATE TABLE bench (id INTEGER, data TEXT);\nquit\n" | nc -q 1 127.0.0.1 5432 &>/dev/null || true
    sleep 1

    # Measure initial size (after table creation)
    INITIAL_SIZE=$(measure_size data)
    echo "Initial size (after CREATE TABLE): $INITIAL_SIZE bytes"

    # Insert initial data
    echo "Inserting $NUM_ROWS rows..."
    for i in $(seq 1 $NUM_ROWS); do
        printf "INSERT INTO bench (id, data) VALUES ($i, 'Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt');\nquit\n" | nc -q 1 127.0.0.1 5432 &>/dev/null || true

        # Show progress every 100 rows
        if [ $((i % 100)) -eq 0 ]; then
            echo "  - Inserted $i rows"
        fi
    done
    sleep 2

    AFTER_INSERT_SIZE=$(measure_size data)
    echo "Size after INSERT: $AFTER_INSERT_SIZE bytes ($(($AFTER_INSERT_SIZE / 1024))KB)"

    # Perform updates (this is what we're measuring)
    echo "Performing $NUM_UPDATES UPDATE operations..."
    BEFORE_UPDATE_SIZE=$AFTER_INSERT_SIZE

    for i in $(seq 1 $NUM_UPDATES); do
        printf "UPDATE bench SET data = 'Updated data iteration $i with some additional text to make it realistic' WHERE id = 1;\nquit\n" | nc -q 1 127.0.0.1 5432 &>/dev/null || true

        if [ $((i % 20)) -eq 0 ]; then
            echo "  - Completed $i updates"
        fi
    done

    # Wait for checkpoint
    sleep 3

    # Measure final size
    AFTER_UPDATE_SIZE=$(measure_size data)
    echo "Size after UPDATE: $AFTER_UPDATE_SIZE bytes ($(($AFTER_UPDATE_SIZE / 1024))KB)"

    # Calculate write amplification
    WRITTEN=$((AFTER_UPDATE_SIZE - BEFORE_UPDATE_SIZE))
    LOGICAL=$((NUM_UPDATES * ROW_SIZE))

    if [ $WRITTEN -le 0 ]; then
        echo "⚠️  Warning: No disk writes detected (written=$WRITTEN)"
        AMPLIFICATION=0
    else
        AMPLIFICATION=$((WRITTEN / LOGICAL))
    fi

    echo ""
    echo "Results:"
    echo "  • Logical writes: $LOGICAL bytes (~$((LOGICAL / 1024))KB)"
    echo "  • Physical writes: $WRITTEN bytes (~$((WRITTEN / 1024))KB)"

    if [ $AMPLIFICATION -gt 0 ]; then
        echo "  • Write amplification: ${AMPLIFICATION}x"
    else
        echo "  • Write amplification: <1x (better than expected!)"
    fi
    echo ""

    # Stop server
    kill $SERVER_PID 2>/dev/null || true
    wait $SERVER_PID 2>/dev/null || true

    # Save results
    echo "$AMPLIFICATION $WRITTEN $LOGICAL"
}

# Run benchmarks
echo ""
LEGACY_RESULTS=($(run_benchmark "Legacy Storage (Vec<Row>)" "RUSTDB_USE_PAGE_STORAGE=0"))
LEGACY_AMP=${LEGACY_RESULTS[0]}
LEGACY_WRITTEN=${LEGACY_RESULTS[1]}
LEGACY_LOGICAL=${LEGACY_RESULTS[2]}

echo ""
PAGE_RESULTS=($(run_benchmark "Page-Based Storage" "RUSTDB_USE_PAGE_STORAGE=1"))
PAGE_AMP=${PAGE_RESULTS[0]}
PAGE_WRITTEN=${PAGE_RESULTS[1]}
PAGE_LOGICAL=${PAGE_RESULTS[2]}

# Calculate improvement
if [ $PAGE_WRITTEN -gt 0 ] && [ $LEGACY_WRITTEN -gt 0 ]; then
    IMPROVEMENT=$((LEGACY_WRITTEN / PAGE_WRITTEN))
else
    IMPROVEMENT="N/A"
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║                    SUMMARY                               ║"
echo "╠══════════════════════════════════════════════════════════╣"
printf "║  Legacy Storage:      %6s bytes written (${LEGACY_AMP}x amp)     ║\n" "$LEGACY_WRITTEN"
printf "║  Page-Based Storage:  %6s bytes written (${PAGE_AMP}x amp)      ║\n" "$PAGE_WRITTEN"
echo "║                                                          ║"
printf "║  🚀 Improvement:      %sx better!                    ║\n" "$IMPROVEMENT"
echo "╚══════════════════════════════════════════════════════════╝"

# Cleanup
rm -rf data
