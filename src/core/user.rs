use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Пользователь базы данных
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    /// SHA-256 хэш пароля (hex string)
    pub password_hash: String,
    /// Является ли суперпользователем (полные права на всё)
    pub is_superuser: bool,
    /// Права на уровне сервера
    pub can_create_db: bool,
    pub can_create_user: bool,
    /// Роли, к которым принадлежит пользователь
    pub roles: HashSet<String>,
}

impl User {
    #[must_use]
    pub fn new(username: String, password: &str, is_superuser: bool) -> Self {
        Self {
            username,
            password_hash: Self::hash_password(password),
            is_superuser,
            can_create_db: is_superuser,
            can_create_user: is_superuser,
            roles: HashSet::new(),
        }
    }

    /// Добавляет роль пользователю
    pub fn add_role(&mut self, role_name: &str) {
        self.roles.insert(role_name.to_string());
    }

    /// Удаляет роль у пользователя
    pub fn remove_role(&mut self, role_name: &str) {
        self.roles.remove(role_name);
    }

    /// Проверяет, имеет ли пользователь роль
    #[must_use]
    pub fn has_role(&self, role_name: &str) -> bool {
        self.roles.contains(role_name)
    }

    /// Хэширует пароль с использованием SHA-256
    #[must_use] 
    pub fn hash_password(password: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Проверяет пароль
    #[must_use] 
    pub fn verify_password(&self, password: &str) -> bool {
        self.password_hash == Self::hash_password(password)
    }

    /// Меняет пароль
    pub fn set_password(&mut self, password: &str) {
        self.password_hash = Self::hash_password(password);
    }
}
