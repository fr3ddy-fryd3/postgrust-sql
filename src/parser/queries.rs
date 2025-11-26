use super::common::{ws, identifier, value};
use super::statement::{
    Statement, Condition, SelectColumn, AggregateFunction, CountTarget,
    JoinClause, JoinType, SortOrder,
};
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while1},
    character::complete::char,
    combinator::{map, opt, recognize},
    multi::separated_list1,
    sequence::{delimited, preceded, tuple},
    IResult,
};

// Parse a simple condition (column = value, etc.)
fn condition_term(input: &str) -> IResult<&str, Condition> {
    let (input, column) = ws(identifier)(input)?;
    let (input, op) = ws(alt((tag("="), tag("!="), tag(">"), tag("<"))))(input)?;
    let (input, val) = ws(value)(input)?;

    let cond = match op {
        "=" => Condition::Equals(column, val),
        "!=" => Condition::NotEquals(column, val),
        ">" => Condition::GreaterThan(column, val),
        "<" => Condition::LessThan(column, val),
        _ => unreachable!(),
    };

    Ok((input, cond))
}

// Parse AND conditions (higher priority than OR)
fn condition_and(input: &str) -> IResult<&str, Condition> {
    let (input, first) = condition_term(input)?;
    let (input, rest) = opt(preceded(ws(tag_no_case("AND")), condition_and))(input)?;

    match rest {
        Some(right) => Ok((input, Condition::And(Box::new(first), Box::new(right)))),
        None => Ok((input, first)),
    }
}

// Parse OR conditions (lower priority than AND)
pub fn condition(input: &str) -> IResult<&str, Condition> {
    let (input, first) = condition_and(input)?;
    let (input, rest) = opt(preceded(ws(tag_no_case("OR")), condition))(input)?;

    match rest {
        Some(right) => Ok((input, Condition::Or(Box::new(first), Box::new(right)))),
        None => Ok((input, first)),
    }
}

// Parse aggregate functions: COUNT(*), COUNT(col), SUM(col), AVG(col), MIN(col), MAX(col)
fn aggregate_function(input: &str) -> IResult<&str, AggregateFunction> {
    alt((
        // COUNT(*) or COUNT(column)
        map(
            tuple((
                ws(tag_no_case("COUNT")),
                delimited(
                    char('('),
                    alt((
                        map(ws(char('*')), |_| CountTarget::All),
                        map(ws(identifier), CountTarget::Column),
                    )),
                    char(')'),
                ),
            )),
            |(_, target)| AggregateFunction::Count(target),
        ),
        // SUM(column)
        map(
            tuple((
                ws(tag_no_case("SUM")),
                delimited(char('('), ws(identifier), char(')')),
            )),
            |(_, col)| AggregateFunction::Sum(col),
        ),
        // AVG(column)
        map(
            tuple((
                ws(tag_no_case("AVG")),
                delimited(char('('), ws(identifier), char(')')),
            )),
            |(_, col)| AggregateFunction::Avg(col),
        ),
        // MIN(column)
        map(
            tuple((
                ws(tag_no_case("MIN")),
                delimited(char('('), ws(identifier), char(')')),
            )),
            |(_, col)| AggregateFunction::Min(col),
        ),
        // MAX(column)
        map(
            tuple((
                ws(tag_no_case("MAX")),
                delimited(char('('), ws(identifier), char(')')),
            )),
            |(_, col)| AggregateFunction::Max(col),
        ),
    ))(input)
}

// Parse select column: either regular column/*, or aggregate function
fn select_column(input: &str) -> IResult<&str, SelectColumn> {
    alt((
        map(aggregate_function, SelectColumn::Aggregate),
        map(
            alt((map(ws(char('*')), |_| "*".to_string()), identifier)),
            SelectColumn::Regular,
        ),
    ))(input)
}

// Parse JOIN clause: [INNER|LEFT|RIGHT] JOIN table ON left.col = right.col
pub fn join_clause(input: &str) -> IResult<&str, JoinClause> {
    let (input, join_type) = alt((
        map(ws(tag_no_case("INNER JOIN")), |_| JoinType::Inner),
        map(ws(tag_no_case("LEFT JOIN")), |_| JoinType::Left),
        map(ws(tag_no_case("RIGHT JOIN")), |_| JoinType::Right),
        map(ws(tag_no_case("JOIN")), |_| JoinType::Inner), // Default to INNER
    ))(input)?;

    let (input, table) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("ON"))(input)?;

    // Parse left_table.column
    let (input, on_left) = recognize(tuple((
        ws(identifier),
        ws(char('.')),
        ws(identifier),
    )))(input)?;

    let (input, _) = ws(char('='))(input)?;

    // Parse right_table.column
    let (input, on_right) = recognize(tuple((
        ws(identifier),
        ws(char('.')),
        ws(identifier),
    )))(input)?;

    Ok((
        input,
        JoinClause {
            join_type,
            table,
            on_left: on_left.trim().to_string(),
            on_right: on_right.trim().to_string(),
        },
    ))
}

// Parse optional WHERE clause
pub fn where_clause(input: &str) -> IResult<&str, Option<Condition>> {
    opt(preceded(ws(tag_no_case("WHERE")), condition))(input)
}

// Parse optional ORDER BY clause
pub fn order_by(input: &str) -> IResult<&str, Option<(String, SortOrder)>> {
    let result = opt(preceded(
        ws(tag_no_case("ORDER BY")),
        tuple((
            ws(identifier),
            opt(alt((
                map(ws(tag_no_case("ASC")), |_| SortOrder::Asc),
                map(ws(tag_no_case("DESC")), |_| SortOrder::Desc),
            ))),
        )),
    ))(input)?;

    Ok((result.0, result.1.map(|(col, sort)| (col, sort.unwrap_or(SortOrder::Asc)))))
}

// Parse optional GROUP BY clause
pub fn group_by(input: &str) -> IResult<&str, Option<Vec<String>>> {
    opt(preceded(
        ws(tag_no_case("GROUP BY")),
        separated_list1(ws(char(',')), ws(identifier)),
    ))(input)
}

// Parse optional LIMIT clause
pub fn limit(input: &str) -> IResult<&str, Option<usize>> {
    opt(preceded(
        ws(tag_no_case("LIMIT")),
        map(
            take_while1(|c: char| c.is_numeric()),
            |s: &str| s.parse::<usize>().unwrap(),
        ),
    ))(input)
}

pub fn select(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("SELECT"))(input)?;
    let (input, columns) = separated_list1(ws(char(',')), select_column)(input)?;
    let (input, _) = ws(tag_no_case("FROM"))(input)?;
    let (input, from) = ws(identifier)(input)?;

    // Parse optional JOIN clauses
    let (input, joins) = nom::multi::many0(join_clause)(input)?;

    let (input, filter) = where_clause(input)?;

    // Parse optional GROUP BY clause
    let (input, group_by) = group_by(input)?;

    // Parse optional ORDER BY clause
    let (input, order_by) = order_by(input)?;

    // Parse optional LIMIT clause
    let (input, limit) = limit(input)?;

    Ok((
        input,
        Statement::Select {
            columns,
            from,
            joins,
            filter,
            group_by,
            order_by,
            limit,
        },
    ))
}
