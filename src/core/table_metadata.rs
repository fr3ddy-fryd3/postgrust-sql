use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use super::privilege::Privilege;

/// Метаданные таблицы (владелец и права доступа)
///
/// Хранит информацию о правах доступа к таблице для пользователей и ролей.
/// Владелец таблицы автоматически получает все права (Privilege::All).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableMetadata {
    pub table_name: String,
    pub owner: String,
    /// Права доступа: username/role_name -> set of privileges
    pub privileges: HashMap<String, HashSet<Privilege>>,
}

impl TableMetadata {
    /// Создает новые метаданные таблицы
    #[must_use]
    pub fn new(table_name: String, owner: String) -> Self {
        let mut privileges = HashMap::new();
        // Владелец получает все права автоматически
        privileges.insert(
            owner.clone(),
            vec![Privilege::All].into_iter().collect(),
        );
        Self {
            table_name,
            owner,
            privileges,
        }
    }

    /// Выдает права пользователю или роли
    pub fn grant(&mut self, grantee: &str, privilege: Privilege) {
        self.privileges
            .entry(grantee.to_string())
            .or_default()
            .insert(privilege);
    }

    /// Отбирает права у пользователя или роли
    pub fn revoke(&mut self, grantee: &str, privilege: &Privilege) {
        if let Some(privs) = self.privileges.get_mut(grantee) {
            privs.remove(privilege);
            // Если прав не осталось, удаляем запись
            if privs.is_empty() {
                self.privileges.remove(grantee);
            }
        }
    }

    /// Проверяет, есть ли у пользователя/роли право
    #[must_use]
    pub fn has_privilege(&self, grantee: &str, privilege: &Privilege) -> bool {
        if let Some(privs) = self.privileges.get(grantee) {
            privs.contains(&Privilege::All) || privs.contains(privilege)
        } else {
            false
        }
    }

    /// Проверяет, является ли пользователь владельцем таблицы
    #[must_use]
    pub fn is_owner(&self, username: &str) -> bool {
        self.owner == username
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_table_metadata() {
        let meta = TableMetadata::new("users".to_string(), "alice".to_string());
        assert_eq!(meta.table_name, "users");
        assert_eq!(meta.owner, "alice");
        assert!(meta.has_privilege("alice", &Privilege::All));
        assert!(meta.is_owner("alice"));
    }

    #[test]
    fn test_grant_privilege() {
        let mut meta = TableMetadata::new("users".to_string(), "alice".to_string());

        meta.grant("bob", Privilege::Select);
        assert!(meta.has_privilege("bob", &Privilege::Select));
        assert!(!meta.has_privilege("bob", &Privilege::Insert));
    }

    #[test]
    fn test_revoke_privilege() {
        let mut meta = TableMetadata::new("users".to_string(), "alice".to_string());

        meta.grant("bob", Privilege::Select);
        meta.grant("bob", Privilege::Insert);
        assert!(meta.has_privilege("bob", &Privilege::Select));

        meta.revoke("bob", &Privilege::Select);
        assert!(!meta.has_privilege("bob", &Privilege::Select));
        assert!(meta.has_privilege("bob", &Privilege::Insert));
    }

    #[test]
    fn test_privilege_all() {
        let mut meta = TableMetadata::new("users".to_string(), "alice".to_string());

        // Owner has All privilege
        assert!(meta.has_privilege("alice", &Privilege::Select));
        assert!(meta.has_privilege("alice", &Privilege::Insert));
        assert!(meta.has_privilege("alice", &Privilege::Update));
        assert!(meta.has_privilege("alice", &Privilege::Delete));
    }
}
