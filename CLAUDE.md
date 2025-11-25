# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RustDB - упрощенная PostgreSQL-подобная БД на Rust. TCP сервер на порту 5432, сохранение в **binary формате** (`./data/*.db`). Поддерживает SQL, транзакции, WAL, **FOREIGN KEY**, **JOIN**, **SERIAL/BIGSERIAL**, **23 типа данных** (45% PostgreSQL compatibility), красивый вывод таблиц.

## Быстрая навигация

**Запуск:**
```bash
cargo run --release              # Сервер (порт 5432)
cargo run --example cli          # CLI клиент (интерактивный)
cargo test                       # 66+ юнит-тестов (включая WAL, FK, SERIAL, types)
./test_features.sh               # Интеграционные тесты
./test_fk_join.sh                # Тесты FK, JOIN, SERIAL
./test_new_types.sh              # Тесты всех 23 типов данных ✨
printf "\\\\dt\nquit\n" | nc 127.0.0.1 5432  # Быстрый тест через netcat (psql-style)
```

**Структура:**
- `src/main.rs` - точка входа, создает Server
- `src/server.rs` - TCP сервер, обработка подключений, **двойной протокол** (text + PostgreSQL), транзакции
- `src/pg_protocol.rs` - **PostgreSQL wire protocol** (новое!)
- `src/parser.rs` - SQL парсер (nom), **включает BEGIN/COMMIT/ROLLBACK**
- `src/executor.rs` - выполнение запросов
- `src/types.rs` - Database, Table, Row, Value (с MVCC поддержкой)
- `src/storage.rs` - сохранение/загрузка binary + **автоматический WAL**
- `src/transaction.rs` - snapshot-based транзакции
- `src/transaction_manager.rs` - **MVCC transaction ID manager** (новое!)
- `src/wal.rs` - **Write-Ahead Log (WAL) система**
- `examples/cli.rs` - **CLI с rustyline (history, arrows)** (обновлено!)
- `examples/pg_test.rs` - **тестовый PostgreSQL клиент** (новое!)

## Критически важные моменты

### 1. Мета-команды (psql-совместимость) - РЕАЛИЗОВАНО ✅
**Статус:** Поддержка psql-style команд + MySQL-style для совместимости
**Парсер:** `src/parser.rs` функции `show_tables()`, `show_users()`, `show_databases()`
**Executor:** `src/executor.rs` функция `show_tables()`

**Поддерживаемые команды:**
- `\dt` или `\d` или `SHOW TABLES` - список таблиц в текущей БД
- `\l` или `SHOW DATABASES` - список баз данных (частично)
- `\du` или `SHOW USERS` - список пользователей (частично)

**Примеры:**
```sql
\dt                  -- psql-style (рекомендуется)
SHOW TABLES;         -- MySQL-style (обратная совместимость)
```

**Промпт:** `postgrustql>`
**Вывод:** Красиво отформатированная таблица или "No tables found"

**PostgreSQL-совместимый синтаксис:**
- `CREATE DATABASE name WITH OWNER username` - правильный PostgreSQL синтаксис ✅
- `CREATE DATABASE name OWNER username` - также поддерживается (обратная совместимость)

### 2. Binary форматы - РЕАЛИЗОВАНО
**Статус:** Работает, экономия 85-90% размера!
**Формат WAL:** Binary (bincode) - `[4 bytes length][N bytes data]`
**Формат Snapshot:** Binary (bincode) - `.db` файлы вместо `.json`
**Преимущества:**
- Компактность: 220 bytes binary vs 1546 bytes JSON (86% экономии!)
- Скорость: быстрее сериализация/десериализация
- Обратная совместимость: fallback на `.json` если `.db` не найден

**Пример:**
```bash
# Старый формат (JSON)
data/main.json  # 1546 bytes

# Новый формат (Binary)
data/main.db    # 220 bytes (86% экономии!)
```

### 3. WAL (Write-Ahead Log) - ПОЛНОСТЬЮ РЕАЛИЗОВАНО ✅
**Статус:** Автоматическое WAL логирование с условными checkpoint'ами
**Файлы:** `src/wal.rs` (380 строк), `src/storage.rs` (интеграция)
**Директория:** `./data/wal/*.wal` - append-only binary лог-файлы

