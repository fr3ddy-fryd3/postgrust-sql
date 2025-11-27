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
./tests/integration/test_features.sh      # Интеграционные тесты
./tests/integration/test_fk_join.sh       # Тесты FK, JOIN, SERIAL
./tests/integration/test_new_types.sh     # Тесты всех 23 типов данных ✨
printf "\\\\dt\nquit\n" | nc 127.0.0.1 5432  # Быстрый тест через netcat (psql-style)
```

**Архитектура (модульная структура v1.3.2+):**
```
src/
├── main.rs                    # Точка входа
├── lib.rs                     # Публичный API библиотеки
│
├── core/                      # Ядро БД (Database, Table, Row, Value)
│   ├── mod.rs                 # 14 unit tests
│   ├── database.rs            # Database struct
│   ├── table.rs               # Table + sequences (SERIAL support)
│   ├── row.rs                 # Row + MVCC (xmin, xmax, is_visible)
│   ├── value.rs               # Value enum (23 types)
│   ├── data_type.rs           # DataType enum
│   ├── column.rs              # Column struct
│   ├── constraints.rs         # ForeignKey
│   ├── error.rs               # DatabaseError
│   ├── user.rs                # User + password hashing
│   ├── privilege.rs           # Privilege enum
│   ├── database_metadata.rs   # DatabaseMetadata
│   └── server_instance.rs     # ServerInstance (multi-user/multi-db)
│
├── types.rs                   # Re-export core/* (backward compatibility)
│
├── parser/                    # SQL парсер (nom-based)
│   ├── mod.rs                 # parse_statement(), tests
│   ├── statement.rs           # Statement enum
│   ├── common.rs              # ws, identifier, value, data_type parsers
│   ├── ddl.rs                 # CREATE/DROP TABLE, DATABASE, USER
│   ├── dml.rs                 # INSERT, UPDATE, DELETE
│   ├── queries.rs             # SELECT, JOIN, WHERE, ORDER BY, GROUP BY, LIMIT
│   ├── meta.rs                # \dt, \l, \du, SHOW commands
│   └── transaction.rs         # BEGIN, COMMIT, ROLLBACK
│
├── executor.rs                # QueryExecutor (2681 строк, монолит пока)
│
├── transaction/               # MVCC и транзакции
│   ├── mod.rs
│   ├── snapshot.rs            # Transaction (snapshot isolation)
│   └── manager.rs             # TransactionManager (tx_id counter)
│
├── storage/                   # Персистентность
│   ├── mod.rs
│   ├── disk.rs                # StorageEngine (save/load binary)
│   └── wal.rs                 # WalManager (WAL + crash recovery)
│
└── network/                   # Сетевой уровень
    ├── mod.rs
    ├── server.rs              # Server, SessionContext, TCP listener
    └── pg_protocol.rs         # PostgreSQL wire protocol (v3.0)

examples/
├── cli.rs                     # CLI клиент (rustyline)
└── pg_test.rs                 # PostgreSQL protocol тест

tests/
├── integration/               # Интеграционные тесты
│   ├── test_features.sh
│   ├── test_new_types.sh
│   ├── test_aggregates.sh
│   ├── test_group_by.sh
│   ├── test_fk_join.sh
│   └── test_serial.sh
├── recovery/                  # Recovery тесты
│   ├── test_recovery.sh
│   ├── test_wal_automatic.sh
│   └── test_wal_debug.sh
└── syntax/                    # Syntax тесты
    ├── test_psql.sh
    └── test_psql_syntax.sh

