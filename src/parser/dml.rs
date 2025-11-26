use super::common::{ws, identifier, value};
use super::statement::Statement;
use super::queries::condition;
use nom::{
    bytes::complete::tag_no_case,
    character::complete::char,
    combinator::opt,
    multi::separated_list1,
    sequence::{delimited, preceded, tuple},
    IResult,
};

pub fn insert(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("INSERT INTO"))(input)?;
    let (input, table) = ws(identifier)(input)?;
    let (input, columns) = opt(delimited(
        ws(char('(')),
        separated_list1(ws(char(',')), identifier),
        ws(char(')')),
    ))(input)?;
    let (input, _) = ws(tag_no_case("VALUES"))(input)?;
    let (input, values) = delimited(
        ws(char('(')),
        separated_list1(ws(char(',')), value),
        ws(char(')')),
    )(input)?;

    Ok((
        input,
        Statement::Insert {
            table,
            columns,
            values,
        },
    ))
}

pub fn update(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("UPDATE"))(input)?;
    let (input, table) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("SET"))(input)?;
    let (input, assignments) = separated_list1(
        ws(char(',')),
        tuple((ws(identifier), ws(char('=')), ws(value))),
    )(input)?;
    let assignments = assignments
        .into_iter()
        .map(|(col, _, val)| (col, val))
        .collect();
    let (input, filter) = opt(preceded(ws(tag_no_case("WHERE")), condition))(input)?;

    Ok((
        input,
        Statement::Update {
            table,
            assignments,
            filter,
        },
    ))
}

pub fn delete(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DELETE FROM"))(input)?;
    let (input, from) = ws(identifier)(input)?;
    let (input, filter) = opt(preceded(ws(tag_no_case("WHERE")), condition))(input)?;

    Ok((input, Statement::Delete { from, filter }))
}
