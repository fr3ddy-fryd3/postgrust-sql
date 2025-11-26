use super::common::ws;
use super::statement::Statement;
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
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