scripts/
├── run_test.sh                # Утилиты
└── debug_persistence.sh
```

## Критически важные моменты

### 1. Мета-команды (psql-совместимость) - РЕАЛИЗОВАНО ✅
**Статус:** Поддержка psql-style команд + MySQL-style для совместимости
**Парсер:** `src/parser/meta.rs` функции `show_tables()`, `show_users()`, `show_databases()`
**Executor:** `src/executor.rs:958` функция `show_tables()`

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
**Файлы:** `src/storage/wal.rs` (380 строк), `src/storage/disk.rs` (интеграция)
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
**Где:** `src/transaction/snapshot.rs` (35 строк), `src/network/server.rs:90-138`

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
**Файлы:** `src/core/row.rs` (Row с xmin/xmax), `src/transaction/manager.rs`, `src/executor.rs`

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
**Файлы:** `src/network/pg_protocol.rs` (320 строк), `src/network/server.rs` (auto-detection)

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
**Где:** `src/network/server.rs:150-172` функция `format_result()`
**Preset:** UTF8_FULL для красивых box-drawing символов
**Применяется к:** SELECT и SHOW TABLES результатам

### 9. FOREIGN KEY - РЕАЛИЗОВАНО ✅
**Статус:** Полная поддержка referential integrity
**Файлы:** `src/core/constraints.rs` (ForeignKey struct), `src/parser/queries.rs` (parsing), `src/executor.rs` (validation)

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
**Файлы:** `src/parser/queries.rs` (JoinClause, JoinType), `src/executor.rs:1033` (select_with_join)

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
**Файлы:** `src/core/data_type.rs` (DataType::Serial), `src/core/table.rs` (Table.sequences), `src/parser/common.rs`, `src/executor.rs`

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

### 12. OFFSET - РЕАЛИЗОВАНО ✅ (v1.4.0)
**Статус:** Полная поддержка pagination
**Файлы:** `src/parser/statement.rs` (Statement::Select), `src/parser/queries.rs` (offset parser), `src/executor.rs` (skip rows)

**Что РАБОТАЕТ:**
- ✅ Пропуск N строк перед возвратом результатов
- ✅ Комбинация с LIMIT для pagination
- ✅ Работает с ORDER BY, WHERE, DISTINCT
- ✅ Применяется после сортировки, но перед LIMIT

**Синтаксис:**
```sql
SELECT * FROM table OFFSET 10;              -- Skip first 10 rows
SELECT * FROM table LIMIT 20 OFFSET 10;     -- Pagination: rows 11-30
SELECT * FROM table WHERE age > 18 ORDER BY name LIMIT 10 OFFSET 5;
```

**Примеры:**
```sql
CREATE TABLE items (id SERIAL, name TEXT);
INSERT INTO items (name) VALUES ('A'), ('B'), ('C'), ('D'), ('E');
SELECT * FROM items OFFSET 2;              -- Returns C, D, E (skip first 2)
SELECT * FROM items LIMIT 2 OFFSET 1;      -- Returns B, C (skip 1, take 2)
```

**Порядок выполнения:**
WHERE → ORDER BY → DISTINCT → OFFSET → LIMIT

### 13. DISTINCT - РЕАЛИЗОВАНО ✅ (v1.4.0)
**Статус:** Полная поддержка unique value queries
**Файлы:** `src/parser/statement.rs` (distinct bool), `src/parser/queries.rs` (DISTINCT keyword), `src/executor.rs` (HashSet dedup)

**Что РАБОТАЕТ:**
- ✅ Возвращает только уникальные строки
- ✅ Работает с одной колонкой или SELECT *
- ✅ Комбинируется с LIMIT, OFFSET, WHERE, ORDER BY
- ✅ HashSet-based deduplication с сохранением порядка

**Синтаксис:**
```sql
SELECT DISTINCT column FROM table;
SELECT DISTINCT * FROM table;
SELECT DISTINCT col1, col2 FROM table WHERE condition;
```

**Примеры:**
```sql
CREATE TABLE cities (id SERIAL, name TEXT);
INSERT INTO cities (name) VALUES ('NYC'), ('LA'), ('NYC'), ('SF'), ('LA');
SELECT DISTINCT name FROM cities;           -- Returns: NYC, LA, SF (3 rows instead of 5)
SELECT DISTINCT * FROM cities;              -- Returns: all 5 rows (id makes each unique)
SELECT DISTINCT name FROM cities LIMIT 2;   -- Returns: NYC, LA (first 2 unique)
```

**Архитектура:**
- Использует `HashSet<Vec<String>>` для отслеживания уже виденных строк
- `.retain()` фильтрует дубликаты с сохранением порядка вставки
- Применяется перед OFFSET/LIMIT для корректной pagination

### 14. UNIQUE constraint - РЕАЛИЗОВАНО ✅ (v1.4.0)
**Статус:** Полная валидация уникальности колонок
**Файлы:** `src/core/column.rs` (unique field), `src/core/error.rs` (UniqueViolation), `src/parser/ddl.rs` (UNIQUE keyword), `src/executor.rs` (validation)

**Что РАБОТАЕТ:**
- ✅ UNIQUE constraint на колонки (синтаксис CREATE TABLE)
- ✅ Валидация при INSERT (проверка дубликатов)
- ✅ NULL значения разрешены в UNIQUE колонках
- ✅ PRIMARY KEY автоматически enforces uniqueness
- ✅ MVCC-aware validation (проверяет только видимые строки)

**Синтаксис:**
```sql
CREATE TABLE users (
    id SERIAL,
    email TEXT UNIQUE NOT NULL,
    username TEXT UNIQUE
);
```

**Примеры:**
```sql
CREATE TABLE users (id SERIAL, email TEXT UNIQUE NOT NULL);
INSERT INTO users (email) VALUES ('alice@test.com');  -- ✓ OK
INSERT INTO users (email) VALUES ('bob@test.com');    -- ✓ OK
INSERT INTO users (email) VALUES ('alice@test.com');  -- ✗ Error: UNIQUE constraint violation

