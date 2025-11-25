# RustDB - Быстрый старт (2 минуты)

## 1. Запустить сервер (Терминал 1)

```bash
cd /home/fr3ddy/Projects/test/rustdb
cargo run --release
```

Ждите сообщение: `RustDB server listening on 127.0.0.1:5432`

## 2. Тестовый клиент (Терминал 2)

```bash
cd /home/fr3ddy/Projects/test/rustdb
cargo run --example simple_test
```

Или интерактивный режим:

```bash
cargo run --example cli
```

## 3. Примеры команд (если используете CLI)

```sql
CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);
INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30);
INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25);
SELECT * FROM users;
UPDATE users SET age = 26 WHERE name = 'Bob';
DELETE FROM users WHERE id = 1;
DROP TABLE users;
quit
```

## 4. Проверить данные

```bash
cat data/main.json
```

## Готово! 🎉

Подробная инструкция: см. `TESTING.md`
