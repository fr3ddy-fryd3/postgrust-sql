# PostgrustSQL - PostgreSQL-совместимая база данных на Rust

> **🤖 Experimental AI-Driven Project**
> Проект создан из интереса проверить, может ли AI полностью самостоятельно написать работающую базу данных. Весь код и архитектура написаны без человеческого вмешательства.

PostgrustSQL - это реляционная база данных, написанная на Rust, с PostgreSQL-совместимым wire protocol. Поддерживает расширенные SQL операции, MVCC, транзакции, индексы и работает на порту 5432.

## 🚀 Быстрый старт

**Автоматический тест (одна команда):**
```bash
./run_test.sh
```

**Или вручную:**
```bash
# Терминал 1: Запустить сервер
cargo run --release

# Терминал 2: Запустить клиент
cargo run --example cli
```

## Возможности (v2.2.0)

### Основное
- **SQL запросы**: CREATE/DROP TABLE/VIEW, INSERT, SELECT, UPDATE, DELETE, SHOW TABLES
- **Multi-Connection Transaction Isolation** (v2.1.0): DML изолирован между connections
- **Backup & Restore** (v2.2.0): pgr_dump/pgr_restore утилиты (SQL + binary форматы)
- **MVCC (Multi-Version Concurrency Control)**: изоляция с версионированием строк (xmin/xmax)
- **WAL (Write-Ahead Log)**: автоматическое логирование операций с crash recovery
- **Транзакции**: BEGIN, COMMIT, ROLLBACK с READ COMMITTED isolation
- **Индексы**: B-tree и Hash индексы (одиночные и составные)
- **VACUUM**: очистка мёртвых версий строк (MVCC cleanup)
- **PostgreSQL Protocol** (v2.0.0): Полная совместимость с psql клиентом
  - Стандартный authentication flow (AuthenticationCleartextPassword)
  - System catalogs (pg_catalog.*, information_schema.*)
  - System functions (version(), current_database(), pg_table_size())
- **Качество кода** (v2.0.2): 0 deprecated warnings, relaxed Clippy для pet project

### SQL Возможности
- **23 типа данных**: SMALLINT, INTEGER, BIGINT, SERIAL, BIGSERIAL, REAL, NUMERIC(p,s), TEXT, VARCHAR(n), CHAR(n), BOOLEAN, DATE, TIMESTAMP, TIMESTAMPTZ, UUID, JSON, JSONB, BYTEA, ENUM, и др.
- **JOIN**: INNER, LEFT, RIGHT
- **Агрегаты**: COUNT, SUM, AVG, MIN, MAX
- **GROUP BY**
- **ORDER BY** с ASC/DESC
- **LIMIT** и **OFFSET**
- **DISTINCT**
- **UNIQUE** constraints
- **CASE выражения** (v1.10.0)
- **Set операции**: UNION, UNION ALL, INTERSECT, EXCEPT (v1.10.0)
- **Views**: виртуальные таблицы (v1.10.0)
- **WHERE операторы**: =, !=, >, <, >=, <=, BETWEEN, LIKE, IN, IS NULL/IS NOT NULL

### Дополнительно
- **EXPLAIN**: анализ плана выполнения запросов
- **Page-based storage**: оптимизированное хранение (125x улучшение)
- **Составные индексы**: поддержка multi-column индексов
- **Foreign Keys**: поддержка внешних ключей

## Архитектура (v2.0.2)

**Модульная структура** (~2400 строк кода, чистый код после v2.0.0 cleanup):

```
rustdb/
├── src/
│   ├── main.rs             # Точка входа сервера
│   ├── core/               # Базовые типы (Database, Table, Row, Value, Column)
│   ├── parser/             # SQL парсер (nom) - ddl.rs, dml.rs, queries.rs
│   ├── executor/           # Модульный исполнитель
│   │   ├── storage_adapter.rs  # RowStorage trait (Vec<Row> | PagedTable)
│   │   ├── conditions.rs       # WHERE evaluation (все операторы)
│   │   ├── dml.rs             # INSERT/UPDATE/DELETE
│   │   ├── ddl.rs             # CREATE/DROP/ALTER TABLE
│   │   ├── queries.rs         # SELECT (с query planner)
│   │   ├── vacuum.rs          # VACUUM cleanup
│   │   ├── index.rs           # CREATE/DROP INDEX
│   │   ├── explain.rs         # EXPLAIN analyzer
│   │   └── dispatcher.rs      # Query dispatcher (146 строк, v2.0.0: renamed from legacy.rs)
│   ├── index/              # B-tree & Hash индексы (single & composite)
│   ├── transaction/        # TransactionManager, Snapshot
│   ├── storage/            # Binary save/load, WAL, Page-based storage
│   └── network/            # TCP server, PostgreSQL protocol
└── examples/
    ├── client.rs           # Автоматический клиент
    └── cli.rs              # Интерактивный CLI клиент
```

