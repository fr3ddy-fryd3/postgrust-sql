use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use super::table::Table;
use super::table_metadata::TableMetadata;
use super::error::DatabaseError;
use crate::index::Index;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Database {
    pub name: String,
    pub tables: HashMap<String, Table>,
    pub enums: HashMap<String, Vec<String>>, // enum_name -> allowed values
    #[serde(skip)] // Don't serialize indexes (rebuild on load)
    pub indexes: HashMap<String, Index>, // index_name -> Index (BTree or Hash)
    pub views: HashMap<String, String>, // view_name -> SQL query (v1.10.0)
    /// v2.3.0: Table metadata (owner + privileges)
    pub table_metadata: HashMap<String, TableMetadata>, // table_name -> TableMetadata
}

impl Database {
    #[must_use]
    pub fn new(name: String) -> Self {
        Self {
            name,
            tables: HashMap::new(),
            enums: HashMap::new(),
            indexes: HashMap::new(),
            views: HashMap::new(),
            table_metadata: HashMap::new(),
        }
    }

    pub fn create_enum(&mut self, name: String, values: Vec<String>) -> Result<(), DatabaseError> {
        if self.enums.contains_key(&name) {
            return Err(DatabaseError::ParseError(format!("Enum '{name}' already exists")));
        }
        self.enums.insert(name, values);
        Ok(())
    }

    #[must_use] 
    pub fn get_enum(&self, name: &str) -> Option<&Vec<String>> {
        self.enums.get(name)
    }

    pub fn create_table(&mut self, table: Table) -> Result<(), DatabaseError> {
        if self.tables.contains_key(&table.name) {
            return Err(DatabaseError::TableAlreadyExists(table.name));
        }

        // v2.3.0: Create table metadata with owner
        let metadata = TableMetadata::new(table.name.clone(), table.owner.clone());
        self.table_metadata.insert(table.name.clone(), metadata);

        self.tables.insert(table.name.clone(), table);
        Ok(())
    }

    #[must_use] 
    pub fn get_table(&self, name: &str) -> Option<&Table> {
        self.tables.get(name)
    }

    pub fn get_table_mut(&mut self, name: &str) -> Option<&mut Table> {
        self.tables.get_mut(name)
    }

    pub fn drop_table(&mut self, name: &str) -> Result<(), DatabaseError> {
        self.tables
            .remove(name)
            .ok_or_else(|| DatabaseError::TableNotFound(name.to_string()))?;

        // v2.3.0: Remove table metadata
        self.table_metadata.remove(name);

        Ok(())
    }
}
