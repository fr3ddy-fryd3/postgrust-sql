#!/bin/bash

# Integration test for v1.4.0 features: OFFSET, DISTINCT, UNIQUE
# Tests all three new features together

set -e

echo "=== PostgrustQL v1.4.0 Integration Test ==="
echo

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Kill any existing server
pkill -9 postgrustql 2>/dev/null || true
sleep 1

# Start server in background
echo "Starting server..."
cargo run --release 2>&1 > /dev/null &
SERVER_PID=$!
sleep 3

# Function to run SQL and check output
run_test() {
    local test_name="$1"
    local sql="$2"
    local expected="$3"

    echo -n "Testing $test_name... "

    result=$(printf "%s\nquit\n" "$sql" | nc 127.0.0.1 5432 2>&1)

    if echo "$result" | grep -q "$expected"; then
        echo -e "${GREEN}✓ PASSED${NC}"
        return 0
    else
        echo -e "${RED}✗ FAILED${NC}"
        echo "Expected: $expected"
        echo "Got: $result"
        return 1
    fi
}

# Track test results
TESTS_PASSED=0
TESTS_FAILED=0

# Test 1: OFFSET basic functionality
if run_test "OFFSET (skip 2 rows)" \
    "CREATE TABLE test (id SERIAL, val TEXT);
INSERT INTO test (val) VALUES ('A');
INSERT INTO test (val) VALUES ('B');
INSERT INTO test (val) VALUES ('C');
INSERT INTO test (val) VALUES ('D');
INSERT INTO test (val) VALUES ('E');
SELECT * FROM test OFFSET 2;" \
    "│ 3"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Test 2: OFFSET with LIMIT
if run_test "OFFSET with LIMIT" \
    "SELECT * FROM test LIMIT 2 OFFSET 1;" \
    "│ 2"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Test 3: DISTINCT on column with duplicates
if run_test "DISTINCT (3 unique from 5 rows)" \
    "DROP TABLE test;
CREATE TABLE cities (id SERIAL, name TEXT);
INSERT INTO cities (name) VALUES ('NYC');
INSERT INTO cities (name) VALUES ('LA');
INSERT INTO cities (name) VALUES ('NYC');
INSERT INTO cities (name) VALUES ('SF');
INSERT INTO cities (name) VALUES ('LA');
SELECT DISTINCT name FROM cities;" \
    "NYC.*LA.*SF" && \
    ! printf "SELECT DISTINCT name FROM cities;\nquit\n" | nc 127.0.0.1 5432 | grep -q "│ 4"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Test 4: DISTINCT with LIMIT
if run_test "DISTINCT with LIMIT" \
    "SELECT DISTINCT name FROM cities LIMIT 2;" \
    "NYC.*LA" && \
    ! printf "SELECT DISTINCT name FROM cities LIMIT 2;\nquit\n" | nc 127.0.0.1 5432 | grep -q "SF"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Test 5: UNIQUE constraint prevents duplicates
if run_test "UNIQUE constraint (prevents duplicates)" \
    "DROP TABLE cities;
CREATE TABLE users (id SERIAL, email TEXT UNIQUE NOT NULL);
INSERT INTO users (email) VALUES ('alice@test.com');
INSERT INTO users (email) VALUES ('alice@test.com');" \
    "UNIQUE constraint violation"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Test 6: UNIQUE allows different values
if run_test "UNIQUE constraint (allows different values)" \
    "INSERT INTO users (email) VALUES ('bob@test.com');
SELECT * FROM users;" \
    "alice@test.com.*bob@test.com"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Test 7: PRIMARY KEY also enforces uniqueness
if run_test "PRIMARY KEY uniqueness" \
    "DROP TABLE users;
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT);
INSERT INTO products VALUES (1, 'Laptop');
INSERT INTO products VALUES (1, 'Phone');" \
    "UNIQUE constraint violation"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Test 8: Combined test - DISTINCT + OFFSET + LIMIT
if run_test "DISTINCT + OFFSET + LIMIT combined" \
    "DROP TABLE products;
CREATE TABLE items (id SERIAL, category TEXT);
INSERT INTO items (category) VALUES ('A');
INSERT INTO items (category) VALUES ('B');
INSERT INTO items (category) VALUES ('A');
INSERT INTO items (category) VALUES ('C');
INSERT INTO items (category) VALUES ('B');
INSERT INTO items (category) VALUES ('D');
SELECT DISTINCT category FROM items LIMIT 2 OFFSET 1;" \
    "B.*C" && \
    ! printf "SELECT DISTINCT category FROM items LIMIT 2 OFFSET 1;\nquit\n" | nc 127.0.0.1 5432 | grep -q "│ A"; then
    ((TESTS_PASSED++))
else
    ((TESTS_FAILED++))
fi

# Cleanup
echo
echo "Cleaning up..."
kill $SERVER_PID 2>/dev/null || true
sleep 1

# Summary
echo
echo "=========================================="
echo "Test Results:"
echo -e "${GREEN}Passed: $TESTS_PASSED${NC}"
if [ $TESTS_FAILED -gt 0 ]; then
    echo -e "${RED}Failed: $TESTS_FAILED${NC}"
else
    echo "Failed: $TESTS_FAILED"
fi
echo "=========================================="

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${GREEN}All tests passed! ✓${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed ✗${NC}"
    exit 1
fi