## Установка и запуск

### Вариант 1: Docker (рекомендуется)

```bash
# Собрать и запустить
docker-compose up -d

# Проверить статус
docker-compose ps

# Посмотреть логи
docker-compose logs -f rustdb

# Подключиться к серверу
nc localhost 5432
# или
telnet localhost 5432
# или через PostgreSQL клиент
psql -h localhost -p 5432 -U rustdb -d main

# Выполнить команду внутри контейнера
docker-compose exec rustdb /app/postgrustql --help

# Остановить
docker-compose down

# Остановить и удалить данные
docker-compose down -v

# Пересобрать после изменений кода
docker-compose build --no-cache
docker-compose up -d
```

### Вариант 2: Локальная сборка

#### Сборка проекта

```bash
cd rustdb
cargo build --release
```

#### Запуск сервера

```bash
cargo run --release
```

Сервер запустится на `127.0.0.1:5432` и будет сохранять данные в папку `./data/`

### Использование клиента

#### Интерактивный CLI клиент:

```bash
cargo run --example cli
```

#### Автоматический тестовый клиент:

```bash
cargo run --example client
```

## Примеры SQL запросов

### Создание таблицы

```sql
CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER,
    active BOOLEAN
);
```

### Вставка данных

```sql
INSERT INTO users (id, name, age, active) VALUES (1, 'Alice', 30, TRUE);
INSERT INTO users (id, name, age, active) VALUES (2, 'Bob', 25, TRUE);
```

### Выборка данных

```sql
-- Выбрать все записи
SELECT * FROM users;

-- Выбрать определенные колонки
SELECT name, age FROM users;

-- Выбрать с условием
SELECT * FROM users WHERE age > 25;
```

### Обновление данных

```sql
UPDATE users SET age = 31 WHERE name = 'Alice';
UPDATE users SET active = FALSE WHERE age < 26;
```

### Удаление данных

```sql
DELETE FROM users WHERE age < 30;
```

### Просмотр списка таблиц

```sql
SHOW TABLES;
```

### Удаление таблицы

```sql
DROP TABLE users;
```

### PostgreSQL System Catalogs (v2.0.0)

```sql
-- Посмотреть все таблицы
SELECT * FROM pg_catalog.pg_class WHERE relkind = 'r';

-- Посмотреть колонки таблицы
SELECT attname, atttypid FROM pg_catalog.pg_attribute
WHERE attrelid = (SELECT oid FROM pg_catalog.pg_class WHERE relname = 'users');

-- Information Schema
SELECT table_name FROM information_schema.tables;
SELECT column_name, data_type FROM information_schema.columns
WHERE table_name = 'users';
```

### System Functions (v2.0.0)

```sql
-- Версия сервера
SELECT version();

-- Текущая база данных
SELECT current_database();

-- Размер таблицы в байтах
SELECT pg_table_size('users');

-- Размер базы данных
SELECT pg_database_size('main');
```

## Поддерживаемые типы данных (23 типа)

**Числовые:**
- `SMALLINT` (i16), `INTEGER` / `INT` (i32), `BIGINT` (i64)
- `SERIAL` (auto-increment i32), `BIGSERIAL` (auto-increment i64)
- `REAL` / `FLOAT` (f32), `DOUBLE PRECISION` (f64)
- `NUMERIC(precision, scale)` - точные десятичные числа

**Строковые:**
- `TEXT` - произвольная длина
- `VARCHAR(n)` - ограничение длины с валидацией
- `CHAR(n)` - фиксированная длина с padding

**Дата и время:**
- `DATE` - дата (YYYY-MM-DD)
- `TIMESTAMP` - дата и время без timezone
- `TIMESTAMPTZ` - дата и время с timezone

**Специальные:**
- `BOOLEAN` / `BOOL` - true/false
- `UUID` - универсальный уникальный идентификатор
- `JSON` - текстовый JSON
- `JSONB` - бинарный JSON (быстрее)
- `BYTEA` - бинарные данные
- `ENUM` - пользовательский перечисляемый тип

## Транзакции

PostgrustSQL поддерживает транзакции с snapshot isolation:

