use serde::{Deserialize, Serialize};

/// Права доступа (privileges) как в PostgreSQL
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Privilege {
    /// Право подключения к базе данных
    Connect,
    /// Право создания объектов в БД
    Create,
    /// Право на SELECT
    Select,
    /// Право на INSERT
    Insert,
    /// Право на UPDATE
    Update,
    /// Право на DELETE
    Delete,
    /// Все права
    All,
}
