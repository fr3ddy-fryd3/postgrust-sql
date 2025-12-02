#!/bin/bash
# Benchmark with forced checkpoint (100+ operations)

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     Write Amplification Benchmark                        ║"
echo "║     (With Checkpoint Trigger)                            ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

cargo build --release --quiet 2>/dev/null

echo "Strategy: Create 500 rows, wait for checkpoint (>100 ops)"
echo ""

test_mode() {
    local MODE=$1
    local ENV=$2

    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Testing: $MODE"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

    rm -rf data
    mkdir -p data

    # Start server
    echo "Starting server..."
    $ENV timeout 30 cargo run --release 2>&1 | grep -E "Ready|enabled|Checkpoint" &
    local PID=$!
    sleep 2

    # Initial checkpoint baseline
    printf "CREATE TABLE bench (id INTEGER, name TEXT, value INTEGER);\n" | nc -q 1 127.0.0.1 5432 &>/dev/null
    sleep 1
    local BASELINE=$(du -sb data 2>/dev/null | cut -f1)
    echo "Baseline size: $BASELINE bytes"

    # Insert 500 rows (will trigger checkpoint after 100)
    echo "Inserting 500 rows..."
    for i in $(seq 1 500); do
        printf "INSERT INTO bench VALUES ($i, 'name$i', $i);\n" | nc -q 1 127.0.0.1 5432 &>/dev/null

        if [ $((i % 100)) -eq 0 ]; then
            echo "  - Inserted $i rows"
        fi
    done
    sleep 3  # Wait for checkpoint

    local AFTER_INSERT=$(du -sb data 2>/dev/null | cut -f1)
    echo "After 500 inserts: $AFTER_INSERT bytes (~$((AFTER_INSERT / 1024))KB)"

    # Now update 1 row and do 99 more SELECTs to trigger checkpoint
    echo "Updating 1 row + forcing checkpoint..."
    printf "UPDATE bench SET value = 9999 WHERE id = 1;\n" | nc -q 1 127.0.0.1 5432 &>/dev/null

    # Do 99 more operations to hit 100 ops checkpoint
    for i in $(seq 1 99); do
        printf "SELECT COUNT(*) FROM bench;\n" | nc -q 1 127.0.0.1 5432 &>/dev/null
    done
    sleep 3

    local AFTER_UPDATE=$(du -sb data 2>/dev/null | cut -f1)
    echo "After 1 update + checkpoint: $AFTER_UPDATE bytes (~$((AFTER_UPDATE / 1024))KB)"

    local CHECKPOINT_WRITE=$((AFTER_UPDATE - AFTER_INSERT))
    echo ""
    echo "Result:"
    echo "  Logical change: ~100 bytes (1 row update)"
    echo "  Physical write: $CHECKPOINT_WRITE bytes (~$((CHECKPOINT_WRITE / 1024))KB)"

    if [ $CHECKPOINT_WRITE -gt 0 ]; then
        local AMP=$((CHECKPOINT_WRITE / 100))
        echo "  Write amplification: ~${AMP}x"
    else
        echo "  Write amplification: <1x"
    fi
    echo ""

    # List files
    echo "Database files:"
    ls -lh data/*.db data/*.wal 2>/dev/null | awk '{print "  ", $5, $9}' || echo "  No files found"
    echo ""

    kill $PID 2>/dev/null
    wait $PID 2>/dev/null

    echo $CHECKPOINT_WRITE
}

echo ""
LEGACY_WRITE=$(test_mode "Legacy Storage (Vec<Row>)" "RUSTDB_USE_PAGE_STORAGE=0")

PAGE_WRITE=$(test_mode "Page-Based Storage" "RUSTDB_USE_PAGE_STORAGE=1")

# Summary
if [ "$LEGACY_WRITE" -gt 0 ] && [ "$PAGE_WRITE" -gt 0 ]; then
    IMPROVEMENT=$((LEGACY_WRITE / PAGE_WRITE))
    echo "╔══════════════════════════════════════════════════════════╗"
    echo "║                      SUMMARY                             ║"
    echo "╠══════════════════════════════════════════════════════════╣"
    printf "║  Legacy:      %8d bytes (~%4dKB)                 ║\n" "$LEGACY_WRITE" "$((LEGACY_WRITE / 1024))"
    printf "║  Page-based:  %8d bytes (~%4dKB)                 ║\n" "$PAGE_WRITE" "$((PAGE_WRITE / 1024))"
    echo "║                                                          ║"
    printf "║  🚀 Improvement: %dx better!                          ║\n" "$IMPROVEMENT"
    echo "╚══════════════════════════════════════════════════════════╝"
else
    echo "⚠️  Could not measure improvement (no disk writes detected)"
fi

rm -rf data