**Что РАБОТАЕТ:**
- ✅ Binary WAL формат (bincode)
- ✅ Автоматическое логирование CREATE/INSERT/UPDATE/DELETE
- ✅ Checkpoint только каждые 100 операций (настраивается)
- ✅ Rotation файлов при 1MB
- ✅ Cleanup старых логов (оставляет последние 2)
- ✅ Recovery механизм (crash recovery работает!)

**Архитектура:**
```
Операция → WAL log → Операция в памяти → (Каждые 100 операций) → Snapshot + WAL cleanup
```

**Механизм checkpoint:**
- `storage.operations_since_snapshot` - счетчик операций
- `storage.snapshot_threshold = 100` - порог для checkpoint
- При операции: логируется в WAL, счетчик ++
- При достижении порога: создается `.db` snapshot, старые WAL удаляются

**Восстановление после краша:**
1. Загружается последний `.db` snapshot (если есть)
2. Применяются все операции из WAL файлов
3. База полностью восстанавливается

### 4. Транзакции - БАЗОВАЯ РЕАЛИЗАЦИЯ
**Статус:** Работает для одного подключения, есть ограничения
**Где:** `src/transaction.rs` (35 строк), `src/server.rs:90-138`

**Что РАБОТАЕТ:**
- ✅ BEGIN - создаёт snapshot базы
- ✅ COMMIT - применяет изменения, сохраняет на диск
- ✅ ROLLBACK - откатывает к snapshot
- ✅ Операции вне транзакции сохраняются сразу

**Механизм:** Snapshot Isolation
```rust
BEGIN   → snapshot = db.clone()  // Клонирует всю БД
UPDATE  → изменения в db         // Модифицирует основную БД
COMMIT  → snapshot = None        // Очищает snapshot
ROLLBACK → db = snapshot         // Восстанавливает из snapshot
```

**ОГРАНИЧЕНИЯ (важно!):**
- ❌ **Нет изоляции между подключениями** - другие подключения видят незакоммиченные изменения
- ❌ **Клонирование всей БД** - при BEGIN копируется вся Database (медленно для больших БД)
- ❌ **Race conditions** - между BEGIN и операциями другие могут изменить БД
- ❌ **Не ACID** - нет полноценной атомарности между подключениями

**Вывод:** Это упрощённая Snapshot Isolation для учебных целей, не production-ready

### 5. MVCC (Multi-Version Concurrency Control) - РЕАЛИЗОВАНО ✅
**Статус:** Read Committed isolation level
**Файлы:** `src/types.rs` (Row с xmin/xmax), `src/transaction_manager.rs`, `src/executor.rs`

**Что РАБОТАЕТ:**
- ✅ Transaction ID management (атомарный счетчик)
- ✅ Row versioning: каждая строка имеет `xmin` (created by) и `xmax` (deleted by)
- ✅ Visibility rules: `row.is_visible(current_tx_id)`
- ✅ UPDATE создает новую версию строки (не удаляет старую)
- ✅ DELETE помечает `xmax` (не удаляет физически)

**Архитектура:**
```rust
pub struct Row {
    pub values: Vec<Value>,
    pub xmin: u64,           // Transaction ID that created this row
    pub xmax: Option<u64>,   // Transaction ID that deleted this row
}

// Visibility rule
fn is_visible(&self, current_tx_id: u64) -> bool {
    self.xmin <= current_tx_id && self.xmax.map_or(true, |xmax| xmax > current_tx_id)
}
```

**Ограничения:**
- ⚠️  Старые версии строк не удаляются автоматически (нет VACUUM)
- ⚠️  Read Committed isolation (не Serializable)

### 6. PostgreSQL Wire Protocol - РЕАЛИЗОВАНО ✅
**Статус:** Полностью рабочий PostgreSQL 3.0 protocol
**Файлы:** `src/pg_protocol.rs` (320 строк), `src/server.rs` (auto-detection)

**Что РАБОТАЕТ:**
- ✅ Protocol version 3.0 (196608)
- ✅ Автоматическое определение протокола (peek first 8 bytes)
- ✅ StartupMessage, AuthenticationOk, ParameterStatus
- ✅ Simple Query Protocol (Query message)
- ✅ RowDescription, DataRow, CommandComplete
- ✅ ErrorResponse с SQLSTATE кодами
- ✅ ReadyForQuery с transaction status (I/T/E)
- ✅ Transaction support (BEGIN/COMMIT/ROLLBACK)

