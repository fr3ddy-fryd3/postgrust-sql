use crate::types::{Column, Database, DatabaseError, Row, Table};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Типы операций, записываемых в WAL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    /// CREATE TABLE
    CreateTable {
        table_name: String,
        table: Table,
    },
    /// DROP TABLE
    DropTable {
        table_name: String,
    },
    /// INSERT INTO
    Insert {
        table_name: String,
        row: Row,
    },
    /// UPDATE (упрощенная версия - удаление + вставка)
    Update {
        table_name: String,
        old_row_index: usize,
        new_row: Row,
    },
    /// DELETE
    Delete {
        table_name: String,
        row_index: usize,
    },
    /// Checkpoint marker - указывает что был создан snapshot
    Checkpoint {
        timestamp: u64,
    },
    /// ALTER TABLE ADD COLUMN
    AlterTableAddColumn {
        table_name: String,
        column: Column,
    },
    /// ALTER TABLE DROP COLUMN
    AlterTableDropColumn {
        table_name: String,
        column_name: String,
    },
    /// ALTER TABLE RENAME COLUMN
    AlterTableRenameColumn {
        table_name: String,
        old_name: String,
        new_name: String,
    },
    /// ALTER TABLE RENAME TO
    AlterTableRename {
        old_table_name: String,
        new_table_name: String,
    },
}

/// Запись в WAL логе
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Порядковый номер (LSN - Log Sequence Number)
    pub sequence: u64,
    /// Timestamp когда операция была выполнена
    pub timestamp: u64,
    /// Операция
    pub operation: Operation,
}

impl LogEntry {
    #[must_use] 
    pub fn new(sequence: u64, operation: Operation) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            sequence,
            timestamp,
            operation,
        }
    }
}

/// Write-Ahead Log Manager
pub struct WalManager {
    /// Директория для WAL файлов
    wal_dir: PathBuf,
    /// Текущий sequence number
    current_sequence: u64,
    /// Текущий активный WAL файл
    current_wal_file: Option<File>,
    /// Имя текущего WAL файла
    current_wal_name: String,
    /// Максимальный размер WAL файла в байтах (по умолчанию 1MB)
    max_wal_size: u64,
}

impl WalManager {
    /// Создает новый WAL Manager
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Result<Self, DatabaseError> {
        let wal_dir = data_dir.as_ref().join("wal");
        fs::create_dir_all(&wal_dir)?;

        let mut manager = Self {
            wal_dir,
            current_sequence: 0,
            current_wal_file: None,
            current_wal_name: String::new(),
            max_wal_size: 1024 * 1024, // 1MB
        };

        // Находим последний sequence number из существующих логов
        manager.recover_sequence()?;
        // Создаем новый WAL файл
        manager.rotate_wal()?;

        Ok(manager)
    }

    /// Восстанавливает sequence number из существующих WAL файлов
    fn recover_sequence(&mut self) -> Result<(), DatabaseError> {
        let mut max_sequence = 0u64;

        for entry in fs::read_dir(&self.wal_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("wal")
                && let Ok(entries) = Self::read_wal_file(&path) {
                    for log_entry in entries {
                        if log_entry.sequence > max_sequence {
                            max_sequence = log_entry.sequence;
                        }
                    }
                }
        }

        self.current_sequence = max_sequence;
        Ok(())
    }

    /// Создает новый WAL файл (rotation)
    fn rotate_wal(&mut self) -> Result<(), DatabaseError> {
        // Закрываем текущий файл (если есть)
        if let Some(mut file) = self.current_wal_file.take() {
            file.flush()?;
        }

        // Создаем новый WAL файл с timestamp
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let wal_name = format!("{timestamp:016x}.wal");
        let wal_path = self.wal_dir.join(&wal_name);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(wal_path)?;

        self.current_wal_file = Some(file);
        self.current_wal_name = wal_name;

        Ok(())
    }

    /// Записывает операцию в WAL
    pub fn append(&mut self, operation: Operation) -> Result<u64, DatabaseError> {
        // Увеличиваем sequence
        self.current_sequence += 1;

        let entry = LogEntry::new(self.current_sequence, operation);

        // Сериализуем в bincode
        let encoded = bincode::serialize(&entry)
            .map_err(|e| DatabaseError::BinarySerialization(e.to_string()))?;

        if let Some(ref mut file) = self.current_wal_file {
            // Записываем длину (4 байта) + данные
            let len = encoded.len() as u32;
            file.write_all(&len.to_le_bytes())?;
            file.write_all(&encoded)?;
            file.flush()?;

            // Проверяем размер файла для rotation
            let metadata = file.metadata()?;
            if metadata.len() >= self.max_wal_size {
                self.rotate_wal()?;
            }
        }

        Ok(self.current_sequence)
    }

