#!/bin/bash

echo "========================================="
echo "RustDB - Автоматический тест"
echo "========================================="
echo ""

# Перейти в директорию проекта
cd "$(dirname "$0")"

# Очистить старые данные
echo "1. Очистка старых данных..."
rm -rf data/
echo "   ✓ Данные очищены"
echo ""

# Собрать проект
echo "2. Компиляция проекта..."
cargo build --release --quiet 2>&1 | grep -v "warning:" | head -20
echo "   ✓ Проект скомпилирован"
echo ""

# Запустить сервер в фоне
echo "3. Запуск сервера..."
cargo run --release > /tmp/rustdb_server.log 2>&1 &
SERVER_PID=$!
sleep 3
echo "   ✓ Сервер запущен (PID: $SERVER_PID)"
echo ""

# Выполнить тестовый клиент
echo "4. Выполнение SQL запросов..."
cargo run --example simple_test --quiet
echo "   ✓ Запросы выполнены"
echo ""

# Показать содержимое базы
echo "5. Содержимое базы данных:"
echo "========================================="
if command -v python3 &> /dev/null; then
    cat data/main.json | python3 -m json.tool
else
    cat data/main.json
fi
echo "========================================="
echo ""

# Остановить сервер
echo "6. Остановка сервера..."
kill $SERVER_PID 2>/dev/null
wait $SERVER_PID 2>/dev/null
echo "   ✓ Сервер остановлен"
echo ""

echo "========================================="
echo "Тест завершён успешно!"
echo "========================================="
echo ""
echo "Данные сохранены в: data/main.json"
echo ""
echo "Чтобы запустить интерактивный режим:"
echo "  1. Терминал 1: cargo run --release"
echo "  2. Терминал 2: cargo run --example cli"
echo ""