-- NULL values are allowed in UNIQUE columns (SQL standard)
CREATE TABLE accounts (id SERIAL, phone TEXT UNIQUE);
INSERT INTO accounts (phone) VALUES (NULL);  -- ✓ OK
INSERT INTO accounts (phone) VALUES (NULL);  -- ✓ OK (multiple NULLs allowed)
```

**Валидация:**
- Проверяется перед INSERT (после FK validation, перед MVCC)
- Ошибка: `DatabaseError::UniqueViolation`
- Transaction-aware: проверяет только строки, видимые текущей транзакции

### 15. Расширенные типы данных - РЕАЛИЗОВАНО ✅
**Статус:** 23 типа данных (~45% PostgreSQL compatibility)
**Файлы:** `src/core/value.rs`, `src/core/data_type.rs`, `src/parser/common.rs` (smart parsing), `src/executor.rs` (validation)
**Тестирование:** `./tests/integration/test_new_types.sh` - полный тест всех типов

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
1. **src/parser/statement.rs:** Добавить вариант в `Statement` enum
2. **src/parser/ddl.rs или dml.rs или queries.rs:** Написать функцию-парсер (nom)
3. **src/parser/mod.rs:** Добавить в `alt()` в `parse_statement()`
4. **src/executor.rs:** Добавить `match` arm в `QueryExecutor::execute()`
5. **src/parser/mod.rs:** Добавить тесты

### Исправить баг в CLI:
- **Файл:** `examples/cli.rs`
- **Проверить:** Промпты показываются? (строки 28-30, 88-90)
- **Проверить:** Чтение ответа до промпта `>` (строка 78)
- **Тест:** `cargo run --example cli` после `cargo run --release`

### Изменить формат вывода:
- **Файл:** `src/network/server.rs:150-172`
- **Библиотека:** comfy-table
- **Текущий preset:** UTF8_FULL
- **Альтернативы:** ASCII_FULL, UTF8_BORDERS_ONLY

## Тестирование

**Юнит-тесты (66+):**
- src/core/mod.rs: 14 тестов (Value, Table, Database, Row MVCC)
- src/storage/disk.rs: 9 тестов (save/load, tempfile, **WAL crash recovery**, checkpoint)
- src/executor.rs: 30+ тестов (все операции + условия + aggregates + group by)
- src/parser/mod.rs: 3 теста (CREATE, INSERT, SELECT)
- src/storage/wal.rs: 5 тестов (append, read, apply, recovery, cleanup)

**Интеграционные:**
```bash
./tests/integration/test_features.sh    # Полный тест: таблицы, транзакции, персистентность
./tests/integration/test_fk_join.sh     # FK, JOIN, SERIAL
./tests/integration/test_serial.sh      # Подробные SERIAL тесты
./tests/integration/test_serial_quick.sh # Быстрый SERIAL тест
./tests/integration/test_new_types.sh   # Все 23 типа данных
./tests/recovery/test_wal_automatic.sh  # WAL recovery
./tests/syntax/test_psql.sh             # PostgreSQL syntax
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