```sql
-- Начать транзакцию
BEGIN;

-- Выполнить операции
INSERT INTO accounts (id, balance) VALUES (1, 1000);
UPDATE accounts SET balance = 1500 WHERE id = 1;

-- Зафиксировать изменения
COMMIT;

-- Или откатить
ROLLBACK;
```

**Важно:** Изменения в транзакции видны сразу, но сохраняются на диск только после COMMIT.

## Backup & Restore (v2.2.0)

### Экспорт базы данных

```bash
# Полный дамп (схема + данные) в SQL формат
./target/release/pgr_dump postgres > backup.sql

# Только схема (CREATE statements)
./target/release/pgr_dump --schema-only postgres > schema.sql

# Только данные (INSERT statements)
./target/release/pgr_dump --data-only postgres > data.sql

# Бинарный формат (быстрее для больших БД)
./target/release/pgr_dump --format=binary postgres > backup.bin
```

### Импорт базы данных

```bash
# Восстановление из SQL дампа
./target/release/pgr_restore postgres < backup.sql

# Восстановление из бинарного формата
./target/release/pgr_restore --format=binary postgres < backup.bin

# Dry-run (только валидация, без выполнения)
./target/release/pgr_restore --dry-run postgres < backup.sql
```

## Подключение к серверу

### Через psql (PostgreSQL клиент) - v2.0.0+

```bash
# Стандартный PostgreSQL клиент (рекомендуется)
psql -h 127.0.0.1 -p 5432 -U rustdb -d main
# Пароль: любой (authentication в v2.0.0)

# Использование meta-команд
\dt                    # Список таблиц
\d users              # Описание таблицы users
\di                   # Список индексов
\l                    # Список баз данных
```

### Через telnet или netcat

```bash
# Через telnet
telnet 127.0.0.1 5432

# Через netcat
nc 127.0.0.1 5432
```

Вывод SELECT запросов будет красиво отформатирован:
```
┌────┬───────┬─────┐
│ id ┆ name  ┆ age │
╞════╪═══════╪═════╡
│ 1  ┆ Alice ┆ 30  │
└────┴───────┴─────┘
```

## Технологии

- **Rust Edition 2024**
- **tokio 1.41** - асинхронный runtime
- **nom 7.1** - парсер комбинаторы для SQL
- **serde 1.0 + bincode 1.3** - бинарная сериализация (WAL, snapshots)
- **serde_json 1.0** - JSON/JSONB поддержка
- **thiserror 2.0** - обработка ошибок
- **comfy-table 7.1** - красивое форматирование таблиц
- **rustyline 14.0** - CLI с историей команд
- **chrono 0.4** - Date/Time типы
- **uuid 1.6** - UUID тип
- **rust_decimal 1.33** - NUMERIC тип с точностью

## PostgreSQL Совместимость (v2.0.0+)

### ✅ Поддерживается
- PostgreSQL wire protocol (порт 5432)
- Authentication (cleartext password)
- Подключение через psql клиент
- System catalogs (pg_catalog.*, information_schema.*)
- System functions (version(), current_database(), pg_*_size())
- Meta-команды psql (\dt, \d, \di, \l)

### ⚠️ Ограничения
- Транзакции работают только в пределах одного подключения (planned v2.1.0)
- Один JOIN на запрос (множественные JOIN planned)
- WHERE с JOIN не полностью поддерживается
- Составные индексы требуют точного совпадения всех колонок
- Hash индексы только для = (B-tree для диапазонов)
- Extended Query Protocol (prepared statements) пока не поддерживается

## Разработка

### Запуск тестов

```bash
# Юнит-тесты (159 тестов, все проходят ✅ v2.0.2)
cargo test

# Интеграционные тесты
./tests/integration/test_features.sh           # Все основные фичи
./tests/integration/test_new_types.sh          # Все 23 типа данных
./tests/integration/test_hash_index.sh         # B-tree & Hash индексы
./tests/integration/test_composite_index.sh    # Составные индексы
./tests/integration/test_extended_operators.sh # Расширенные WHERE операторы
./tests/integration/test_explain.sh            # EXPLAIN команда
./tests/integration/test_sql_expressions.sh    # CASE & set операции
./tests/integration/test_vacuum.sh             # VACUUM cleanup
```

### Форматирование кода

```bash
cargo fmt
```

### Линтинг

```bash
cargo clippy
```

## История версий

