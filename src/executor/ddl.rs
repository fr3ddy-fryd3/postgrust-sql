/// DDL (Data Definition Language) operations
///
/// CREATE TABLE, DROP TABLE, ALTER TABLE, SHOW TABLES

use crate::types::{Database, DatabaseError, Table, Column, DataType};
use crate::parser::{ColumnDef, AlterTableOperation};
use crate::storage::StorageEngine;
use super::legacy_executor::QueryResult;

pub struct DdlExecutor;

impl DdlExecutor {
    /// Execute CREATE TABLE statement
    ///
    /// Validates:
    /// - ENUM type resolution from db.enums
    /// - Foreign key references (table/column existence, PRIMARY KEY)
    pub fn create_table(
        db: &mut Database,
        name: String,
        column_defs: Vec<ColumnDef>,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        // Build columns from column definitions
        let columns: Vec<Column> = column_defs
            .into_iter()
            .map(|def| {
                // Resolve enum types: if data_type is Enum with empty values, look up in db.enums
                let data_type = match def.data_type {
                    DataType::Enum { ref name, ref values } if values.is_empty() => {
                        // Look up enum values from database
                        if let Some(enum_values) = db.enums.get(name) {
                            DataType::Enum {
                                name: name.clone(),
                                values: enum_values.clone(),
                            }
                        } else {
                            return Err(DatabaseError::ParseError(format!(
                                "Unknown enum type '{}'", name
                            )));
                        }
                    }
                    other => other,
                };

                Ok(Column {
                    name: def.name.clone(),
                    data_type,
                    nullable: def.nullable,
                    primary_key: def.primary_key,
                    unique: def.unique,
                    foreign_key: def.foreign_key.clone(),
                })
            })
            .collect::<Result<Vec<Column>, DatabaseError>>()?;

        // Validate foreign key references
        for col in &columns {
            if let Some(ref fk) = col.foreign_key {
                // Check if referenced table exists
                let ref_table = db
                    .get_table(&fk.referenced_table)
                    .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                        format!("Referenced table '{}' does not exist", fk.referenced_table)
                    ))?;

                // Check if referenced column exists and is a primary key
                let ref_col = ref_table.columns.iter().find(|c| c.name == fk.referenced_column)
                    .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                        format!("Referenced column '{}' does not exist in table '{}'",
                                fk.referenced_column, fk.referenced_table)
                    ))?;

                if !ref_col.primary_key {
                    return Err(DatabaseError::ForeignKeyViolation(
                        format!("Referenced column '{}' must be a primary key", fk.referenced_column)
                    ));
                }
            }
        }

        let table = Table::new(name.clone(), columns);

        // Log to WAL before executing
        if let Some(storage) = storage {
            storage.log_create_table(&table)?;
        }

        db.create_table(table)?;
        Ok(QueryResult::Success(format!(
            "Table '{}' created successfully",
            name
        )))
    }

    /// Execute DROP TABLE statement
    pub fn drop_table(
        db: &mut Database,
        name: String,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        // Log to WAL before executing
        if let Some(storage) = storage {
            storage.log_drop_table(&name)?;
        }

        db.drop_table(&name)?;
        Ok(QueryResult::Success(format!(
            "Table '{}' dropped successfully",
            name
        )))
    }

    /// Execute ALTER TABLE statement
    ///
    /// Operations:
    /// - ADD COLUMN
    /// - DROP COLUMN
    /// - RENAME COLUMN
    /// - RENAME TO (table rename)
    pub fn alter_table(
        db: &mut Database,
        table_name: String,
        operation: AlterTableOperation,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        use AlterTableOperation::*;

        match operation {
            AddColumn(column_def) => {
                Self::alter_table_add_column(db, &table_name, column_def, storage)
            }
            DropColumn(column_name) => {
                Self::alter_table_drop_column(db, &table_name, column_name, storage)
            }
            RenameColumn { old_name, new_name } => {
                Self::alter_table_rename_column(db, &table_name, old_name, new_name, storage)
            }
            RenameTable(new_name) => {
                Self::alter_table_rename_table(db, &table_name, new_name, storage)
            }
        }
    }

    /// ALTER TABLE ADD COLUMN
    fn alter_table_add_column(
        db: &mut Database,
        table_name: &str,
        column_def: ColumnDef,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        // First, do all validations (immutable borrows)
        {
            let table = db.get_table(table_name)
                .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

            // Check if column already exists
            if table.columns.iter().any(|c| c.name == column_def.name) {
                return Err(DatabaseError::ParseError(format!(
                    "Column '{}' already exists in table '{}'",
                    column_def.name, table_name
                )));
            }
        }

        // Resolve ENUM type if needed
        let data_type = match column_def.data_type {
            DataType::Enum { ref name, ref values } if values.is_empty() => {
                if let Some(enum_values) = db.enums.get(name) {
                    DataType::Enum {
                        name: name.clone(),
                        values: enum_values.clone(),
                    }
                } else {
                    return Err(DatabaseError::ParseError(format!(
                        "Unknown enum type '{}'", name
                    )));
                }
            }
            other => other,
        };

        // Validate foreign key if present
        if let Some(ref fk) = column_def.foreign_key {
            let ref_table = db.get_table(&fk.referenced_table)
                .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                    format!("Referenced table '{}' does not exist", fk.referenced_table)
                ))?;

            let ref_col = ref_table.columns.iter().find(|c| c.name == fk.referenced_column)
                .ok_or_else(|| DatabaseError::ForeignKeyViolation(
                    format!("Referenced column '{}' does not exist", fk.referenced_column)
                ))?;

            if !ref_col.primary_key {
                return Err(DatabaseError::ForeignKeyViolation(
                    format!("Referenced column must be PRIMARY KEY")
                ));
            }
        }

        let new_column = Column {
            name: column_def.name.clone(),
            data_type,
            nullable: column_def.nullable,
            primary_key: column_def.primary_key,
            unique: column_def.unique,
            foreign_key: column_def.foreign_key.clone(),
        };

        // Log to WAL
        if let Some(storage) = storage {
            storage.log_alter_table_add_column(table_name, &new_column)?;
        }

        // Now get mutable table after all validations
        let table = db.get_table_mut(table_name).unwrap(); // Safe: we validated existence above

        // Add column to schema
        table.columns.push(new_column);

        // Add NULL value to all existing rows
        use crate::types::Value;
        for row in &mut table.rows {
            row.values.push(Value::Null);
        }

        Ok(QueryResult::Success(format!(
            "Column '{}' added to table '{}'",
            column_def.name, table_name
        )))
    }

    /// ALTER TABLE DROP COLUMN
    fn alter_table_drop_column(
        db: &mut Database,
        table_name: &str,
        column_name: String,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db.get_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        // Find column index
        let col_idx = table.columns.iter().position(|c| c.name == column_name)
            .ok_or_else(|| DatabaseError::ParseError(format!(
                "Column '{}' not found in table '{}'",
                column_name, table_name
            )))?;

        // Prevent dropping PRIMARY KEY columns
        if table.columns[col_idx].primary_key {
            return Err(DatabaseError::ParseError(
                "Cannot drop PRIMARY KEY column".to_string()
            ));
        }

        // Log to WAL
        if let Some(storage) = storage {
            storage.log_alter_table_drop_column(table_name, &column_name)?;
        }

        // Remove column from schema
        table.columns.remove(col_idx);

        // Remove value from all rows
        for row in &mut table.rows {
            row.values.remove(col_idx);
        }

        Ok(QueryResult::Success(format!(
            "Column '{}' dropped from table '{}'",
            column_name, table_name
        )))
    }

    /// ALTER TABLE RENAME COLUMN
    fn alter_table_rename_column(
        db: &mut Database,
        table_name: &str,
        old_name: String,
        new_name: String,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        let table = db.get_table_mut(table_name)
            .ok_or_else(|| DatabaseError::TableNotFound(table_name.to_string()))?;

        // Check if old column exists
        let col_idx = table.columns.iter().position(|c| c.name == old_name)
            .ok_or_else(|| DatabaseError::ParseError(format!(
                "Column '{}' not found", old_name
            )))?;

        // Check if new name already exists
        if table.columns.iter().any(|c| c.name == new_name) {
            return Err(DatabaseError::ParseError(format!(
                "Column '{}' already exists", new_name
            )));
        }

        // Log to WAL
        if let Some(storage) = storage {
            storage.log_alter_table_rename_column(table_name, &old_name, &new_name)?;
        }

        // Rename column
        table.columns[col_idx].name = new_name.clone();

        Ok(QueryResult::Success(format!(
            "Column '{}' renamed to '{}'",
            old_name, new_name
        )))
    }

    /// ALTER TABLE RENAME TO
    fn alter_table_rename_table(
        db: &mut Database,
        old_name: &str,
        new_name: String,
        storage: Option<&mut StorageEngine>,
    ) -> Result<QueryResult, DatabaseError> {
        // Check if new name is available
        if db.tables.contains_key(&new_name) {
            return Err(DatabaseError::TableAlreadyExists(new_name));
        }

        // Check if old table exists
        let mut table = db.tables.remove(old_name)
            .ok_or_else(|| DatabaseError::TableNotFound(old_name.to_string()))?;

        // Log to WAL
        if let Some(storage) = storage {
            storage.log_alter_table_rename(old_name, &new_name)?;
        }

        // Rename table
        table.name = new_name.clone();
        db.tables.insert(new_name.clone(), table);

        Ok(QueryResult::Success(format!(
            "Table '{}' renamed to '{}'",
            old_name, new_name
        )))
    }

    /// Execute SHOW TABLES statement
    pub fn show_tables(db: &Database) -> Result<QueryResult, DatabaseError> {
        let table_names: Vec<Vec<String>> = db
            .tables
            .keys()
            .map(|name| vec![name.clone()])
            .collect();

        if table_names.is_empty() {
            return Ok(QueryResult::Success("No tables found".to_string()));
        }

        Ok(QueryResult::Rows(
            table_names,
            vec!["Tables".to_string()],
        ))
    }
}
