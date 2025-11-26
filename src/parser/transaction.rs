use super::common::ws;
use super::statement::Statement;
use nom::{
    branch::alt,
    bytes::complete::tag_no_case,
    IResult,
};

pub fn begin_transaction(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(alt((
        tag_no_case("BEGIN"),
        tag_no_case("BEGIN TRANSACTION"),
        tag_no_case("START TRANSACTION"),
    )))(input)?;
    Ok((input, Statement::Begin))
}

pub fn commit_transaction(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(alt((
        tag_no_case("COMMIT"),
        tag_no_case("COMMIT TRANSACTION"),
    )))(input)?;
    Ok((input, Statement::Commit))
}

pub fn rollback_transaction(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(alt((
        tag_no_case("ROLLBACK"),
        tag_no_case("ROLLBACK TRANSACTION"),
    )))(input)?;
    Ok((input, Statement::Rollback))
}
