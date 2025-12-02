#!/bin/bash
# Simple benchmark to demonstrate write amplification difference

echo "╔══════════════════════════════════════════════════════════╗"
echo "║     Write Amplification: Simple Demo                     ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""

cargo build --release --quiet 2>/dev/null

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Scenario: 1000 rows, then 10 single-row updates"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

test_mode() {
    local MODE=$1
    local ENV=$2

    echo "[$MODE]"
    rm -rf data

    # Start server
    $ENV timeout 20 cargo run --release &>/dev/null &
    local PID=$!
    sleep 2

    # Create and populate
    printf "CREATE TABLE t (id INTEGER, data TEXT);\n" | nc -q 1 127.0.0.1 5432 &>/dev/null

    for i in {1..1000}; do
        printf "INSERT INTO t VALUES ($i, 'data$i');\n" | nc -q 1 127.0.0.1 5432 &>/dev/null
    done
    sleep 1

    # Measure before updates
    local SIZE_BEFORE=$(du -sb data 2>/dev/null | cut -f1)

    # Perform 10 updates
    for i in {1..10}; do
        printf "UPDATE t SET data = 'updated$i' WHERE id = 1;\n" | nc -q 1 127.0.0.1 5432 &>/dev/null
    done
    sleep 2

    # Measure after updates
    local SIZE_AFTER=$(du -sb data 2>/dev/null | cut -f1)

    kill $PID 2>/dev/null
    wait $PID 2>/dev/null

    local WRITTEN=$((SIZE_AFTER - SIZE_BEFORE))
    echo "  Disk writes for 10 updates: $WRITTEN bytes (~$((WRITTEN / 1024))KB)"

    if [ -d "data" ]; then
        echo "  Files: $(ls -lh data/*.db data/*.wal 2>/dev/null | awk '{print $5, $9}')"
    fi
    echo ""
}

test_mode "Legacy Storage" "RUSTDB_USE_PAGE_STORAGE=0"
test_mode "Page Storage" "RUSTDB_USE_PAGE_STORAGE=1"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Theory:"
echo "  • Legacy: Rewrites entire database on every checkpoint"
echo "  • Page-based: Only writes modified 8KB pages"
echo "  • Expected improvement: ~1000x for this scenario"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

rm -rf data
