use crate::types::DataType;
use super::common::{ws, identifier, data_type, string_literal};
use super::statement::{Statement, ColumnDef, PrivilegeType};
use nom::{
    branch::alt,
    bytes::complete::{tag_no_case, take_while1},
    character::complete::char,
    combinator::{map, opt},
    multi::separated_list1,
    sequence::{delimited, preceded, tuple},
    IResult,
};

fn column_def(input: &str) -> IResult<&str, ColumnDef> {
    let (input, name) = ws(identifier)(input)?;
    let (input, data_type) = ws(data_type)(input)?;
    let (input, primary_key) = opt(ws(tag_no_case("PRIMARY KEY")))(input)?;
    let (input, unique_kw) = opt(ws(tag_no_case("UNIQUE")))(input)?;
    let (input, not_null) = opt(ws(tag_no_case("NOT NULL")))(input)?;

    // Parse REFERENCES table(column) for foreign key
    let (input, foreign_key) = opt(tuple((
        ws(tag_no_case("REFERENCES")),
        ws(identifier),
        delimited(ws(char('(')), ws(identifier), ws(char(')'))),
    )))(input)?;

    let foreign_key = foreign_key.map(|(_, table, column)| crate::types::ForeignKey {
        referenced_table: table,
        referenced_column: column,
    });

    // SERIAL and BIGSERIAL columns are automatically NOT NULL and PRIMARY KEY
    let is_serial = matches!(data_type, DataType::Serial | DataType::BigSerial);
    let nullable = if is_serial {
        false
    } else {
        not_null.is_none() && primary_key.is_none()
    };
    let primary_key = is_serial || primary_key.is_some();
    let unique = unique_kw.is_some();

    Ok((
        input,
        ColumnDef {
            name,
            data_type,
            nullable,
            primary_key,
            unique,
            foreign_key,
        },
    ))
}

pub fn create_table(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE TABLE"))(input)?;
    let (input, name) = ws(identifier)(input)?;
    let (input, columns) = delimited(
        ws(char('(')),
        separated_list1(ws(char(',')), column_def),
        ws(char(')')),
    )(input)?;

    Ok((input, Statement::CreateTable { name, columns, owner: None }))
}

pub fn drop_table(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP TABLE"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    Ok((input, Statement::DropTable { name }))
}

pub fn create_database(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE DATABASE"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    // Support both "WITH OWNER" (PostgreSQL) and "OWNER" (backwards compat)
    let (input, owner) = opt(alt((
        preceded(ws(tag_no_case("WITH OWNER")), ws(identifier)),
        preceded(ws(tag_no_case("OWNER")), ws(identifier)),
    )))(input)?;

    Ok((input, Statement::CreateDatabase {
        name,
        owner,
    }))
}

pub fn drop_database(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP DATABASE"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    Ok((input, Statement::DropDatabase {
        name,
    }))
}

pub fn create_user(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE USER"))(input)?;
    let (input, username) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("WITH PASSWORD"))(input)?;
    let (input, password) = ws(string_literal)(input)?;
    let (input, is_superuser) = opt(ws(tag_no_case("SUPERUSER")))(input)?;

    Ok((input, Statement::CreateUser {
        username,
        password,
        is_superuser: is_superuser.is_some(),
    }))
}

pub fn drop_user(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP USER"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::DropUser {
        username,
    }))
}

pub fn alter_user(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("ALTER USER"))(input)?;
    let (input, username) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("WITH PASSWORD"))(input)?;
    let (input, password) = ws(string_literal)(input)?;

    Ok((input, Statement::AlterUser {
        username,
        password,
    }))
}

pub fn create_role(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE ROLE"))(input)?;
    let (input, role_name) = ws(identifier)(input)?;
    let (input, is_superuser) = opt(ws(tag_no_case("SUPERUSER")))(input)?;

    Ok((input, Statement::CreateRole {
        role_name,
        is_superuser: is_superuser.is_some(),
    }))
}

pub fn drop_role(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP ROLE"))(input)?;
    let (input, role_name) = ws(identifier)(input)?;

    Ok((input, Statement::DropRole {
        role_name,
    }))
}

pub fn grant_role(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("GRANT"))(input)?;
    let (input, role_name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("TO"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::GrantRole {
        role_name,
        to_user: username,
    }))
}

pub fn revoke_role(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("REVOKE"))(input)?;
    let (input, role_name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("FROM"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::RevokeRole {
        role_name,
        from_user: username,
    }))
}

pub fn privilege_type(input: &str) -> IResult<&str, PrivilegeType> {
    alt((
        map(tag_no_case("CONNECT"), |_| PrivilegeType::Connect),
        map(tag_no_case("CREATE"), |_| PrivilegeType::Create),
        map(tag_no_case("SELECT"), |_| PrivilegeType::Select),
        map(tag_no_case("INSERT"), |_| PrivilegeType::Insert),
        map(tag_no_case("UPDATE"), |_| PrivilegeType::Update),
        map(tag_no_case("DELETE"), |_| PrivilegeType::Delete),
        map(tag_no_case("ALL"), |_| PrivilegeType::All),
    ))(input)
}

pub fn grant(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("GRANT"))(input)?;
    let (input, privilege) = ws(privilege_type)(input)?;
    let (input, _) = ws(tag_no_case("ON DATABASE"))(input)?;
    let (input, db_name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("TO"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::Grant {
        privilege,
        on_database: db_name,
        to_user: username,
    }))
}

