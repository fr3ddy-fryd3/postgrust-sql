use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use super::privilege::Privilege;

/// Метаданные базы данных (владелец и права доступа)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseMetadata {
    pub name: String,
    pub owner: String,
    /// Права доступа: username -> set of privileges
    pub privileges: HashMap<String, HashSet<Privilege>>,
}

impl DatabaseMetadata {
    pub fn new(name: String, owner: String) -> Self {
        let mut privileges = HashMap::new();
        // Владелец получает все права автоматически
        privileges.insert(
            owner.clone(),
            vec![Privilege::All].into_iter().collect(),
        );
        Self {
            name,
            owner,
            privileges,
        }
    }

    /// Выдает права пользователю
    pub fn grant(&mut self, username: &str, privilege: Privilege) {
        self.privileges
            .entry(username.to_string())
            .or_insert_with(HashSet::new)
            .insert(privilege);
    }

    /// Отбирает права у пользователя
    pub fn revoke(&mut self, username: &str, privilege: &Privilege) {
        if let Some(privs) = self.privileges.get_mut(username) {
            privs.remove(privilege);
        }
    }

    /// Проверяет, есть ли у пользователя право
    pub fn has_privilege(&self, username: &str, privilege: &Privilege) -> bool {
        if let Some(privs) = self.privileges.get(username) {
            privs.contains(&Privilege::All) || privs.contains(privilege)
        } else {
            false
        }
    }
}
