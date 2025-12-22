// Module declarations
mod statement;
mod common;
mod ddl;
mod dml;
mod queries;
mod meta;
mod transaction;

// Re-export all public types for backward compatibility
pub use statement::{
    Statement,
    ColumnDef,
    AlterTableOperation,
    Condition,
    SortOrder,
    SelectColumn,
    AggregateFunction,
    CountTarget,
    JoinType,
    JoinClause,
    PrivilegeType,
    GrantObject,     // v2.3.0
    CaseExpression,  // v1.10.0
    WhenClause,      // v1.10.0
};

// Main parser function that combines all parsers
use nom::branch::alt;

pub fn parse_statement(input: &str) -> Result<Statement, String> {
    let input = input.trim();
    let input = input.trim_end_matches(';');

    // Split into three alt blocks due to nom's 21-element tuple limit
    let result = alt((
        alt((
            meta::explain,  // v1.8.0 - must come before show_* to avoid conflicts
            meta::show_users,
            meta::show_databases,
            meta::show_tables,
            transaction::begin_transaction,
            transaction::commit_transaction,
            transaction::rollback_transaction,
            ddl::create_type,
            ddl::create_user,
            ddl::drop_user,
            ddl::alter_user,
            ddl::create_role,  // v2.3.0
            ddl::drop_role,    // v2.3.0
            ddl::grant_role,   // v2.3.0 - must come before grant (database privilege)
            ddl::revoke_role,  // v2.3.0 - must come before revoke (database privilege)
            ddl::create_database,
        )),
        alt((
            ddl::drop_database,
            ddl::grant,
            ddl::revoke,
            ddl::parse_create_view,  // v1.10.0 - before create_table to avoid conflicts
            ddl::parse_drop_view,    // v1.10.0
            ddl::create_table,
            ddl::drop_table,
            ddl::alter_table,
            ddl::parse_create_index,
            ddl::parse_drop_index,
            ddl::parse_vacuum,
            dml::insert,
            queries::select,
            dml::update,
            dml::delete,
        )),
    ))(input);

    match result {
        Ok((remaining, stmt)) => {
            if remaining.trim().is_empty() {
                Ok(stmt)
            } else {
                Err(format!("Unexpected input after statement: {remaining}"))
            }
        }
        Err(e) => Err(format!("Parse error: {e:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_create_table() {
        let sql = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER)";
        let stmt = parse_statement(sql).unwrap();
        assert!(matches!(stmt, Statement::CreateTable { .. }));
    }

    #[test]
    fn test_parse_insert() {
        let sql = "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30)";
        let stmt = parse_statement(sql).unwrap();
        assert!(matches!(stmt, Statement::Insert { .. }));
    }

    #[test]
    fn test_parse_select() {
        let sql = "SELECT * FROM users WHERE id = 1";
        let stmt = parse_statement(sql).unwrap();
        assert!(matches!(stmt, Statement::Select { .. }));
    }

    #[test]
    fn test_parse_select_with_and() {
        let sql = "SELECT * FROM users WHERE age > 25 AND age < 35";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { filter: Some(Condition::And(_, _)), .. } => (),
            _ => panic!("Expected AND condition"),
        }
    }

    #[test]
    fn test_parse_select_with_or() {
        let sql = "SELECT * FROM users WHERE name = 'Alice' OR name = 'Bob'";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { filter: Some(Condition::Or(_, _)), .. } => (),
            _ => panic!("Expected OR condition"),
        }
    }

    #[test]
    fn test_parse_select_with_order_by_asc() {
        let sql = "SELECT * FROM users ORDER BY age ASC";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { order_by: Some((col, SortOrder::Asc)), .. } => {
                assert_eq!(col, "age");
            }
            _ => panic!("Expected ORDER BY ASC"),
        }
    }

    #[test]
    fn test_parse_select_with_order_by_desc() {
        let sql = "SELECT * FROM users ORDER BY age DESC";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { order_by: Some((col, SortOrder::Desc)), .. } => {
                assert_eq!(col, "age");
            }
            _ => panic!("Expected ORDER BY DESC"),
        }
    }

    #[test]
    fn test_parse_select_with_limit() {
        let sql = "SELECT * FROM users LIMIT 10";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { limit: Some(10), .. } => (),
            _ => panic!("Expected LIMIT 10"),
        }
    }

    #[test]
    fn test_parse_select_with_order_by_and_limit() {
        let sql = "SELECT * FROM users ORDER BY age DESC LIMIT 5";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select {
                order_by: Some((col, SortOrder::Desc)),
                limit: Some(5),
                ..
            } => {
                assert_eq!(col, "age");
            }
            _ => panic!("Expected ORDER BY DESC LIMIT 5"),
        }
    }

    #[test]
    fn test_parse_select_complex() {
        let sql = "SELECT name, age FROM users WHERE age > 25 AND age < 35 ORDER BY age ASC LIMIT 10";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select {
                columns,
                filter: Some(Condition::And(_, _)),
                order_by: Some((col, SortOrder::Asc)),
                limit: Some(10),
                ..
            } => {
                assert_eq!(columns.len(), 2);
                assert_eq!(col, "age");
            }
            _ => panic!("Expected complex SELECT"),
        }
    }

    #[test]
    fn test_parse_create_index() {
        let sql = "CREATE INDEX idx_age ON users(age)";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::CreateIndex { name, table, columns, unique, index_type } => {
                assert_eq!(name, "idx_age");
                assert_eq!(table, "users");
                assert_eq!(columns, vec!["age".to_string()]);
                assert_eq!(unique, false);
                assert_eq!(index_type, crate::index::IndexType::BTree); // default
            }
            _ => panic!("Expected CREATE INDEX"),
        }
    }

    #[test]
    fn test_parse_create_unique_index() {
        let sql = "CREATE UNIQUE INDEX idx_name ON users(name)";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::CreateIndex { name, table, columns, unique, index_type } => {
                assert_eq!(name, "idx_name");
                assert_eq!(table, "users");
                assert_eq!(columns, vec!["name".to_string()]);
                assert_eq!(unique, true);
                assert_eq!(index_type, crate::index::IndexType::BTree); // default
            }
            _ => panic!("Expected CREATE UNIQUE INDEX"),
        }
    }

    #[test]
    fn test_parse_create_hash_index() {
        let sql = "CREATE INDEX idx_category ON products(category) USING HASH";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::CreateIndex { name, table, columns, unique, index_type } => {
                assert_eq!(name, "idx_category");
                assert_eq!(table, "products");
                assert_eq!(columns, vec!["category".to_string()]);
                assert_eq!(unique, false);
                assert_eq!(index_type, crate::index::IndexType::Hash);
            }
            _ => panic!("Expected CREATE INDEX USING HASH"),
        }
    }

    #[test]
    fn test_parse_create_btree_index() {
        let sql = "CREATE INDEX idx_price ON products(price) USING BTREE";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::CreateIndex { name, table, columns, unique, index_type } => {
                assert_eq!(name, "idx_price");
                assert_eq!(table, "products");
                assert_eq!(columns, vec!["price".to_string()]);
                assert_eq!(unique, false);
                assert_eq!(index_type, crate::index::IndexType::BTree);
            }
            _ => panic!("Expected CREATE INDEX USING BTREE"),
        }
    }

    #[test]
    fn test_parse_drop_index() {
        let sql = "DROP INDEX idx_age";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::DropIndex { name } => {
                assert_eq!(name, "idx_age");
            }
            _ => panic!("Expected DROP INDEX"),
        }
    }

    #[test]
    fn test_parse_case_simple() {
        let sql = "SELECT name, CASE WHEN age < 18 THEN 'minor' ELSE 'adult' END FROM users";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { columns, .. } => {
                assert_eq!(columns.len(), 2);
                match &columns[1] {
                    SelectColumn::Case(case_expr) => {
                        assert_eq!(case_expr.when_clauses.len(), 1);
                        assert!(case_expr.else_value.is_some());
                    }
                    _ => panic!("Expected CASE expression"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_case_multiple_when() {
        let sql = "SELECT CASE WHEN age < 18 THEN 'minor' WHEN age < 65 THEN 'adult' ELSE 'senior' END AS category FROM users";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { columns, .. } => {
                assert_eq!(columns.len(), 1);
                match &columns[0] {
                    SelectColumn::Case(case_expr) => {
                        assert_eq!(case_expr.when_clauses.len(), 2);
                        assert!(case_expr.else_value.is_some());
                        assert_eq!(case_expr.alias, Some("category".to_string()));
                    }
                    _ => panic!("Expected CASE expression"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }

    #[test]
    fn test_parse_case_no_else() {
        let sql = "SELECT CASE WHEN status = 'active' THEN 'Y' END FROM users";
        let stmt = parse_statement(sql).unwrap();
        match stmt {
            Statement::Select { columns, .. } => {
                match &columns[0] {
                    SelectColumn::Case(case_expr) => {
                        assert_eq!(case_expr.when_clauses.len(), 1);
                        assert!(case_expr.else_value.is_none());
                    }
                    _ => panic!("Expected CASE expression"),
                }
            }
            _ => panic!("Expected SELECT"),
        }
    }
}
