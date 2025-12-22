use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use super::database::Database;
use super::database_metadata::DatabaseMetadata;
use super::user::User;
use super::role::Role;
use super::privilege::Privilege;
use super::error::DatabaseError;

/// Корневой объект сервера - содержит все БД и пользователей
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInstance {
    /// Все базы данных: name -> Database
    pub databases: HashMap<String, Database>,
    /// Метаданные баз данных: name -> `DatabaseMetadata`
    pub database_metadata: HashMap<String, DatabaseMetadata>,
    /// Все пользователи: username -> User
    pub users: HashMap<String, User>,
    /// Все роли: role_name -> Role
    pub roles: HashMap<String, Role>,
}

impl ServerInstance {
    #[must_use]
    pub fn new() -> Self {
        Self {
            databases: HashMap::new(),
            database_metadata: HashMap::new(),
            users: HashMap::new(),
            roles: HashMap::new(),
        }
    }

    /// Создает начальную конфигурацию (суперпользователь + БД)
    #[must_use] 
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
    #[must_use] 
    pub fn get_database(&self, name: &str) -> Option<&Database> {
        self.databases.get(name)
    }

    /// Получает мутабельную БД
    pub fn get_database_mut(&mut self, name: &str) -> Option<&mut Database> {
        self.databases.get_mut(name)
    }

    /// Получает метаданные БД
    #[must_use] 
    pub fn get_database_metadata(&self, name: &str) -> Option<&DatabaseMetadata> {
        self.database_metadata.get(name)
    }

    /// Получает мутабельные метаданные БД
    pub fn get_database_metadata_mut(&mut self, name: &str) -> Option<&mut DatabaseMetadata> {
        self.database_metadata.get_mut(name)
    }

    /// Проверяет пароль пользователя
    #[must_use] 
    pub fn authenticate(&self, username: &str, password: &str) -> bool {
        if let Some(user) = self.users.get(username) {
            user.verify_password(password)
        } else {
            false
        }
    }

    /// Создает роль
    pub fn create_role(&mut self, role_name: &str, is_superuser: bool) -> Result<(), DatabaseError> {
        if self.roles.contains_key(role_name) {
            return Err(DatabaseError::RoleAlreadyExists(role_name.to_string()));
        }
        let role = Role::new(role_name.to_string(), is_superuser);
        self.roles.insert(role_name.to_string(), role);
        Ok(())
    }

    /// Удаляет роль
    pub fn drop_role(&mut self, role_name: &str) -> Result<(), DatabaseError> {
        if !self.roles.contains_key(role_name) {
            return Err(DatabaseError::RoleNotFound(role_name.to_string()));
        }

        // Удаляем роль у всех пользователей
        for user in self.users.values_mut() {
            user.remove_role(role_name);
        }

        // Удаляем из наследования других ролей
        for role in self.roles.values_mut() {
            role.remove_parent_role(role_name);
            role.remove_member(role_name);
        }

        self.roles.remove(role_name);
        Ok(())
    }

    /// Выдает роль пользователю (GRANT role TO user)
    pub fn grant_role_to_user(&mut self, role_name: &str, username: &str) -> Result<(), DatabaseError> {
        if !self.roles.contains_key(role_name) {
            return Err(DatabaseError::RoleNotFound(role_name.to_string()));
        }
        if !self.users.contains_key(username) {
            return Err(DatabaseError::UserNotFound(username.to_string()));
        }

        // Добавляем роль пользователю
        if let Some(user) = self.users.get_mut(username) {
            user.add_role(role_name);
        }

        // Добавляем пользователя в члены роли
        if let Some(role) = self.roles.get_mut(role_name) {
            role.add_member(username);
        }

        Ok(())
    }

    /// Отбирает роль у пользователя (REVOKE role FROM user)
    pub fn revoke_role_from_user(&mut self, role_name: &str, username: &str) -> Result<(), DatabaseError> {
        if !self.roles.contains_key(role_name) {
            return Err(DatabaseError::RoleNotFound(role_name.to_string()));
        }
        if !self.users.contains_key(username) {
            return Err(DatabaseError::UserNotFound(username.to_string()));
        }

        // Удаляем роль у пользователя
        if let Some(user) = self.users.get_mut(username) {
            user.remove_role(role_name);
        }

        // Удаляем пользователя из членов роли
        if let Some(role) = self.roles.get_mut(role_name) {
            role.remove_member(username);
        }

        Ok(())
    }