**Как работает detection:**
```rust
// server.rs:53-75
async fn handle_client_auto(socket: TcpStream, ...) {
    let mut peek_buf = [0u8; 8];
    socket.peek(&mut peek_buf).await?;

    let length = i32::from_be_bytes([peek_buf[0..4]]);
    let version = i32::from_be_bytes([peek_buf[4..8]]);

    if length > 0 && length < 10000 && version == 196608 {
        handle_postgres_client(...)  // PostgreSQL protocol
    } else {
        handle_text_client(...)       // Text protocol
    }
}
```

**Тестирование:**
```bash
# С psql (если установлен)
psql -h 127.0.0.1 -p 5432 -U rustdb -d main

# С тестовым клиентом
cargo run --example pg_test

# Text protocol (backwards compatible)
printf "SELECT * FROM users;\nquit\n" | nc 127.0.0.1 5432
```

**Message flow:**
1. Client → StartupMessage (user, database)
2. Server → AuthenticationOk (0 = trust auth)
3. Server → ParameterStatus (server_version, encoding)
4. Server → ReadyForQuery ('I' = idle)
5. Client → Query ('Q' + SQL string)
6. Server → RowDescription (column names, types)
7. Server → DataRow[] (result rows)
8. Server → CommandComplete (tag with row count)
9. Server → ReadyForQuery ('I' или 'T' = in transaction)

### 7. CLI клиент (`examples/cli.rs`) - ОБНОВЛЕНО ✅
**Статус:** Полностью переписан с rustyline
**Библиотека:** rustyline 14.0

**Что РАБОТАЕТ:**
- ✅ История команд (↑/↓ arrows)
- ✅ Редактирование строки (←/→ arrows, Home/End, Ctrl+A/E)
- ✅ Персистентная история в `~/.rustdb_history`
- ✅ Правильный exit/quit (больше не требует Ctrl+C)
- ✅ Ctrl+C и Ctrl+D обработка
- ✅ Работает с pipe: `printf "commands\n" | cargo run --example cli`

**Альтернативы:**
- `nc 127.0.0.1 5432` - для скриптов (быстрее)
- `psql -h 127.0.0.1 -p 5432` - если установлен PostgreSQL client

### 8. Форматирование таблиц
**Библиотека:** comfy-table 7.1
**Где:** `src/server.rs:150-172` функция `format_result()`
**Preset:** UTF8_FULL для красивых box-drawing символов
**Применяется к:** SELECT и SHOW TABLES результатам

### 9. FOREIGN KEY - РЕАЛИЗОВАНО ✅
**Статус:** Полная поддержка referential integrity
**Файлы:** `src/types.rs` (ForeignKey struct), `src/parser.rs` (parsing), `src/executor.rs` (validation)

**Что РАБОТАЕТ:**
- ✅ Синтаксис `REFERENCES table(column)`
- ✅ Валидация при CREATE TABLE (referenced table/column должны существовать)
- ✅ Валидация при INSERT (значение должно существовать в referenced table)
- ✅ Referenced column должен быть PRIMARY KEY
- ✅ NULL values разрешены в FK колонках (если nullable)

**Пример:**
```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id),
    product TEXT NOT NULL
);
INSERT INTO users VALUES (1, 'Alice');
INSERT INTO orders VALUES (1, 1, 'Laptop');  -- ✓ OK
INSERT INTO orders VALUES (2, 99, 'Mouse');  -- ✗ FK violation
```

### 10. JOIN операции - РЕАЛИЗОВАНО ✅
**Статус:** INNER, LEFT, RIGHT JOIN работают
**Файлы:** `src/parser.rs` (JoinClause, JoinType), `src/executor.rs` (select_with_join)

**Что РАБОТАЕТ:**
- ✅ INNER JOIN - только совпадающие строки
- ✅ LEFT JOIN - все строки из левой таблицы + NULLs
- ✅ RIGHT JOIN - все строки из правой таблицы + NULLs
- ✅ JOIN (alias для INNER JOIN)
- ✅ MVCC visibility support

**Синтаксис:**
```sql
SELECT * FROM table1 [INNER|LEFT|RIGHT] JOIN table2 ON table1.col = table2.col;
```

