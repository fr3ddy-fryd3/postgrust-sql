use super::common::ws;
use super::statement::Statement;
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    combinator::rest,
    IResult,
};

pub fn show_tables(input: &str) -> IResult<&str, Statement> {
    // Support both "SHOW TABLES" (MySQL-style) and "\dt" or "\d" (psql-style)
    let (input, _) = ws(alt((
        tag_no_case("SHOW TABLES"),
        tag("\\dt"),
        tag("\\d"),
    )))(input)?;
    Ok((input, Statement::ShowTables))
}

pub fn show_users(input: &str) -> IResult<&str, Statement> {
    // Support both "SHOW USERS" and "\du" (psql-style)
    let (input, _) = ws(alt((
        tag_no_case("SHOW USERS"),
        tag("\\du"),
    )))(input)?;
    Ok((input, Statement::ShowUsers))
}

pub fn show_databases(input: &str) -> IResult<&str, Statement> {
    // Support both "SHOW DATABASES" and "\l" (psql-style)
    let (input, _) = ws(alt((
        tag_no_case("SHOW DATABASES"),
        tag("\\l"),
    )))(input)?;
    Ok((input, Statement::ShowDatabases))
}

// EXPLAIN command (v1.8.0)
pub fn explain(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("EXPLAIN"))(input)?;
    let (input, query_str) = rest(input)?;

    // Parse the inner statement
    match crate::parser::parse_statement(query_str.trim()) {
        Ok(inner_stmt) => {
            // Only allow EXPLAIN for SELECT statements
            if matches!(inner_stmt, Statement::Select { .. }) {
                Ok((input, Statement::Explain {
                    statement: Box::new(inner_stmt),
                }))
            } else {
                // For now, only support EXPLAIN SELECT
                Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Tag,
                )))
            }
        }
        Err(_) => Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        ))),
    }
}