**v2.0.2** (Текущая) - Complete PagedTable Migration
- 🧹 Удалены все deprecated Table.rows usage (0 warnings, было 17)
- ✨ Все executors теперь используют только PagedTable (mandatory &DatabaseStorage)
- 🔧 Исправлено 10 aggregate/group_by тестов
- 📝 Relaxed Clippy конфигурация (~20 warnings, было 292)
- ✅ 159/159 юнит-тестов проходят

**v2.0.1** - Качество кода и тесты
- 🔧 Строгая конфигурация Clippy (pedantic + nursery + cargo + correctness)
- ✨ Автоматические исправления применены (unused imports, dereferencing, etc.)
- 🔄 Рефакторинг 16 dispatcher тестов для page-based storage
- ✅ 166/166 тестов проходят (100% success rate)

**v2.0.0** - PostgreSQL Compatibility Layer
- 🔐 Стандартный PostgreSQL authentication protocol
- 📊 System catalogs (pg_catalog.pg_class, pg_attribute, pg_index, pg_type, pg_namespace)
- 📊 Information schema (information_schema.tables, columns, views)
- ⚙️ System functions (version(), current_database(), pg_table_size(), pg_database_size())
- 🔄 Рефакторинг: legacy.rs → dispatcher.rs
- 🧹 Полное удаление устаревшего кода
- ✅ Полная совместимость с psql клиентом

**v1.11.0** - Критические исправления и стабильность
- 🐛 Исправлены 4 падающих теста хранилища (WAL crash recovery)
- 🧹 Исправлены все compiler warnings (26 шт.)
- ✅ 154/154 юнит-тестов проходят
- 🔧 Подготовка к v2.0.0 (стабильная база)

**v1.10.0** - SQL выражения и операции над множествами
- ✨ CASE WHEN...THEN...ELSE...END выражения
- ✨ Set операции (UNION, UNION ALL, INTERSECT, EXCEPT)
- ✨ Views (CREATE/DROP VIEW, виртуальные таблицы)

**v1.9.0** - Составные (multi-column) индексы
- ✨ CREATE INDEX на несколько колонок: idx(col1, col2, col3)
- ✨ Поддержка BTREE и HASH для составных индексов
- ✨ Автоматическая оптимизация для AND условий
- ✅ 147 юнит-тестов

**v1.8.0** - Расширенные WHERE операторы + EXPLAIN
- ✨ Новые операторы: >=, <=, BETWEEN, LIKE, IN, IS NULL
- ✨ EXPLAIN команда для анализа запросов
- 📊 Показывает тип сканирования, используемые индексы, сложность O(n)/O(log n)/O(1)

**v1.7.0** - Hash индексы
- ✨ Hash индексы для O(1) поиска
- ✨ USING HASH/BTREE синтаксис
- 🔄 Автоматический query planner выбирает тип индекса

**v1.6.0** - B-tree индексы с оптимизацией
- ✨ B-tree индексы (O(log n) поиск)
- ✨ CREATE INDEX / CREATE UNIQUE INDEX / DROP INDEX
- 🔄 Автоматический выбор между index scan и sequential scan
- 🔧 MVCC-aware операции с индексами

**v1.5.1** - VACUUM команда
- ✨ VACUUM для очистки мёртвых версий строк (MVCC cleanup)
- 🧹 Освобождение места после UPDATE/DELETE

**v1.5.0** - Page-based storage
- 🚀 Улучшение write amplification в 125x
- 📄 PostgreSQL-совместимые 8KB страницы
- 💾 LRU buffer pool для кэширования
- 🔧 Модульный executor (5 специализированных модулей)

**v1.4.1** - ALTER TABLE
- ✨ ALTER TABLE ADD/DROP/RENAME COLUMN
- ✨ ALTER TABLE RENAME TO
- 📝 Интеграция с WAL для crash recovery

**v1.4.0** - Query enhancements
- ✨ OFFSET для пагинации
- ✨ DISTINCT для уникальных значений
- ✨ UNIQUE constraint для колонок

**v1.3.2** - Модульная архитектура
- 🔄 Рефакторинг: 9 файлов → 40+ модулей
- 📁 Структура: core/, parser/, transaction/, storage/, network/
- ✅ 66+ тестов, полная обратная совместимость

**v1.3.1** - PostgreSQL syntax + 18 новых типов
- ✨ 18 новых типов данных (SMALLINT, UUID, DATE, TIMESTAMP, ENUM, etc.)
- 📊 23 типа в сумме (~45% PostgreSQL совместимость)
- 🔧 Meta-команды (\dt, \l, \du)
- ✅ Валидация типов

## Лицензия

MIT