    /// Читает все записи из WAL файла (binary format)
    fn read_wal_file<P: AsRef<Path>>(path: P) -> Result<Vec<LogEntry>, DatabaseError> {
        let mut file = File::open(path)?;
        let mut entries = Vec::new();

        loop {
            // Читаем длину записи (4 байта)
            let mut len_bytes = [0u8; 4];
            match file.read_exact(&mut len_bytes) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    // Конец файла - это нормально
                    break;
                }
                Err(e) => return Err(e.into()),
            }

            let len = u32::from_le_bytes(len_bytes) as usize;

            // Читаем данные
            let mut data = vec![0u8; len];
            file.read_exact(&mut data)?;

            // Десериализуем
            match bincode::deserialize::<LogEntry>(&data) {
                Ok(entry) => entries.push(entry),
                Err(e) => {
                    eprintln!("Warning: failed to parse WAL entry: {e}");
                    // Продолжаем, игнорируя поврежденные записи
                }
            }
        }

        Ok(entries)
    }

    /// Читает все WAL записи (для recovery)
    pub fn read_all_logs(&self) -> Result<Vec<LogEntry>, DatabaseError> {
        let mut all_entries = Vec::new();
        let mut wal_files = Vec::new();

        // Собираем все WAL файлы
        for entry in fs::read_dir(&self.wal_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("wal") {
                wal_files.push(path);
            }
        }

        // Сортируем по имени (timestamp в имени)
        wal_files.sort();

        // Читаем все файлы по порядку
        for wal_file in wal_files {
            let entries = Self::read_wal_file(&wal_file)?;
            all_entries.extend(entries);
        }

        // Сортируем по sequence number для надежности
        all_entries.sort_by_key(|e| e.sequence);

        Ok(all_entries)
    }

    /// Применяет операцию к базе данных
    ///
    /// LEGACY: This function is used for WAL replay on v1.x databases only
    #[allow(deprecated)]
    pub fn apply_operation(db: &mut Database, operation: &Operation) -> Result<(), DatabaseError> {
        match operation {
            Operation::CreateTable { table_name, table } => {
                if !db.tables.contains_key(table_name) {
                    db.create_table(table.clone())?;
                }
            }
            Operation::DropTable { table_name } => {
                db.drop_table(table_name).ok(); // Игнорируем ошибки
            }
            Operation::Insert { table_name, row } => {
                if let Some(table) = db.get_table_mut(table_name) {
                    table.insert(row.clone())?;
                }
            }
            Operation::Update {
                table_name,
                old_row_index,
                new_row,
            } => {
                if let Some(table) = db.get_table_mut(table_name)
                    && *old_row_index < table.rows.len() {
                        table.rows[*old_row_index] = new_row.clone();
                    }
            }
            Operation::Delete {
                table_name,
                row_index,
            } => {
                if let Some(table) = db.get_table_mut(table_name)
                    && *row_index < table.rows.len() {
                        table.rows.remove(*row_index);
                    }
            }
            Operation::Checkpoint { .. } => {
                // Checkpoint marker - ничего не делаем
            }
            Operation::AlterTableAddColumn { table_name, column } => {
                if let Some(table) = db.get_table_mut(table_name) {
                    table.columns.push(column.clone());
                    // Add NULL value to all existing rows
                    for row in &mut table.rows {
                        row.values.push(crate::types::Value::Null);
                    }
                }
            }
            Operation::AlterTableDropColumn { table_name, column_name } => {
                if let Some(table) = db.get_table_mut(table_name)
                    && let Some(col_idx) = table.get_column_index(column_name) {
                        table.columns.remove(col_idx);
                        for row in &mut table.rows {
                            row.values.remove(col_idx);
                        }
                    }
            }
            Operation::AlterTableRenameColumn { table_name, old_name, new_name } => {
                if let Some(table) = db.get_table_mut(table_name)
                    && let Some(col_idx) = table.get_column_index(old_name) {
                        table.columns[col_idx].name = new_name.clone();
                    }
            }
            Operation::AlterTableRename { old_table_name, new_table_name } => {
                if let Some(mut table) = db.tables.remove(old_table_name) {
                    table.name = new_table_name.clone();
                    db.tables.insert(new_table_name.clone(), table);
                }
            }
        }

        Ok(())
    }

    /// Удаляет старые WAL файлы (после checkpoint)
    pub fn cleanup_old_logs(&self, keep_count: usize) -> Result<(), DatabaseError> {
        let mut wal_files = Vec::new();

        for entry in fs::read_dir(&self.wal_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("wal") {
                wal_files.push(path);
            }
        }

        // Сортируем по имени
        wal_files.sort();

        // Удаляем старые, оставляя последние keep_count
        if wal_files.len() > keep_count {
            let to_remove = wal_files.len() - keep_count;
            for path in wal_files.iter().take(to_remove) {
                fs::remove_file(path).ok();
            }
        }

        Ok(())
    }

    /// Записывает checkpoint маркер
    pub fn checkpoint(&mut self) -> Result<(), DatabaseError> {
        self.append(Operation::Checkpoint {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, DataType, Value};
    use tempfile::TempDir;

    #[test]
    fn test_wal_creation() {
        let temp_dir = TempDir::new().unwrap();
        let wal = WalManager::new(temp_dir.path()).unwrap();

        assert_eq!(wal.current_sequence, 0);
        assert!(wal.wal_dir.exists());
    }

    #[test]
    fn test_wal_append_and_read() {
        let temp_dir = TempDir::new().unwrap();
        let mut wal = WalManager::new(temp_dir.path()).unwrap();

        let columns = vec![Column {
            name: "id".to_string(),
            data_type: DataType::Integer,
            nullable: false,
            primary_key: true,
                foreign_key: None,
                unique: false,
        }];

        let table = Table::new("test".to_string(), columns);
        let op = Operation::CreateTable {
            table_name: "test".to_string(),
            table,
        };

        let seq = wal.append(op).unwrap();
        assert_eq!(seq, 1);

        let logs = wal.read_all_logs().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].sequence, 1);
    }

    #[test]
    fn test_wal_apply_operations() {
        let mut db = Database::new("test".to_string());

        let columns = vec![Column {
            name: "id".to_string(),
            data_type: DataType::Integer,
            nullable: false,
            primary_key: true,
                foreign_key: None,
                unique: false,
        }];

        let table = Table::new("users".to_string(), columns);
        let op = Operation::CreateTable {
            table_name: "users".to_string(),
            table,
        };

        WalManager::apply_operation(&mut db, &op).unwrap();
        assert!(db.get_table("users").is_some());

        let row = Row::new(vec![Value::Integer(1)]);
        let op = Operation::Insert {
            table_name: "users".to_string(),
            row,
        };

        WalManager::apply_operation(&mut db, &op).unwrap();
        let table = db.get_table("users").unwrap();
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn test_wal_recovery() {
        let temp_dir = TempDir::new().unwrap();

        // Создаем WAL и записываем операции
        {
            let mut wal = WalManager::new(temp_dir.path()).unwrap();

            let columns = vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                foreign_key: None,
                unique: false,
            }];

            let table = Table::new("users".to_string(), columns);
            wal.append(Operation::CreateTable {
                table_name: "users".to_string(),
                table,
            })
            .unwrap();

            wal.append(Operation::Insert {
                table_name: "users".to_string(),
                row: Row::new(vec![Value::Integer(1)]),
            })
            .unwrap();
        }

        // Восстанавливаем WAL (имитация перезапуска)
        {
            let wal = WalManager::new(temp_dir.path()).unwrap();
            assert_eq!(wal.current_sequence, 2);

            let logs = wal.read_all_logs().unwrap();
            assert_eq!(logs.len(), 2);
        }
    }

    #[test]
    fn test_cleanup_old_logs() {
        let temp_dir = TempDir::new().unwrap();
        let mut wal = WalManager::new(temp_dir.path()).unwrap();

        // Создаем несколько WAL файлов
        for _ in 0..5 {
            wal.append(Operation::Checkpoint { timestamp: 0 }).unwrap();
            wal.rotate_wal().unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Оставляем только 2 последних
        wal.cleanup_old_logs(2).unwrap();

        let wal_files: Vec<_> = fs::read_dir(temp_dir.path().join("wal"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("wal"))
            .collect();

        assert!(wal_files.len() <= 2);
    }
}