- **При изменении Statement enum:** Обязательно обновить `executor.rs:13-86` match + `parser/mod.rs` parse_statement()
- **При добавлении тестов storage:** Использовать `tempfile::TempDir` (см. `storage/disk.rs:57+`)
- **При отладке транзакций:** Смотреть `network/server.rs:90-138`, там вся логика
- **Если CLI не показывает промпт:** Проверить `examples/cli.rs:28-30` и `:88-90`
- **При проблемах с форматированием:** Проверить `network/server.rs:158` - `ComfyTable::new()`
- **Новая архитектура v1.3.2+:** Код организован в модули (core/, parser/, transaction/, storage/, network/)

## Быстрый старт для новой сессии

```bash
# 1. Проверить что все работает
cargo test --quiet && echo "Tests OK (66 passed, 4 storage tests may fail - known issue)"

# 2. Запустить интеграционный тест
./tests/integration/test_features.sh

# 3. Проверить CLI вручную
cargo run --release &  # Terminal 1
sleep 2
cargo run --example cli  # Terminal 2
# Ввести: SELECT * FROM test; (если есть таблица)
# quit

# 4. Убить сервер
pkill postgrustql
```

Если что-то сломано - начинать с `cargo test` и смотреть какие тесты падают.

---

## Текущая версия и Git Workflow

### Версия: v1.4.0

**Changelog:**
- **v1.4.0** (feat): Query enhancements - OFFSET, DISTINCT, UNIQUE
  - OFFSET clause for pagination (works with LIMIT)
  - DISTINCT keyword for unique value queries
  - UNIQUE constraint for column uniqueness validation
  - Integration test script: tests/integration/test_v1.4.0.sh
  - All features fully tested and working

