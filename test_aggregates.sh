#!/bin/bash

# Test script for aggregate functions: COUNT, SUM, AVG, MIN, MAX

set -e

echo "Starting RustDB server in background..."
cargo build --release --quiet
cargo run --release &
SERVER_PID=$!

# Wait for server to start
sleep 2

echo "Testing aggregate functions..."

# Test 1: Create table and insert data
echo "Test 1: Creating table and inserting test data..."
printf "CREATE TABLE employees (id INTEGER PRIMARY KEY, name TEXT NOT NULL, salary INTEGER, department TEXT);\n" | nc -w 1 127.0.0.1 5432 > /dev/null

printf "INSERT INTO employees (id, name, salary, department) VALUES (1, 'Alice', 50000, 'Engineering');\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO employees (id, name, salary, department) VALUES (2, 'Bob', 60000, 'Engineering');\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO employees (id, name, salary, department) VALUES (3, 'Charlie', 55000, 'Sales');\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO employees (id, name, salary, department) VALUES (4, 'Diana', 70000, 'Engineering');\n" | nc -w 1 127.0.0.1 5432 > /dev/null
printf "INSERT INTO employees (id, name, salary, department) VALUES (5, 'Eve', 45000, 'Sales');\n" | nc -w 1 127.0.0.1 5432 > /dev/null

echo "✓ Data inserted"

# Test 2: COUNT(*)
echo ""
echo "Test 2: COUNT(*) - всего сотрудников"
printf "SELECT COUNT(*) FROM employees;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 3: COUNT(column)
echo ""
echo "Test 3: COUNT(salary) - количество ненулевых зарплат"
printf "SELECT COUNT(salary) FROM employees;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 4: SUM
echo ""
echo "Test 4: SUM(salary) - общая сумма зарплат"
printf "SELECT SUM(salary) FROM employees;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 5: AVG
echo ""
echo "Test 5: AVG(salary) - средняя зарплата"
printf "SELECT AVG(salary) FROM employees;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 6: MIN
echo ""
echo "Test 6: MIN(salary) - минимальная зарплата"
printf "SELECT MIN(salary) FROM employees;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 7: MAX
echo ""
echo "Test 7: MAX(salary) - максимальная зарплата"
printf "SELECT MAX(salary) FROM employees;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 8: COUNT with WHERE
echo ""
echo "Test 8: COUNT(*) WHERE salary > 50000 - сотрудники с зарплатой > 50k"
printf "SELECT COUNT(*) FROM employees WHERE salary > 50000;\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 9: AVG with WHERE
echo ""
echo "Test 9: AVG(salary) WHERE department = 'Engineering' - средняя зарплата инженеров"
printf "SELECT AVG(salary) FROM employees WHERE department = 'Engineering';\nquit\n" | nc -w 1 127.0.0.1 5432

# Test 10: Multiple aggregates
echo ""
echo "Test 10: Несколько агрегатов одновременно"
printf "SELECT COUNT(*), SUM(salary), AVG(salary), MIN(salary), MAX(salary) FROM employees;\nquit\n" | nc -w 1 127.0.0.1 5432

# Cleanup
echo ""
echo "Cleaning up..."
kill $SERVER_PID 2>/dev/null || true
wait $SERVER_PID 2>/dev/null || true

# Remove test database
rm -f data/main.db data/main.json data/wal/*.wal 2>/dev/null || true

echo ""
echo "✓ All aggregate tests completed successfully!"
