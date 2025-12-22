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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Table;
    use crate::types::Column;
    use crate::types::DataType;

    fn create_test_instance() -> ServerInstance {
        ServerInstance::initialize("postgres", "password", "testdb")
    }

    #[test]
    fn test_create_role() {
        let mut inst = create_test_instance();

        // Create a regular role
        inst.create_role("readonly", false).unwrap();
        assert!(inst.roles.contains_key("readonly"));
        assert!(!inst.roles.get("readonly").unwrap().is_superuser);

        // Create a superuser role
        inst.create_role("admin", true).unwrap();
        assert!(inst.roles.contains_key("admin"));
        assert!(inst.roles.get("admin").unwrap().is_superuser);

        // Try to create duplicate role - should fail
        let result = inst.create_role("readonly", false);
        assert!(result.is_err());
    }

    #[test]
    fn test_drop_role() {
        let mut inst = create_test_instance();

        inst.create_role("temp_role", false).unwrap();
        assert!(inst.roles.contains_key("temp_role"));

        inst.drop_role("temp_role").unwrap();
        assert!(!inst.roles.contains_key("temp_role"));

        // Try to drop non-existent role - should fail
        let result = inst.drop_role("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_grant_revoke_role() {
        let mut inst = create_test_instance();

        // Create user and role
        inst.create_user("alice", "password", false).unwrap();
        inst.create_role("developers", false).unwrap();

        // Grant role to user
        inst.grant_role_to_user("developers", "alice").unwrap();
        let user = inst.users.get("alice").unwrap();
        assert!(user.roles.contains("developers"));

        let role = inst.roles.get("developers").unwrap();
        assert!(role.members.contains("alice"));

        // Revoke role from user
        inst.revoke_role_from_user("developers", "alice").unwrap();
        let user = inst.users.get("alice").unwrap();
        assert!(!user.roles.contains("developers"));

        let role = inst.roles.get("developers").unwrap();
        assert!(!role.members.contains("alice"));
    }

    #[test]
    fn test_role_hierarchy() {
        let mut inst = create_test_instance();

        // Create role hierarchy: developer -> readonly
        inst.create_role("readonly", false).unwrap();
        inst.create_role("developer", false).unwrap();

        // Make developer inherit readonly
        inst.roles.get_mut("developer").unwrap().add_parent_role("readonly");

        // Create user with developer role
        inst.create_user("bob", "password", false).unwrap();
        inst.grant_role_to_user("developer", "bob").unwrap();

        // Get all roles (should include both developer and readonly)
        let all_roles = inst.get_user_roles("bob");
        assert!(all_roles.contains("developer"));
        assert!(all_roles.contains("readonly"));
    }

    #[test]
    fn test_table_ownership() {
        let mut inst = create_test_instance();

        // Create user
        inst.create_user("alice", "password", false).unwrap();

        // Create table with owner
        let table = Table::new_with_owner(
            "users".to_string(),
            vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            }],
            "alice".to_string(),
        );

        {
            let db = inst.get_database_mut("testdb").unwrap();
            db.create_table(table).unwrap();
        } // Drop mutable borrow

        // Check ownership
        let db = inst.get_database("testdb").unwrap();
        assert!(db.is_table_owner("alice", "users"));
        assert!(!db.is_table_owner("postgres", "users"));
    }

    #[test]
    fn test_table_permission_checks() {
        let mut inst = create_test_instance();

        // Create users
        inst.create_user("alice", "password", false).unwrap();
        inst.create_user("bob", "password", false).unwrap();

        // Alice creates a table
        let table = Table::new_with_owner(
            "orders".to_string(),
            vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            }],
            "alice".to_string(),
        );

        {
            let db = inst.get_database_mut("testdb").unwrap();
            db.create_table(table).unwrap();
        } // Drop mutable borrow

        // Alice (owner) should have all permissions
        assert!(inst.check_table_permission("alice", "testdb", "orders", &Privilege::Select));
        assert!(inst.check_table_permission("alice", "testdb", "orders", &Privilege::Insert));

        // Bob should NOT have permissions
        assert!(!inst.check_table_permission("bob", "testdb", "orders", &Privilege::Select));

        // Grant SELECT to bob
        {
            let db = inst.get_database_mut("testdb").unwrap();
            let metadata = db.table_metadata.get_mut("orders").unwrap();
            metadata.grant("bob", Privilege::Select);
        } // Drop mutable borrow

        // Now bob should have SELECT but not INSERT
        assert!(inst.check_table_permission("bob", "testdb", "orders", &Privilege::Select));
        assert!(!inst.check_table_permission("bob", "testdb", "orders", &Privilege::Insert));
    }

    #[test]
    fn test_superuser_permissions() {
        let mut inst = create_test_instance();

        // Create regular user
        inst.create_user("alice", "password", false).unwrap();

        // Alice creates a table
        let table = Table::new_with_owner(
            "data".to_string(),
            vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            }],
            "alice".to_string(),
        );

        {
            let db = inst.get_database_mut("testdb").unwrap();
            db.create_table(table).unwrap();
        } // Drop mutable borrow

        // postgres (superuser) should have all permissions even without grants
        assert!(inst.check_table_permission("postgres", "testdb", "data", &Privilege::Select));
        assert!(inst.check_table_permission("postgres", "testdb", "data", &Privilege::Insert));
        assert!(inst.check_table_permission("postgres", "testdb", "data", &Privilege::Delete));
    }

    #[test]
    fn test_role_based_permissions() {
        let mut inst = create_test_instance();

        // Create role and user
        inst.create_role("readers", false).unwrap();
        inst.create_user("bob", "password", false).unwrap();
        inst.grant_role_to_user("readers", "bob").unwrap();

        // Create table
        inst.create_user("alice", "password", false).unwrap();
        let table = Table::new_with_owner(
            "products".to_string(),
            vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            }],
            "alice".to_string(),
        );

        {
            let db = inst.get_database_mut("testdb").unwrap();
            db.create_table(table).unwrap();

            // Grant SELECT to readers role
            let metadata = db.table_metadata.get_mut("products").unwrap();
            metadata.grant("readers", Privilege::Select);
        } // Drop mutable borrow

        // Bob should have SELECT through role membership
        assert!(inst.check_table_permission("bob", "testdb", "products", &Privilege::Select));
        assert!(!inst.check_table_permission("bob", "testdb", "products", &Privilege::Insert));
    }

    #[test]
    fn test_is_table_owner_or_superuser() {
        let mut inst = create_test_instance();

        // Create user
        inst.create_user("alice", "password", false).unwrap();

        // Alice creates a table
        let table = Table::new_with_owner(
            "test_table".to_string(),
            vec![Column {
                name: "id".to_string(),
                data_type: DataType::Integer,
                nullable: false,
                primary_key: true,
                unique: false,
                foreign_key: None,
            }],
            "alice".to_string(),
        );

        {
            let db = inst.get_database_mut("testdb").unwrap();
            db.create_table(table).unwrap();
        } // Drop mutable borrow

        // Alice should be owner
        assert!(inst.is_table_owner_or_superuser("alice", "testdb", "test_table"));

        // postgres should be superuser
        assert!(inst.is_table_owner_or_superuser("postgres", "testdb", "test_table"));

        // Create another user who is neither owner nor superuser
        inst.create_user("bob", "password", false).unwrap();
        assert!(!inst.is_table_owner_or_superuser("bob", "testdb", "test_table"));
    }
}