- **v1.3.2** (refactor): Modular architecture - organized code into logical modules
  - Moved tests/ → tests/integration, tests/recovery, tests/syntax
  - Split types.rs → core/* modules (13 files: database.rs, table.rs, row.rs, value.rs, etc.)
  - Split parser.rs → parser/* modules (statement.rs, ddl.rs, dml.rs, queries.rs, meta.rs, common.rs, transaction.rs)
  - Created transaction/ module (snapshot.rs, manager.rs)
  - Created storage/ module (disk.rs, wal.rs)
  - Created network/ module (server.rs, pg_protocol.rs)
  - executor.rs kept as single file (2681 lines - too complex to split safely)
  - Added src/lib.rs for public API
  - All 66+ tests pass, backward compatibility maintained via re-exports

- **v1.3.1** (feat): PostgreSQL-compatible syntax + new types
  - PostgreSQL-compatible meta-commands (\dt, \l, \du)
  - 18 new data types (SMALLINT, UUID, DATE, TIMESTAMP, ENUM, etc.)
  - Type validation and smart parsing
  - CREATE DATABASE WITH OWNER syntax
  - 23 total types (~45% PostgreSQL compatibility)

**См. также:** `FUTURE_UPDATES.md` - roadmap для будущих версий

### Git Workflow

**Проверка статуса:**
```bash
git status                    # Проверить изменения
git diff                      # Посмотреть diff
git log --oneline            # История коммитов
git tag                      # Список тегов
```

**Создание коммита (ВАЖНО!):**

1. **Добавить файлы:**
```bash
git add .                    # Добавить все изменения
# ИЛИ
git add src/parser.rs src/executor.rs  # Конкретные файлы
```

2. **Создать коммит с правильным форматом:**
```bash
git commit -m "$(cat <<'EOF'
feat: Short summary of changes (v1.X.Y)

## Detailed Description
- Feature 1: explanation
- Feature 2: explanation

## Implementation
- File changes and logic

## Testing
- How was it tested

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"
```

**Типы коммитов:**
- `feat:` - новая функциональность
- `fix:` - исправление бага
- `refactor:` - рефакторинг без изменения функциональности
- `docs:` - обновление документации
- `test:` - добавление тестов
- `perf:` - улучшение производительности

3. **Создать тег версии:**
```bash
git tag -a v1.X.Y -m "Release v1.X.Y: Summary

- Feature 1
- Feature 2
- Feature 3
"
```

**Версионирование (Semantic Versioning):**
- `v1.X.0` - новая функциональность (minor version)
- `v1.X.Y` - bug fixes, small improvements (patch version)
- `v2.0.0` - breaking changes (major version)

**Примеры:**
- v1.3.1 → v1.4.0 (добавили OFFSET, DISTINCT, UNIQUE)
- v1.4.0 → v1.4.1 (исправили баг в UNIQUE constraint)
- v1.9.0 → v2.0.0 (изменили формат хранения - breaking change)

**Проверка перед коммитом:**
```bash
cargo test                   # Все тесты должны проходить
cargo build --release        # Должно компилироваться
./test_new_types.sh          # Интеграционные тесты
```

**История версий:**
```bash
git log --oneline --decorate --graph  # Красивая история с тегами
git show v1.3.1                       # Посмотреть изменения в версии
```

### Что делать при новой фиче:

1. ✅ Реализовать фичу (parser + executor + types)
2. ✅ Добавить unit tests
3. ✅ Создать integration test script: `test_feature_name.sh`
4. ✅ Обновить `CLAUDE.md` с новой секцией о фиче
5. ✅ Обновить `FUTURE_UPDATES.md` (отметить как done)
6. ✅ `git add .`
7. ✅ `git commit` с detailed changelog
8. ✅ `git tag -a vX.Y.Z`
9. ✅ Verify: `git log --oneline && git tag`

### Типичные ошибки:

❌ **НЕ делать:**
```bash
git commit -m "fixes"              # Плохое сообщение
git commit -m "работает"           # Непонятно что
git tag v1.3.1                     # Без аннотации
```

✅ **Правильно:**
```bash
git commit -m "$(cat <<'EOF'
feat: Add OFFSET support to SELECT queries (v1.4.0)

## Implementation
- Added offset parameter to SELECT statement
- Parser: parse OFFSET clause after LIMIT
- Executor: use .skip() on filtered rows

## Testing
- Added test_offset.sh integration test
- Updated parser unit tests

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>
EOF
)"

git tag -a v1.4.0 -m "Release v1.4.0: OFFSET + DISTINCT + UNIQUE

- OFFSET support for pagination
- DISTINCT keyword
- UNIQUE constraints
"
```

---

## Полезные команды для отладки

**Логирование:**
```bash
RUST_LOG=debug cargo run --release  # Включить debug логи
```

**Быстрая проверка синтаксиса:**
```bash
echo "SELECT * FROM users WHERE age > 18 AND city = 'Moscow';" | cargo run --example simple_test
```

**Benchmark производительности:**
```bash
hyperfine './target/release/postgrustql' 'psql'  # Сравнение с PostgreSQL
```

**Проверка размера бинарника:**
```bash
ls -lh target/release/postgrustql
strip target/release/postgrustql  # Удалить debug symbols
```

