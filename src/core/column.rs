use serde::{Deserialize, Serialize};
use super::data_type::DataType;
use super::constraints::ForeignKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub foreign_key: Option<ForeignKey>,
}
