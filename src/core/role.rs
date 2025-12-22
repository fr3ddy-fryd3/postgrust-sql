use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Роль базы данных (группа пользователей с общими правами)
///
/// В PostgreSQL роли могут быть:
/// - User roles (с возможностью логина) - хранятся в User
/// - Group roles (без логина, только для группировки прав)
///
/// Наша реализация: Role = группа без логина, User = роль с логином
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    /// Является ли суперролью (полные права на всё)
    pub is_superuser: bool,
    /// Права на уровне сервера
    pub can_create_db: bool,
    pub can_create_role: bool,
    /// Пользователи, которым выдана эта роль (members)
    pub members: HashSet<String>,
    /// Роли, которые наследует эта роль (member_of)
    /// Например: analyst наследует readonly
    pub member_of: HashSet<String>,
}

impl Role {
    /// Создает новую роль
    #[must_use]
    pub fn new(name: String, is_superuser: bool) -> Self {
        Self {
            name,
            is_superuser,
            can_create_db: is_superuser,
            can_create_role: is_superuser,
            members: HashSet::new(),
            member_of: HashSet::new(),
        }
    }

    /// Добавляет пользователя в роль
    pub fn add_member(&mut self, username: &str) {
        self.members.insert(username.to_string());
    }

    /// Удаляет пользователя из роли
    pub fn remove_member(&mut self, username: &str) {
        self.members.remove(username);
    }

    /// Проверяет, является ли пользователь членом роли
    #[must_use]
    pub fn has_member(&self, username: &str) -> bool {
        self.members.contains(username)
    }

    /// Добавляет роль, которую наследует текущая роль
    pub fn add_parent_role(&mut self, role_name: &str) {
        self.member_of.insert(role_name.to_string());
    }

    /// Удаляет наследуемую роль
    pub fn remove_parent_role(&mut self, role_name: &str) {
        self.member_of.remove(role_name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_role() {
        let role = Role::new("readonly".to_string(), false);
        assert_eq!(role.name, "readonly");
        assert!(!role.is_superuser);
        assert!(!role.can_create_db);
        assert!(role.members.is_empty());
    }

    #[test]
    fn test_superuser_role() {
        let role = Role::new("admin".to_string(), true);
        assert!(role.is_superuser);
        assert!(role.can_create_db);
        assert!(role.can_create_role);
    }

    #[test]
    fn test_add_remove_member() {
        let mut role = Role::new("developers".to_string(), false);

        role.add_member("alice");
        role.add_member("bob");
        assert_eq!(role.members.len(), 2);
        assert!(role.has_member("alice"));
        assert!(role.has_member("bob"));

        role.remove_member("alice");
        assert_eq!(role.members.len(), 1);
        assert!(!role.has_member("alice"));
        assert!(role.has_member("bob"));
    }

    #[test]
    fn test_role_inheritance() {
        let mut analyst = Role::new("analyst".to_string(), false);
        analyst.add_parent_role("readonly");

        assert!(analyst.member_of.contains("readonly"));

        analyst.remove_parent_role("readonly");
        assert!(analyst.member_of.is_empty());
    }
}
