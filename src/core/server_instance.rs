use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::database::Database;
use super::database_metadata::DatabaseMetadata;
use super::user::User;
use super::privilege::Privilege;
use super::error::DatabaseError;

/// Корневой объект сервера - содержит все БД и пользователей
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInstance {
    /// Все базы данных: name -> Database
    pub databases: HashMap<String, Database>,
    /// Метаданные баз данных: name -> DatabaseMetadata
    pub database_metadata: HashMap<String, DatabaseMetadata>,
    /// Все пользователи: username -> User
    pub users: HashMap<String, User>,
}

impl ServerInstance {
    pub fn new() -> Self {
        Self {
            databases: HashMap::new(),
            database_metadata: HashMap::new(),
            users: HashMap::new(),
        }
    }

    /// Создает начальную конфигурацию (суперпользователь + БД)
    pub fn initialize(
        superuser_name: &str,
        superuser_password: &str,
        initial_db_name: &str,
    ) -> Self {
        let mut instance = Self::new();

        // Создаем суперпользователя
        let superuser = User::new(superuser_name.to_string(), superuser_password, true);
        instance.users.insert(superuser_name.to_string(), superuser);

        // Создаем начальную БД
        let db = Database::new(initial_db_name.to_string());
        let db_meta = DatabaseMetadata::new(initial_db_name.to_string(), superuser_name.to_string());

        instance.databases.insert(initial_db_name.to_string(), db);
        instance.database_metadata.insert(initial_db_name.to_string(), db_meta);

        instance
    }

    /// Создает пользователя
    pub fn create_user(&mut self, username: &str, password: &str, is_superuser: bool) -> Result<(), DatabaseError> {
        if self.users.contains_key(username) {
            return Err(DatabaseError::UserAlreadyExists(username.to_string()));
        }
        let user = User::new(username.to_string(), password, is_superuser);
        self.users.insert(username.to_string(), user);
        Ok(())
    }

    /// Удаляет пользователя
    pub fn drop_user(&mut self, username: &str) -> Result<(), DatabaseError> {
        if !self.users.contains_key(username) {
            return Err(DatabaseError::UserNotFound(username.to_string()));
        }
        self.users.remove(username);
        Ok(())
    }

    /// Создает базу данных
    pub fn create_database(&mut self, db_name: &str, owner: &str) -> Result<(), DatabaseError> {
        if self.databases.contains_key(db_name) {
            return Err(DatabaseError::DatabaseAlreadyExists(db_name.to_string()));
        }
        if !self.users.contains_key(owner) {
            return Err(DatabaseError::UserNotFound(owner.to_string()));
        }

        let db = Database::new(db_name.to_string());
        let db_meta = DatabaseMetadata::new(db_name.to_string(), owner.to_string());

        self.databases.insert(db_name.to_string(), db);
        self.database_metadata.insert(db_name.to_string(), db_meta);

        Ok(())
    }

    /// Удаляет базу данных
    pub fn drop_database(&mut self, db_name: &str) -> Result<(), DatabaseError> {
        if !self.databases.contains_key(db_name) {
            return Err(DatabaseError::DatabaseNotFound(db_name.to_string()));
        }
        self.databases.remove(db_name);
        self.database_metadata.remove(db_name);
        Ok(())
    }

    /// Получает БД
    pub fn get_database(&self, name: &str) -> Option<&Database> {
        self.databases.get(name)
    }

    /// Получает мутабельную БД
    pub fn get_database_mut(&mut self, name: &str) -> Option<&mut Database> {
        self.databases.get_mut(name)
    }

    /// Получает метаданные БД
    pub fn get_database_metadata(&self, name: &str) -> Option<&DatabaseMetadata> {
        self.database_metadata.get(name)
    }

    /// Получает мутабельные метаданные БД
    pub fn get_database_metadata_mut(&mut self, name: &str) -> Option<&mut DatabaseMetadata> {
        self.database_metadata.get_mut(name)
    }

    /// Проверяет пароль пользователя
    pub fn authenticate(&self, username: &str, password: &str) -> bool {
        if let Some(user) = self.users.get(username) {
            user.verify_password(password)
        } else {
            false
        }
    }

    /// Проверяет, есть ли у пользователя право на БД
    pub fn check_privilege(&self, username: &str, db_name: &str, privilege: &Privilege) -> Result<bool, DatabaseError> {
        // Суперпользователь имеет все права
        if let Some(user) = self.users.get(username) {
            if user.is_superuser {
                return Ok(true);
            }
        }

        // Проверяем права в метаданных БД
        if let Some(db_meta) = self.database_metadata.get(db_name) {
            Ok(db_meta.has_privilege(username, privilege))
        } else {
            Err(DatabaseError::DatabaseNotFound(db_name.to_string()))
        }
    }
}

impl Default for ServerInstance {
    fn default() -> Self {
        Self::new()
    }
}
