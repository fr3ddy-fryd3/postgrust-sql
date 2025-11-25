# Инструкция по тестированию RustDB

## Предварительные требования

Убедитесь, что у вас установлены:
- Rust (версия 1.70+)
- Cargo

Проверить можно командой:
```bash
rustc --version
cargo --version
```

## Шаг 1: Перейти в директорию проекта

```bash
cd /home/fr3ddy/Projects/test/rustdb
```

## Шаг 2: Очистить старые данные (опционально)

Если хотите начать с чистой базы:
```bash
rm -rf data/
```

## Шаг 3: Запустить сервер

В первом терминале запустите сервер:
```bash
cargo run --release
```

Вы должны увидеть:
```
Starting RustDB server...
RustDB server listening on 127.0.0.1:5432
```

**Оставьте этот терминал открытым!** Сервер работает.

## Шаг 4: Тестирование (выберите один способ)

### Способ 1: Автоматический тест (рекомендуется)

Откройте второй терминал и выполните:
```bash
cd /home/fr3ddy/Projects/test/rustdb
cargo run --example simple_test
```

Вы увидите отправку всех запросов к серверу.

### Способ 2: Интерактивный CLI

Откройте второй терминал:
```bash
cd /home/fr3ddy/Projects/test/rustdb
cargo run --example cli
```

Теперь вы можете вводить SQL команды вручную:

```sql
CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price INTEGER);
INSERT INTO products (id, name, price) VALUES (1, 'Laptop', 1500);
INSERT INTO products (id, name, price) VALUES (2, 'Mouse', 25);
SELECT * FROM products;
UPDATE products SET price = 30 WHERE name = 'Mouse';
SELECT * FROM products WHERE price > 20;
DELETE FROM products WHERE id = 1;
SELECT * FROM products;
```

Для выхода введите `quit` или `exit`.

### Способ 3: Полный тест-клиент

```bash
cd /home/fr3ddy/Projects/test/rustdb
cargo run --example client
```

Выполнит полный набор тестов с созданием таблицы users.

## Шаг 5: Проверить сохранённые данные

После выполнения запросов, проверьте, что данные сохранились на диск:

```bash
cat data/main.json
```

Или более читаемо:
```bash
cat data/main.json | python3 -m json.tool
```

Вы увидите JSON с вашими таблицами и данными.

## Шаг 6: Проверить персистентность

1. Остановите сервер (Ctrl+C в первом терминале)
2. Запустите сервер снова:
   ```bash
   cargo run --release
   ```
3. Подключитесь клиентом:
   ```bash
   cargo run --example cli
   ```
4. Выполните SELECT запрос:
   ```sql
   SELECT * FROM users;
   ```

Данные должны сохраниться после перезапуска сервера!

## Примеры SQL команд для тестирования

### Создание таблицы
```sql
CREATE TABLE employees (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    salary INTEGER,
    active BOOLEAN
);
```

### Вставка данных
```sql
INSERT INTO employees (id, name, salary, active) VALUES (1, 'John', 5000, TRUE);
INSERT INTO employees (id, name, salary, active) VALUES (2, 'Jane', 6000, TRUE);
INSERT INTO employees (id, name, salary, active) VALUES (3, 'Bob', 4500, FALSE);
```

### Выборка данных
```sql
SELECT * FROM employees;
SELECT name, salary FROM employees;
SELECT * FROM employees WHERE salary > 5000;
SELECT * FROM employees WHERE active = TRUE;
```

### Обновление данных
```sql
UPDATE employees SET salary = 5500 WHERE name = 'John';
UPDATE employees SET active = TRUE WHERE id = 3;
```

### Удаление данных
```sql
DELETE FROM employees WHERE salary < 5000;
DELETE FROM employees WHERE active = FALSE;
```

### Удаление таблицы
```sql
DROP TABLE employees;
```

## Поддерживаемые типы данных

- `INTEGER` или `INT` - целые числа
- `REAL` или `FLOAT` - вещественные числа
- `TEXT` или `VARCHAR` - строки (в одинарных кавычках: 'text')
- `BOOLEAN` или `BOOL` - TRUE/FALSE

## Операторы WHERE

- `=` - равно
- `!=` - не равно
- `>` - больше
- `<` - меньше

## Известные ограничения

1. Только одно условие в WHERE (нет AND/OR)
2. Нет JOIN операций
3. Нет индексов
4. Нет агрегатных функций (COUNT, SUM, AVG)
5. Нет GROUP BY, ORDER BY, LIMIT

## Остановка сервера

В терминале с сервером нажмите `Ctrl+C`

## Очистка

Удалить все данные:
```bash
rm -rf data/
```

Пересобрать проект:
```bash
cargo clean
cargo build --release
```

## Возможные проблемы

### Порт уже занят
Если порт 5432 занят, измените его в `src/main.rs`:
```rust
server.start("127.0.0.1:5433").await?;
```

### Ошибки компиляции
Попробуйте обновить зависимости:
```bash
cargo update
cargo build --release
```

### База не сохраняется
Проверьте права доступа к папке `data/`:
```bash
ls -la data/
```

## Дополнительные тесты

### Тест производительности
Создайте много записей:
```bash
cargo run --example cli
```

Затем в цикле вставляйте данные (или напишите скрипт).

### Тест конкурентности
Запустите несколько клиентов одновременно:
```bash
# Терминал 2
cargo run --example cli

# Терминал 3
cargo run --example cli

# Терминал 4
cargo run --example cli
```

Все клиенты могут работать с базой одновременно!

---

**Готово!** Теперь у вас работает собственная база данных на Rust.
