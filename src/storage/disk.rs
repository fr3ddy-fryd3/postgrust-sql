use crate::types::{Column, Database, DatabaseError, Row, ServerInstance, Table};
use crate::storage::wal::{Operation, WalManager};
use std::fs;
use std::path::{Path, PathBuf};

pub struct StorageEngine {
    data_dir: PathBuf,
    wal: WalManager,
    /// Счетчик операций с момента последнего snapshot
    operations_since_snapshot: usize,
    /// Порог операций для создания нового snapshot
    snapshot_threshold: usize,
}

impl StorageEngine {
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Result<Self, DatabaseError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        let wal = WalManager::new(&data_dir)?;

        Ok(Self {
            data_dir,
            wal,
            operations_since_snapshot: 0,
            snapshot_threshold: 100, // Создаем snapshot каждые 100 операций
        })
    }

    /// Сохраняет snapshot серверного экземпляра в binary формате
    fn save_snapshot(&self, instance: &ServerInstance) -> Result<(), DatabaseError> {
        let instance_path = self.data_dir.join("server_instance.db");
        let encoded = bincode::serialize(instance)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;
        fs::write(instance_path, encoded)?;
        Ok(())
    }

    /// Загружает snapshot серверного экземпляра из binary формата
    fn load_snapshot(&self) -> Result<Option<ServerInstance>, DatabaseError> {
        let instance_path = self.data_dir.join("server_instance.db");

        // Проверяем новый формат (server_instance.db)
        if instance_path.exists() {
            let data = fs::read(instance_path)?;
            let instance = bincode::deserialize(&data)
                .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;
            return Ok(Some(instance));
        }

        // Fallback: пробуем загрузить старый формат (отдельные БД)
        // Это для обратной совместимости
        let main_db_path = self.data_dir.join("main.db");
        if main_db_path.exists() {
            let data = fs::read(&main_db_path)?;
            let db: Database = bincode::deserialize(&data)
                .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;

            // Создаем ServerInstance из старой БД
            let mut instance = ServerInstance::new();
            instance.databases.insert(db.name.clone(), db);
            return Ok(Some(instance));
        }

        Ok(None)
    }

    /// Загружает ServerInstance из snapshot + применяет WAL
    pub fn load_server_instance(&self) -> Result<ServerInstance, DatabaseError> {
        // Загружаем последний snapshot
        let mut instance = self.load_snapshot()?.unwrap_or_else(ServerInstance::new);

        // Применяем все операции из WAL
        let logs = self.wal.read_all_logs()?;
        for entry in logs {
            // Применяем операции ко всем БД
            // TODO: WAL нужно расширить для поддержки multi-database операций
            // Пока применяем к первой найденной БД (legacy behavior)
            if let Some(db) = instance.databases.values_mut().next() {
                WalManager::apply_operation(db, &entry.operation)?;
            }
        }

        Ok(instance)
    }

    /// Загружает базу данных из snapshot + применяет WAL (legacy метод для совместимости)
    #[allow(dead_code)]
    pub fn load_database(&self, name: &str) -> Result<Database, DatabaseError> {
        // Сначала пытаемся загрузить из ServerInstance
        let instance = self.load_server_instance()?;

        if let Some(db) = instance.databases.get(name) {
            return Ok(db.clone());
        }

        // Fallback: проверяем legacy формат {name}.db
        let db_path = self.data_dir.join(format!("{}.db", name));
        if db_path.exists() {
            let data = fs::read(&db_path)?;
            let mut db: Database = bincode::deserialize(&data)
                .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;

            // Применяем WAL операции
            let logs = self.wal.read_all_logs()?;
            for entry in logs {
                WalManager::apply_operation(&mut db, &entry.operation)?;
            }

            return Ok(db);
        }

        // Если БД не найдена в snapshot, но есть WAL - применяем WAL к новой БД
        // Это нужно для crash recovery когда snapshot не был создан
        let mut db = Database::new(name.to_string());
        let logs = self.wal.read_all_logs()?;
        for entry in logs {
            WalManager::apply_operation(&mut db, &entry.operation)?;
        }

        Ok(db)
    }

    /// Проверяет нужен ли checkpoint
    pub fn should_checkpoint(&self) -> bool {
        self.operations_since_snapshot >= self.snapshot_threshold
    }

    /// Сохраняет ServerInstance (создаёт checkpoint только при необходимости)
    pub fn save_server_instance(&mut self, instance: &ServerInstance) -> Result<(), DatabaseError> {
        // Делаем checkpoint только если достигли порога операций
        if self.should_checkpoint() {
            self.create_checkpoint_instance(instance)?;
        }
        Ok(())
    }

    /// Сохраняет базу данных (legacy метод, теперь сохраняет через конкретную БД)
    pub fn save_database(&mut self, db: &Database) -> Result<(), DatabaseError> {
        // Legacy: сохраняем отдельную БД
        if self.should_checkpoint() {
            let db_path = self.data_dir.join(format!("{}.db", db.name));
            let encoded = bincode::serialize(db)
                .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;
            fs::write(db_path, encoded)?;

            self.wal.checkpoint()?;
            self.wal.cleanup_old_logs(2)?;
            self.operations_since_snapshot = 0;
        }
        Ok(())
    }

    /// Создает checkpoint для ServerInstance
    pub fn create_checkpoint_instance(&mut self, instance: &ServerInstance) -> Result<(), DatabaseError> {
        // Сохраняем snapshot
        self.save_snapshot(instance)?;

        // Записываем маркер checkpoint в WAL
        self.wal.checkpoint()?;

        // Удаляем старые WAL файлы (оставляем последние 2)
        self.wal.cleanup_old_logs(2)?;

        // Сбрасываем счетчик
        self.operations_since_snapshot = 0;

        Ok(())
    }

    /// Создает checkpoint: snapshot + очистка старых логов (legacy для одной БД)
    #[allow(dead_code)]
    pub fn create_checkpoint(&mut self, db: &Database) -> Result<(), DatabaseError> {
        // Legacy: сохраняем отдельную БД
        let db_path = self.data_dir.join(format!("{}.db", db.name));
        let encoded = bincode::serialize(db)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;
        fs::write(db_path, encoded)?;

        self.wal.checkpoint()?;
        self.wal.cleanup_old_logs(2)?;
        self.operations_since_snapshot = 0;

        Ok(())
    }

    /// Логирует CREATE TABLE операцию
    pub fn log_create_table(&mut self, table: &Table) -> Result<(), DatabaseError> {
        self.wal.append(Operation::CreateTable {
            table_name: table.name.clone(),
            table: table.clone(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует DROP TABLE операцию
    pub fn log_drop_table(&mut self, table_name: &str) -> Result<(), DatabaseError> {
        self.wal.append(Operation::DropTable {
            table_name: table_name.to_string(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует INSERT операцию
    pub fn log_insert(&mut self, table_name: &str, row: &Row) -> Result<(), DatabaseError> {
        self.wal.append(Operation::Insert {
            table_name: table_name.to_string(),
            row: row.clone(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует UPDATE операцию
    pub fn log_update(
        &mut self,
        table_name: &str,
        row_index: usize,
        new_row: &Row,
    ) -> Result<(), DatabaseError> {
        self.wal.append(Operation::Update {
            table_name: table_name.to_string(),
            old_row_index: row_index,
            new_row: new_row.clone(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует DELETE операцию
    pub fn log_delete(&mut self, table_name: &str, row_index: usize) -> Result<(), DatabaseError> {
        self.wal.append(Operation::Delete {
            table_name: table_name.to_string(),
            row_index,
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует ALTER TABLE ADD COLUMN операцию
    pub fn log_alter_table_add_column(&mut self, table_name: &str, column: &Column) -> Result<(), DatabaseError> {
        self.wal.append(Operation::AlterTableAddColumn {
            table_name: table_name.to_string(),
            column: column.clone(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует ALTER TABLE DROP COLUMN операцию
    pub fn log_alter_table_drop_column(&mut self, table_name: &str, column_name: &str) -> Result<(), DatabaseError> {
        self.wal.append(Operation::AlterTableDropColumn {
            table_name: table_name.to_string(),
            column_name: column_name.to_string(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует ALTER TABLE RENAME COLUMN операцию
    pub fn log_alter_table_rename_column(&mut self, table_name: &str, old_name: &str, new_name: &str) -> Result<(), DatabaseError> {
        self.wal.append(Operation::AlterTableRenameColumn {
            table_name: table_name.to_string(),
            old_name: old_name.to_string(),
            new_name: new_name.to_string(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    /// Логирует ALTER TABLE RENAME TO операцию
    pub fn log_alter_table_rename(&mut self, old_table_name: &str, new_table_name: &str) -> Result<(), DatabaseError> {
        self.wal.append(Operation::AlterTableRename {
            old_table_name: old_table_name.to_string(),
            new_table_name: new_table_name.to_string(),
        })?;
        self.operations_since_snapshot += 1;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn delete_database(&self, name: &str) -> Result<(), DatabaseError> {
        // Удаляем binary формат
        let db_path = self.data_dir.join(format!("{}.db", name));
        if db_path.exists() {
            fs::remove_file(db_path)?;
        }

        // Удаляем старый JSON формат если есть
        let json_path = self.data_dir.join(format!("{}.json", name));
        if json_path.exists() {
            fs::remove_file(json_path)?;
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_databases(&self) -> Result<Vec<String>, DatabaseError> {
        let mut databases = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let ext = path.extension().and_then(|s| s.to_str());
                // Ищем и .db (новый формат) и .json (старый формат)
                if ext == Some("db") || ext == Some("json") {
                    if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                        if seen.insert(name.to_string()) {
                            databases.push(name.to_string());
                        }
                    }
                }
            }
        }
        Ok(databases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_storage_new_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let data_path = temp_dir.path().join("data");

        let _storage = StorageEngine::new(&data_path).unwrap();
        assert!(data_path.exists());
        assert!(data_path.is_dir());
    }

    #[test]
    fn test_save_and_load_database() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = StorageEngine::new(temp_dir.path()).unwrap();

        // Create a test database
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            crate::types::Column {
                name: "id".to_string(),
                data_type: crate::types::DataType::Integer,
                nullable: false,
                primary_key: true,
                    foreign_key: None,
                    unique: false,
            },
            crate::types::Column {
                name: "name".to_string(),
                data_type: crate::types::DataType::Text,
                nullable: false,
                primary_key: false,
                    foreign_key: None,
                    unique: false,
            },
        ];
        let table = crate::types::Table::new("users".to_string(), columns);
        db.create_table(table).unwrap();

        // Save database (create checkpoint for testing)
        storage.create_checkpoint(&db).unwrap();

        // Load database
        let loaded_db = storage.load_database("test_db").unwrap();
        assert_eq!(loaded_db.name, "test_db");
        assert_eq!(loaded_db.tables.len(), 1);
        assert!(loaded_db.get_table("users").is_some());
    }

    #[test]
    fn test_load_nonexistent_database() {
        let temp_dir = TempDir::new().unwrap();
        let storage = StorageEngine::new(temp_dir.path()).unwrap();

        // Loading a non-existent database should create a new empty one
        let db = storage.load_database("nonexistent").unwrap();
        assert_eq!(db.name, "nonexistent");
        assert_eq!(db.tables.len(), 0);
    }

    #[test]
    fn test_delete_database() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = StorageEngine::new(temp_dir.path()).unwrap();

        // Create and save a database
        let db = Database::new("to_delete".to_string());
        storage.create_checkpoint(&db).unwrap();

        // Verify it exists (binary format)
        let db_path = temp_dir.path().join("to_delete.db");
        assert!(db_path.exists());

        // Delete it
        storage.delete_database("to_delete").unwrap();
        assert!(!db_path.exists());
    }

    #[test]
    fn test_delete_nonexistent_database() {
        let temp_dir = TempDir::new().unwrap();
        let storage = StorageEngine::new(temp_dir.path()).unwrap();

        // Deleting a non-existent database should not error
        assert!(storage.delete_database("nonexistent").is_ok());
    }

    #[test]
    fn test_list_databases() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = StorageEngine::new(temp_dir.path()).unwrap();

        // Initially empty
        let dbs = storage.list_databases().unwrap();
        assert_eq!(dbs.len(), 0);

        // Create some databases
        let db1 = Database::new("db1".to_string());
        let db2 = Database::new("db2".to_string());
        storage.create_checkpoint(&db1).unwrap();
        storage.create_checkpoint(&db2).unwrap();

        // List should contain both
        let mut dbs = storage.list_databases().unwrap();
        dbs.sort();
        assert_eq!(dbs, vec!["db1".to_string(), "db2".to_string()]);
    }

    #[test]
    fn test_save_database_with_data() {
        let temp_dir = TempDir::new().unwrap();
        let mut storage = StorageEngine::new(temp_dir.path()).unwrap();

        // Create database with data
        let mut db = Database::new("test_db".to_string());
        let columns = vec![
            crate::types::Column {
                name: "id".to_string(),
                data_type: crate::types::DataType::Integer,
                nullable: false,
                primary_key: true,
                    foreign_key: None,
                    unique: false,
            },
        ];
        let mut table = crate::types::Table::new("users".to_string(), columns);
        let row = crate::types::Row::new(vec![crate::types::Value::Integer(1)]);
        table.insert(row).unwrap();
        db.create_table(table).unwrap();

        // Save and reload
        storage.create_checkpoint(&db).unwrap();
        let loaded_db = storage.load_database("test_db").unwrap();

        let loaded_table = loaded_db.get_table("users").unwrap();
        assert_eq!(loaded_table.rows.len(), 1);
        assert_eq!(loaded_table.rows[0].values[0], crate::types::Value::Integer(1));
    }

    #[test]
    fn test_wal_crash_recovery() {
        let temp_dir = TempDir::new().unwrap();

        // Фаза 1: Создаем БД, логируем операции в WAL (без snapshot)
        {
            let mut storage = StorageEngine::new(temp_dir.path()).unwrap();
            let mut db = Database::new("crash_test".to_string());

            // Создаем таблицу
            let columns = vec![
                crate::types::Column {
                    name: "id".to_string(),
                    data_type: crate::types::DataType::Integer,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                    unique: false,
                },
                crate::types::Column {
                    name: "name".to_string(),
                    data_type: crate::types::DataType::Text,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                    unique: false,
                },
            ];
            let table = crate::types::Table::new("users".to_string(), columns);

            // Логируем CREATE TABLE в WAL
            storage.log_create_table(&table).unwrap();
            db.create_table(table).unwrap();

            // Логируем несколько INSERT операций
            let row1 = crate::types::Row::new(vec![
                crate::types::Value::Integer(1),
                crate::types::Value::Text("Alice".to_string()),
            ]);
            storage.log_insert("users", &row1).unwrap();

            let row2 = crate::types::Row::new(vec![
                crate::types::Value::Integer(2),
                crate::types::Value::Text("Bob".to_string()),
            ]);
            storage.log_insert("users", &row2).unwrap();

            let row3 = crate::types::Row::new(vec![
                crate::types::Value::Integer(3),
                crate::types::Value::Text("Charlie".to_string()),
            ]);
            storage.log_insert("users", &row3).unwrap();

            // НЕ вызываем checkpoint - симулируем краш!
            // storage уничтожается (drop) без сохранения snapshot
        }

        // Фаза 2: "Перезапуск" после краша - создаем новый storage
        {
            let storage = StorageEngine::new(temp_dir.path()).unwrap();

            // Загружаем БД - должен произойти replay из WAL
            let loaded_db = storage.load_database("crash_test").unwrap();

            // Проверяем что таблица восстановилась
            assert!(loaded_db.get_table("users").is_some());

            let table = loaded_db.get_table("users").unwrap();
            assert_eq!(table.columns.len(), 2);
            assert_eq!(table.columns[0].name, "id");
            assert_eq!(table.columns[1].name, "name");

            // Проверяем что все 3 строки восстановились из WAL
            assert_eq!(table.rows.len(), 3);
            assert_eq!(table.rows[0].values[0], crate::types::Value::Integer(1));
            assert_eq!(
                table.rows[0].values[1],
                crate::types::Value::Text("Alice".to_string())
            );
            assert_eq!(table.rows[1].values[0], crate::types::Value::Integer(2));
            assert_eq!(
                table.rows[1].values[1],
                crate::types::Value::Text("Bob".to_string())
            );
            assert_eq!(table.rows[2].values[0], crate::types::Value::Integer(3));
            assert_eq!(
                table.rows[2].values[1],
                crate::types::Value::Text("Charlie".to_string())
            );
        }
    }

    #[test]
    fn test_wal_with_checkpoint() {
        let temp_dir = TempDir::new().unwrap();

        // Создаем snapshot + WAL
        {
            let mut storage = StorageEngine::new(temp_dir.path()).unwrap();
            let mut db = Database::new("checkpoint_test".to_string());

            // Создаем таблицу и делаем checkpoint
            let columns = vec![crate::types::Column {
                name: "id".to_string(),
                data_type: crate::types::DataType::Integer,
                nullable: false,
                primary_key: true,
                    foreign_key: None,
                    unique: false,
            }];
            let table = crate::types::Table::new("test".to_string(), columns);
            db.create_table(table.clone()).unwrap();

            // Создаем checkpoint (snapshot)
            storage.create_checkpoint(&db).unwrap();

            // Добавляем операции после checkpoint
            storage.log_insert("test", &crate::types::Row::new(vec![crate::types::Value::Integer(1)])).unwrap();
            storage.log_insert("test", &crate::types::Row::new(vec![crate::types::Value::Integer(2)])).unwrap();
        }

        // Загружаем - должен загрузить snapshot + применить WAL после checkpoint
        {
            let storage = StorageEngine::new(temp_dir.path()).unwrap();
            let loaded_db = storage.load_database("checkpoint_test").unwrap();

            let table = loaded_db.get_table("test").unwrap();
            // Должно быть 2 строки из WAL (после checkpoint)
            assert_eq!(table.rows.len(), 2);
            assert_eq!(table.rows[0].values[0], crate::types::Value::Integer(1));
            assert_eq!(table.rows[1].values[0], crate::types::Value::Integer(2));
        }
    }
}
