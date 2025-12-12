use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
        }
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