pub fn revoke(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("REVOKE"))(input)?;
    let (input, privilege) = ws(privilege_type)(input)?;
    let (input, _) = ws(tag_no_case("ON DATABASE"))(input)?;
    let (input, db_name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("FROM"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::Revoke {
        privilege,
        on_database: db_name,
        from_user: username,
    }))
}

pub fn create_type(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE TYPE"))(input)?;
    let (input, name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("AS ENUM"))(input)?;
    let (input, _) = ws(char('('))(input)?;
    let (input, values) = separated_list1(
        ws(char(',')),
        map(
            delimited(char('\''), take_while1(|c| c != '\''), char('\'')),
            |s: &str| s.to_string()
        )
    )(input)?;
    let (input, _) = ws(char(')'))(input)?;

    Ok((input, Statement::CreateType {
        name,
        values,
    }))
}

pub fn alter_table(input: &str) -> IResult<&str, Statement> {
    use super::statement::AlterTableOperation;
    
    let (input, _) = ws(tag_no_case("ALTER TABLE"))(input)?;
    let (input, table_name) = ws(identifier)(input)?;
    
    // Try different ALTER TABLE operations
    let (input, operation) = alt((
        // ADD COLUMN
        map(
            preceded(
                ws(tag_no_case("ADD COLUMN")),
                column_def
            ),
            AlterTableOperation::AddColumn
        ),
        // DROP COLUMN
        map(
            preceded(
                ws(tag_no_case("DROP COLUMN")),
                ws(identifier)
            ),
            AlterTableOperation::DropColumn
        ),
        // RENAME COLUMN
        map(
            tuple((
                preceded(ws(tag_no_case("RENAME COLUMN")), ws(identifier)),
                preceded(ws(tag_no_case("TO")), ws(identifier)),
            )),
            |(old_name, new_name)| AlterTableOperation::RenameColumn { old_name, new_name }
        ),
        // RENAME TO (rename table)
        map(
            preceded(
                ws(tag_no_case("RENAME TO")),
                ws(identifier)
            ),
            AlterTableOperation::RenameTable
        ),
        // OWNER TO (change table owner) - v2.3.0
        map(
            preceded(
                ws(tag_no_case("OWNER TO")),
                ws(identifier)
            ),
            AlterTableOperation::OwnerTo
        ),
    ))(input)?;
    
    Ok((input, Statement::AlterTable {
        name: table_name,
        operation,
    }))
}

/// Parse CREATE INDEX statement
///
/// Syntax:
/// - CREATE INDEX `idx_name` ON table(column);
/// - CREATE UNIQUE INDEX `idx_name` ON table(column);
/// - CREATE INDEX `idx_name` ON table(column) USING HASH;
/// - CREATE INDEX `idx_name` ON table(column) USING BTREE;
pub fn parse_create_index(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE"))(input)?;

    // Check for UNIQUE keyword
    let (input, unique) = opt(ws(tag_no_case("UNIQUE")))(input)?;
    let unique = unique.is_some();

    let (input, _) = ws(tag_no_case("INDEX"))(input)?;
    let (input, name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("ON"))(input)?;
    let (input, table) = ws(identifier)(input)?;

    // Column(s) in parentheses - v1.9.0: supports comma-separated list
    let (input, columns) = delimited(
        ws(char('(')),
        separated_list1(ws(char(',')), ws(identifier)),
        ws(char(')'))
    )(input)?;

    // Optional USING clause
    let (input, index_type) = opt(|i| {
        let (i, _) = ws(tag_no_case("USING"))(i)?;
        let (i, type_name) = ws(identifier)(i)?;
        Ok((i, type_name))
    })(input)?;

    let index_type = match index_type.as_deref() {
        Some("hash" | "HASH") => crate::index::IndexType::Hash,
        Some("btree" | "BTREE") => crate::index::IndexType::BTree,
        None => crate::index::IndexType::BTree, // default
        _ => crate::index::IndexType::BTree, // invalid type defaults to btree
    };

    Ok((input, Statement::CreateIndex {
        name,
        table,
        columns,
        unique,
        index_type,
    }))
}

/// Parse DROP INDEX statement
///
/// Syntax:
/// - DROP INDEX `idx_name`;
pub fn parse_drop_index(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP INDEX"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    Ok((input, Statement::DropIndex { name }))
}

/// Parse VACUUM statement
///
/// Syntax:
/// - VACUUM;              -- vacuum all tables
/// - VACUUM `table_name`;   -- vacuum specific table
pub fn parse_vacuum(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("VACUUM"))(input)?;

    // Optional table name
    let (input, table) = opt(ws(identifier))(input)?;

    Ok((input, Statement::Vacuum { table }))
}

/// Parse CREATE VIEW statement (v1.10.0)
///
/// Syntax: CREATE VIEW name AS SELECT ...
pub fn parse_create_view(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE"))(input)?;
    let (input, _) = ws(tag_no_case("VIEW"))(input)?;
    let (input, name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("AS"))(input)?;

    // Capture the rest as query string (до конца или точки с запятой)
    let (input, query) = nom::bytes::complete::take_while(|c: char| c != ';')(input)?;

    Ok((input, Statement::CreateView {
        name,
        query: query.trim().to_string(),
    }))
}

/// Parse DROP VIEW statement (v1.10.0)
///
/// Syntax: DROP VIEW name
pub fn parse_drop_view(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP"))(input)?;
    let (input, _) = ws(tag_no_case("VIEW"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    Ok((input, Statement::DropView { name }))
}