**Пример:**
```sql
SELECT * FROM users INNER JOIN orders ON users.id = orders.user_id;
SELECT * FROM users LEFT JOIN orders ON users.id = orders.user_id;
```

**Ограничения:**
- Только один JOIN за запрос (пока нет chaining)
- WHERE с JOIN пока не поддерживается
- Column selection пока не реализован (возвращает все колонки)

### 11. SERIAL (auto-increment) - РЕАЛИЗОВАНО ✅
**Статус:** PostgreSQL-like SERIAL type
**Файлы:** `src/types.rs` (DataType::Serial, Table.sequences), `src/parser.rs`, `src/executor.rs`

**Что РАБОТАЕТ:**
- ✅ Автоматически PRIMARY KEY и NOT NULL
- ✅ Auto-increment начиная с 1
- ✅ Не нужно указывать id в INSERT
- ✅ Sequence обновляется правильно при explicit вставке
- ✅ Работает с FOREIGN KEY

**Синтаксис:**
```sql
CREATE TABLE users (id SERIAL, name TEXT NOT NULL);
```

**Пример:**
```sql
CREATE TABLE users (id SERIAL, name TEXT NOT NULL);
INSERT INTO users (name) VALUES ('Alice');  -- id=1
INSERT INTO users (name) VALUES ('Bob');    -- id=2
SELECT * FROM users;
-- id=1, name=Alice
-- id=2, name=Bob
```

**Архитектура:**
- `Table.sequences: HashMap<String, i64>` - хранит текущее значение sequence
- При INSERT: если SERIAL column = NULL → подставляется sequence value
- После INSERT: `sequence = max(current_seq, inserted_value + 1)`

### 12. Расширенные типы данных - РЕАЛИЗОВАНО ✅
**Статус:** 23 типа данных (~45% PostgreSQL compatibility)
**Файлы:** `src/types.rs` (Value, DataType), `src/parser.rs` (smart parsing), `src/executor.rs` (validation)
**Тестирование:** `./test_new_types.sh` - полный тест всех типов

**Поддерживаемые типы (18 новых):**

**Числовые типы:**
- ✅ `SMALLINT` - 16-bit integer (-32768 to 32767)
- ✅ `INTEGER` / `INT` - 64-bit integer
- ✅ `BIGINT` - alias для INTEGER
- ✅ `SERIAL` - auto-increment INTEGER
- ✅ `BIGSERIAL` - auto-increment BIGINT
- ✅ `REAL` / `FLOAT` - floating point (f64)
- ✅ `NUMERIC(p,s)` / `DECIMAL(p,s)` - arbitrary precision decimals (rust_decimal)

**Строковые типы:**
- ✅ `TEXT` - unlimited text
- ✅ `VARCHAR(n)` - variable length with max limit + validation
- ✅ `CHAR(n)` - fixed length with automatic space padding

**Дата/Время:**
- ✅ `DATE` - date only ('2025-01-15', format: YYYY-MM-DD)
- ✅ `TIMESTAMP` - datetime without timezone ('2025-01-15 14:30:00')
- ✅ `TIMESTAMPTZ` - datetime with timezone (RFC3339 format)

**Специальные типы:**
- ✅ `BOOLEAN` / `BOOL` - true/false
- ✅ `UUID` - universal unique identifier (uuid crate)
- ✅ `JSON` - JSON data as text
- ✅ `JSONB` - binary JSON (stored same as JSON for now)
- ✅ `BYTEA` - binary data (hex encoding: \x48656c6c6f)
- ✅ `ENUM` - user-defined enumerated types via CREATE TYPE

**Smart Value Parsing:**
Парсер автоматически определяет типы по формату значения:
- `'550e8400-...'` → UUID
- `'2025-01-15'` → DATE
- `'2025-01-15 14:30:00'` → TIMESTAMP
- `123.45` → NUMERIC (exact precision) или REAL
- `100` → SMALLINT если -32768..32767, иначе INTEGER
- `'text'` → TEXT

**Type Validation:**
- VARCHAR(n): проверка длины при INSERT, ошибка если превышает max_length
- CHAR(n): автоматическое заполнение пробелами до fixed length
- ENUM: валидация что значение входит в allowed values