    /// Получает все роли пользователя (включая наследуемые)
    pub fn get_user_roles(&self, username: &str) -> HashSet<String> {
        let mut all_roles = HashSet::new();

        if let Some(user) = self.users.get(username) {
            // Добавляем прямые роли
            for role_name in &user.roles {
                self.collect_roles_recursive(role_name, &mut all_roles);
            }
        }

        all_roles
    }

    /// Рекурсивно собирает все роли (включая наследуемые)
    fn collect_roles_recursive(&self, role_name: &str, collected: &mut HashSet<String>) {
        if collected.contains(role_name) {
            return; // Избегаем циклов
        }

        collected.insert(role_name.to_string());

        if let Some(role) = self.roles.get(role_name) {
            for parent_role in &role.member_of {
                self.collect_roles_recursive(parent_role, collected);
            }
        }
    }

    /// Проверяет, есть ли у пользователя право на БД (с учетом ролей)
    pub fn check_privilege(&self, username: &str, db_name: &str, privilege: &Privilege) -> Result<bool, DatabaseError> {
        // Суперпользователь имеет все права
        if let Some(user) = self.users.get(username)
            && user.is_superuser {
                return Ok(true);
            }

        // Проверяем права в метаданных БД для пользователя
        if let Some(db_meta) = self.database_metadata.get(db_name) {
            if db_meta.has_privilege(username, privilege) {
                return Ok(true);
            }

            // Проверяем права через роли
            let user_roles = self.get_user_roles(username);
            for role_name in user_roles {
                // Проверяем, есть ли у роли суперправа
                if let Some(role) = self.roles.get(&role_name) {
                    if role.is_superuser {
                        return Ok(true);
                    }
                }
                // Проверяем права роли в БД
                if db_meta.has_privilege(&role_name, privilege) {
                    return Ok(true);
                }
            }

            Ok(false)
        } else {
            Err(DatabaseError::DatabaseNotFound(db_name.to_string()))
        }
    }

    /// v2.3.0: Проверяет, есть ли у пользователя право на таблицу (с учетом ролей)
    ///
    /// Возвращает true если:
    /// - Пользователь - суперпользователь
    /// - Пользователь - владелец таблицы
    /// - Пользователь имеет данное право на таблицу
    /// - Одна из ролей пользователя имеет это право
    #[must_use]
    pub fn check_table_permission(
        &self,
        username: &str,
        db_name: &str,
        table_name: &str,
        privilege: &Privilege,
    ) -> bool {
        // Суперпользователь имеет все права
        if let Some(user) = self.users.get(username) {
            if user.is_superuser {
                return true;
            }
        }

        // Проверяем через роли на суперпользователя
        let user_roles = self.get_user_roles(username);
        for role_name in &user_roles {
            if let Some(role) = self.roles.get(role_name) {
                if role.is_superuser {
                    return true;
                }
            }
        }

        // Проверяем права на таблицу в базе данных
        if let Some(db) = self.databases.get(db_name) {
            // Проверяем права пользователя
            if db.check_table_permission(username, table_name, privilege.clone()) {
                return true;
            }

            // Проверяем права через роли
            for role_name in &user_roles {
                if db.check_table_permission(role_name, table_name, privilege.clone()) {
                    return true;
                }
            }
        }

        false
    }

    /// v2.3.0: Проверяет, является ли пользователь владельцем таблицы или суперпользователем
    #[must_use]
    pub fn is_table_owner_or_superuser(
        &self,
        username: &str,
        db_name: &str,
        table_name: &str,
    ) -> bool {
        // Суперпользователь имеет все права
        if let Some(user) = self.users.get(username) {
            if user.is_superuser {
                return true;
            }
        }

        // Проверяем через роли на суперпользователя
        let user_roles = self.get_user_roles(username);
        for role_name in &user_roles {
            if let Some(role) = self.roles.get(role_name) {
                if role.is_superuser {
                    return true;
                }
            }
        }

        // Проверяем владение таблицей
        if let Some(db) = self.databases.get(db_name) {
            db.is_table_owner(username, table_name)
        } else {
            false
        }
    }
}

impl Default for ServerInstance {
    fn default() -> Self {
        Self::new()
    }
}