**Примеры:**
```sql
-- Numeric types
CREATE TABLE test (small SMALLINT, big BIGSERIAL, price NUMERIC(10,2));
INSERT INTO test VALUES (100, NULL, 123.45);

-- String types with validation
CREATE TABLE users (username VARCHAR(20), code CHAR(5));
INSERT INTO users VALUES ('john_doe', 'ABC');  -- code padded to 'ABC  '

-- Date/Time types
CREATE TABLE events (event_date DATE, created_at TIMESTAMP);
INSERT INTO events VALUES ('2025-01-15', '2025-01-15 14:30:00');

-- UUID and JSON
CREATE TABLE sessions (id UUID, metadata JSON);
INSERT INTO sessions VALUES ('550e8400-e29b-41d4-a716-446655440000', '{"key":"value"}');

-- ENUM types
CREATE TYPE mood AS ENUM ('happy', 'sad', 'neutral');
CREATE TABLE person (name TEXT, current_mood mood);
INSERT INTO person VALUES ('Alice', 'happy');  -- ✓ OK
INSERT INTO person VALUES ('Bob', 'excited');  -- ✗ Error: not in enum
```

**Зависимости:**
```toml
chrono = "0.4"           # Date/Time types
uuid = "1.6"             # UUID type
rust_decimal = "1.33"    # NUMERIC/DECIMAL (exact precision)
hex = "0.4"              # Binary data display
```

**Не реализовано (низкий приоритет):**
- ARRAY types (INTEGER[], TEXT[])
- Geometric types (POINT, LINE, POLYGON)
- Network types (INET, CIDR, MACADDR)
- Range types (INT4RANGE, TSRANGE)
- XML, MONEY types

## Архитектура данных

### Поток выполнения запроса:
1. TCP connection → `server.rs:handle_client()`
2. Parse query → `parser::parse_statement()` → `Statement` enum
3. Проверка транзакции (BEGIN/COMMIT/ROLLBACK обрабатываются в сервере)
4. Выполнение → `executor::QueryExecutor::execute()` → `QueryResult`
5. Форматирование → `server::format_result()` → UTF-8 таблица
6. Персистентность → `storage.save_database()` → создаёт binary checkpoint
   - Snapshot: `data/main.db` (bincode)
   - WAL markers: `data/wal/*.wal` (checkpoint только)

### Состояние транзакции (per-connection):
```rust
// src/server.rs:62
let mut transaction = Transaction::new();

// BEGIN - создает снимок
transaction.begin(&db);  // клонирует Database

// COMMIT - очищает снимок, сохраняет на диск
transaction.commit();
storage.save_database(&db)?;

// ROLLBACK - восстанавливает из снимка
transaction.rollback(&mut db);
```

## Частые задачи

### Добавить новую SQL команду:
1. **parser.rs:** Добавить вариант в `Statement` enum
2. **parser.rs:** Написать функцию-парсер (nom)
3. **parser.rs:** Добавить в `alt()` в `parse_statement()`
4. **executor.rs:** Добавить `match` arm в `QueryExecutor::execute()`
5. **parser.rs:** Добавить тесты

### Исправить баг в CLI:
- **Файл:** `examples/cli.rs`
- **Проверить:** Промпты показываются? (строки 28-30, 88-90)
- **Проверить:** Чтение ответа до промпта `>` (строка 78)
- **Тест:** `cargo run --example cli` после `cargo run --release`

### Изменить формат вывода:
- **Файл:** `src/server.rs:150-172`
- **Библиотека:** comfy-table
- **Текущий preset:** UTF8_FULL
- **Альтернативы:** ASCII_FULL, UTF8_BORDERS_ONLY

## Тестирование

**Юнит-тесты (66+):**
- types.rs: 13 тестов (Value, Table, Database)
- storage.rs: 9 тестов (save/load, tempfile, **WAL crash recovery**, checkpoint)
- executor.rs: 30+ тестов (все операции + условия + aggregates + group by)
- parser.rs: 3 теста (CREATE, INSERT, SELECT)
- wal.rs: 5 тестов (append, read, apply, recovery, cleanup)

**Интеграционные:**
```bash
./test_features.sh    # Полный тест: таблицы, транзакции, персистентность
./test_fk_join.sh     # FK, JOIN, SERIAL
./test_serial.sh      # Подробные SERIAL тесты
./test_serial_quick.sh # Быстрый SERIAL тест
```

## Ограничения (что НЕ реализовано)

- Множественные JOIN (только один JOIN за запрос)
- WHERE с JOIN
- Column selection в JOIN (возвращает все колонки)
- ON DELETE CASCADE / ON UPDATE CASCADE для FK
- Индексы
- Подготовленные запросы
- Аутентификация (trust mode)
- VACUUM для старых версий строк (MVCC)

## Потенциальные баги / области для улучшения

1. **Транзакции:**
   - Snapshot клонирует весь Database - может быть медленно для больших БД
   - Нет изоляции между подключениями (snapshot isolation только внутри одной транзакции)
   - При ROLLBACK в `transaction.rs:20-23` делается `*db = snapshot` - требует Clone

2. **CLI клиент:**
   - ✅ ~~При использовании pipe может зависнуть~~ **ИСПРАВЛЕНО!**
   - ✅ ~~Нет истории команд~~ **ИСПРАВЛЕНО!** - rustyline с историей
   - ✅ ~~exit/quit требует Ctrl+C~~ **ИСПРАВЛЕНО!** - правильный exit

3. **Парсер:**
   - Строки только в одинарных кавычках `'text'`
   - Нет экранирования кавычек внутри строк
   - Нет поддержки NULL в INSERT (только через явный NULL keyword)

4. **Storage:**
   - ✅ ~~JSON может быть большим~~ **РЕШЕНО** - binary формат (bincode)
   - ✅ ~~Нет инкрементального сохранения~~ **РЕШЕНО** - WAL + checkpoint каждые 100 операций
   - ⚠️  Нет компрессии данных

5. **WAL:**
   - ✅ ~~Методы `log_*` НЕ интегрированы~~ **ИСПРАВЛЕНО!** - автоматическое логирование
   - ✅ ~~Операции не логируются~~ **ИСПРАВЛЕНО!** - все операции в WAL
   - ✅ ~~WAL файлы в JSON~~ **ИСПРАВЛЕНО** - binary (bincode)
   - ✅ ~~Checkpoint после КАЖДОЙ операции~~ **ИСПРАВЛЕНО** - каждые 100 операций
   - ✅ ~~Нет MVCC~~ **ИСПРАВЛЕНО!** - полноценный MVCC

6. **MVCC:**
   - ⚠️  Нет автоматического VACUUM (старые версии строк не удаляются)
   - ⚠️  Read Committed isolation level (не Serializable)

## Зависимости

```toml
tokio = "1.41"           # async runtime
nom = "7.1"              # SQL parsing
serde = "1.0"            # сериализация
serde_json = "1.0"       # JSON (legacy, для обратной совместимости)
bincode = "1.3"          # binary сериализация
thiserror = "2.0"        # error handling
comfy-table = "7.1"      # table formatting
bytes = "1.9"            # byte buffers для PostgreSQL protocol
rustyline = "14.0"       # CLI с историей и редактированием
dirs = "5.0"             # поиск home directory для истории
chrono = "0.4"           # Date/Time types (DATE, TIMESTAMP, TIMESTAMPTZ) ✨
uuid = "1.6"             # UUID type ✨
rust_decimal = "1.33"    # NUMERIC/DECIMAL (exact precision) ✨
hex = "0.4"              # Binary data display (BYTEA) ✨
tempfile = "3.8"         # для тестов (dev-dependency)
```

## Советы для разработки

- **При изменении Statement enum:** Обязательно обновить `executor.rs:13-42` match
- **При добавлении тестов storage:** Использовать `tempfile::TempDir` (см. `storage.rs:57+`)
- **При отладке транзакций:** Смотреть `server.rs:90-138`, там вся логика
- **Если CLI не показывает промпт:** Проверить `examples/cli.rs:28-30` и `:88-90`
- **При проблемах с форматированием:** Проверить `server.rs:158` - `ComfyTable::new()`

## Быстрый старт для новой сессии

```bash
# 1. Проверить что все работает
cargo test --quiet && echo "Tests OK"

# 2. Запустить интеграционный тест
./test_features.sh

# 3. Проверить CLI вручную
cargo run --release &  # Terminal 1
sleep 2
cargo run --example cli  # Terminal 2
# Ввести: SELECT * FROM test; (если есть таблица)
# quit

# 4. Убить сервер
pkill rustdb
```

Если что-то сломано - начинать с `cargo test` и смотреть какие тесты падают.
