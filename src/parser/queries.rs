use super::common::{ws, identifier, non_keyword_identifier, value};
use super::statement::{
    Statement, Condition, SelectColumn, AggregateFunction, CountTarget,
    JoinClause, JoinType, SortOrder, CaseExpression, WhenClause,
    WindowFunction, WindowSpec,
};
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while1},
    character::complete::{char, digit1},
    combinator::{map, opt, recognize},
    multi::separated_list1,
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

// Parse a subquery: (SELECT ...)  (v2.6.0)
// Using a closure to enable recursive parsing
fn subquery(input: &str) -> IResult<&str, Box<Statement>> {
    delimited(
        ws(char('(')),
        |i| {
            let (i, stmt) = select(i)?;
            Ok((i, Box::new(stmt)))
        },
        ws(char(')')),
    )(input)
}

// Parse a simple condition (column = value, etc.)
fn condition_term(input: &str) -> IResult<&str, Condition> {
    alt((
        // EXISTS (SELECT ...) (v2.6.0)
        map(
            preceded(ws(tag_no_case("EXISTS")), subquery),
            Condition::Exists,
        ),
        // NOT EXISTS (SELECT ...) (v2.6.0)
        map(
            preceded(
                tuple((ws(tag_no_case("NOT")), ws(tag_no_case("EXISTS")))),
                subquery,
            ),
            |stmt| Condition::NotExists(stmt),
        ),
        // col IN (SELECT ...) or col NOT IN (SELECT ...) (v2.6.0)
        map(
            tuple((
                ws(non_keyword_identifier),
                opt(ws(tag_no_case("NOT"))),
                ws(tag_no_case("IN")),
                subquery,
            )),
            |(col, not, _, stmt)| {
                if not.is_some() {
                    Condition::NotInSubquery(col, stmt)
                } else {
                    Condition::InSubquery(col, stmt)
                }
            },
        ),
        // col = (SELECT ...) (v2.6.0)
        map(
            tuple((ws(non_keyword_identifier), ws(char('=')), subquery)),
            |(col, _, stmt)| Condition::EqualsSubquery(col, stmt),
        ),
        // col > (SELECT ...) (v2.6.0)
        map(
            tuple((ws(non_keyword_identifier), ws(char('>')), subquery)),
            |(col, _, stmt)| Condition::GreaterThanSubquery(col, stmt),
        ),
        // col < (SELECT ...) (v2.6.0)
        map(
            tuple((ws(non_keyword_identifier), ws(char('<')), subquery)),
            |(col, _, stmt)| Condition::LessThanSubquery(col, stmt),
        ),
        // IS NULL / IS NOT NULL (v1.8.0)
        map(
            tuple((
                ws(non_keyword_identifier),
                ws(tag_no_case("IS")),
                ws(tag_no_case("NOT")),
                ws(tag_no_case("NULL")),
            )),
            |(col, _, _, _)| Condition::IsNotNull(col),
        ),
        map(
            tuple((ws(non_keyword_identifier), ws(tag_no_case("IS")), ws(tag_no_case("NULL")))),
            |(col, _, _)| Condition::IsNull(col),
        ),
        // BETWEEN (v1.8.0)
        map(
            tuple((
                ws(non_keyword_identifier),
                ws(tag_no_case("BETWEEN")),
                ws(value),
                ws(tag_no_case("AND")),
                ws(value),
            )),
            |(col, _, low, _, high)| Condition::Between(col, low, high),
        ),
        // LIKE (v1.8.0)
        map(
            tuple((ws(non_keyword_identifier), ws(tag_no_case("LIKE")), ws(value))),
            |(col, _, val)| {
                if let crate::types::Value::Text(pattern) = val {
                    Condition::Like(col, pattern)
                } else {
                    // Fallback - should not happen with proper value parser
                    Condition::Like(col, String::new())
                }
            },
        ),
        // IN (v1.8.0)
        map(
            tuple((
                ws(non_keyword_identifier),
                ws(tag_no_case("IN")),
                delimited(
                    ws(char('(')),
                    separated_list1(ws(char(',')), ws(value)),
                    ws(char(')')),
                ),
            )),
            |(col, _, values)| Condition::In(col, values),
        ),
        // Comparison operators (including >=, <=)
        map(
            tuple((
                ws(non_keyword_identifier),
                ws(alt((
                    tag(">="),
                    tag("<="),
                    tag("!="),
                    tag("="),
                    tag(">"),
                    tag("<"),
                ))),
                ws(value),
            )),
            |(column, op, val)| match op {
                "=" => Condition::Equals(column, val),
                "!=" => Condition::NotEquals(column, val),
                ">" => Condition::GreaterThan(column, val),
                "<" => Condition::LessThan(column, val),
                ">=" => Condition::GreaterThanOrEqual(column, val), // v1.8.0
                "<=" => Condition::LessThanOrEqual(column, val),    // v1.8.0
                _ => unreachable!(),
            },
        ),
    ))(input)
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

// Parse window specification: OVER (PARTITION BY ... ORDER BY ...) (v2.6.0)
fn window_spec(input: &str) -> IResult<&str, WindowSpec> {
    let (input, _) = ws(tag_no_case("OVER"))(input)?;
    let (input, _) = ws(char('('))(input)?;

    // Parse optional PARTITION BY
    let (input, partition_by) = opt(preceded(
        ws(tag_no_case("PARTITION BY")),
        separated_list1(ws(char(',')), ws(identifier)),
    ))(input)?;

    // Parse optional ORDER BY
    let (input, order_by) = opt(preceded(
        ws(tag_no_case("ORDER BY")),
        separated_list1(
            ws(char(',')),
            tuple((
                ws(identifier),
                opt(alt((
                    map(ws(tag_no_case("ASC")), |_| SortOrder::Asc),
                    map(ws(tag_no_case("DESC")), |_| SortOrder::Desc),
                ))),
            )),
        ),
    ))(input)?;

    let (input, _) = ws(char(')'))(input)?;

    Ok((input, WindowSpec {
        partition_by: partition_by.unwrap_or_default(),
        order_by: order_by.unwrap_or_default()
            .into_iter()
            .map(|(col, order)| (col, order.unwrap_or(SortOrder::Asc)))
            .collect(),
    }))
}

// Parse window functions: ROW_NUMBER(), RANK(), DENSE_RANK(), LAG(), LEAD() (v2.6.0)
fn window_function(input: &str) -> IResult<&str, WindowFunction> {
    alt((
        map(
            preceded(ws(tag_no_case("ROW_NUMBER")), tuple((ws(char('(')), ws(char(')'))))),
            |_| WindowFunction::RowNumber,
        ),
        map(
            preceded(ws(tag_no_case("RANK")), tuple((ws(char('(')), ws(char(')'))))),
            |_| WindowFunction::Rank,
        ),
        map(
            preceded(ws(tag_no_case("DENSE_RANK")), tuple((ws(char('(')), ws(char(')'))))),
            |_| WindowFunction::DenseRank,
        ),
        // LAG(column) or LAG(column, offset)
        map(
            tuple((
                ws(tag_no_case("LAG")),
                delimited(
                    ws(char('(')),
                    tuple((
                        ws(identifier),
                        opt(preceded(ws(char(',')), ws(recognize(pair(opt(char('-')), digit1))))),
                    )),
                    ws(char(')')),
                ),
            )),
            |(_, (col, offset))| {
                let offset_val = offset.and_then(|s| s.parse::<i64>().ok());
                WindowFunction::Lag(col, offset_val)
            },
        ),
        // LEAD(column) or LEAD(column, offset)
        map(
            tuple((
                ws(tag_no_case("LEAD")),
                delimited(
                    ws(char('(')),
                    tuple((
                        ws(identifier),
                        opt(preceded(ws(char(',')), ws(recognize(pair(opt(char('-')), digit1))))),
                    )),
                    ws(char(')')),
                ),
            )),
            |(_, (col, offset))| {
                let offset_val = offset.and_then(|s| s.parse::<i64>().ok());
                WindowFunction::Lead(col, offset_val)
            },
        ),
    ))(input)
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

// Parse WHEN clause: WHEN condition THEN value
fn when_clause(input: &str) -> IResult<&str, WhenClause> {
    let (input, _) = ws(tag_no_case("WHEN"))(input)?;
    let (input, cond) = condition(input)?;
    let (input, _) = ws(tag_no_case("THEN"))(input)?;
    let (input, result) = ws(value)(input)?;

    Ok((input, WhenClause { condition: cond, result }))
}

// Parse CASE expression: CASE WHEN ... THEN ... [WHEN ... THEN ...] [ELSE ...] END [AS alias]
fn case_expression(input: &str) -> IResult<&str, CaseExpression> {
    let (input, _) = ws(tag_no_case("CASE"))(input)?;

    // Parse one or more WHEN clauses
    let (input, when_clauses) = nom::multi::many1(when_clause)(input)?;

    // Parse optional ELSE clause
    let (input, else_value) = opt(preceded(ws(tag_no_case("ELSE")), ws(value)))(input)?;

    let (input, _) = ws(tag_no_case("END"))(input)?;

    // Parse optional AS alias
    let (input, alias) = opt(preceded(ws(tag_no_case("AS")), ws(identifier)))(input)?;

    Ok((input, CaseExpression {
        when_clauses,
        else_value,
        alias,
    }))
}

// Parse select column: either regular column/*, aggregate function, CASE expression, or literal
fn select_column(input: &str) -> IResult<&str, SelectColumn> {
    alt((
        map(case_expression, SelectColumn::Case),
        map(aggregate_function, SelectColumn::Aggregate),
        // Window function: ROW_NUMBER() OVER (...), etc. (v2.6.0)
        map(
            tuple((
                window_function,
                window_spec,
                opt(preceded(ws(tag_no_case("AS")), ws(identifier))),
            )),
            |(function, spec, alias)| SelectColumn::Window {
                function,
                spec,
                alias,
            },
        ),
        // Scalar subquery: (SELECT ...) or (SELECT ...) AS alias (v2.6.0)
        map(
            tuple((
                subquery,
                opt(preceded(ws(tag_no_case("AS")), ws(identifier))),
            )),
            |(query, alias)| SelectColumn::Subquery {
                query,
                alias,
            },
        ),
        // Literal value: numbers, strings, booleans, NULL (v2.6.0)
        map(ws(value), SelectColumn::Literal),
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

pub fn offset(input: &str) -> IResult<&str, Option<usize>> {
    opt(preceded(
        ws(tag_no_case("OFFSET")),
        map(
            take_while1(|c: char| c.is_numeric()),
            |s: &str| s.parse::<usize>().unwrap(),
        ),
    ))(input)
}

// Parse base SELECT (without set operations)
fn select_base(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("SELECT"))(input)?;

    // Parse optional DISTINCT keyword
    let (input, distinct) = opt(ws(tag_no_case("DISTINCT")))(input)?;
    let distinct = distinct.is_some();

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

    // Parse optional OFFSET clause
    let (input, offset) = offset(input)?;

    Ok((
        input,
        Statement::Select {
            distinct,
            columns,
            from,
            joins,
            filter,
            group_by,
            order_by,
            limit,
            offset,
        },
    ))
}

// Parse SELECT with set operations (UNION/INTERSECT/EXCEPT) (v1.10.0)
pub fn select(input: &str) -> IResult<&str, Statement> {
    let (input, left) = select_base(input)?;

    // Check for set operations
    let (input, set_op) = opt(alt((
        map(
            tuple((
                ws(tag_no_case("UNION")),
                opt(ws(tag_no_case("ALL"))),
            )),
            |(_, all)| ("UNION", all.is_some()),
        ),
        map(ws(tag_no_case("INTERSECT")), |_| ("INTERSECT", false)),
        map(ws(tag_no_case("EXCEPT")), |_| ("EXCEPT", false)),
    )))(input)?;

    match set_op {
        Some(("UNION", all)) => {
            let (input, right) = select(input)?;
            Ok((
                input,
                Statement::Union {
                    left: Box::new(left),
                    right: Box::new(right),
                    all,
                },
            ))
        }
        Some(("INTERSECT", _)) => {
            let (input, right) = select(input)?;
            Ok((
                input,
                Statement::Intersect {
                    left: Box::new(left),
                    right: Box::new(right),
                },
            ))
        }
        Some(("EXCEPT", _)) => {
            let (input, right) = select(input)?;
            Ok((
                input,
                Statement::Except {
                    left: Box::new(left),
                    right: Box::new(right),
                },
            ))
        }
        _ => Ok((input, left)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_exists_subquery() {
        let sql = "EXISTS (SELECT * FROM users)";
        let result = condition_term(sql);
        assert!(result.is_ok());
        let (_, cond) = result.unwrap();
        assert!(matches!(cond, Condition::Exists(_)));
    }

    #[test]
    fn test_parse_not_exists_subquery() {
        let sql = "NOT EXISTS (SELECT * FROM orders)";
        let result = condition_term(sql);
        assert!(result.is_ok());
        let (_, cond) = result.unwrap();
        assert!(matches!(cond, Condition::NotExists(_)));
    }

    #[test]
    fn test_parse_in_subquery() {
        let sql = "user_id IN (SELECT id FROM users)";
        let result = condition_term(sql);
        assert!(result.is_ok());
        let (_, cond) = result.unwrap();
        assert!(matches!(cond, Condition::InSubquery(_, _)));
    }

    #[test]
    fn test_parse_not_in_subquery() {
        let sql = "product_id NOT IN (SELECT id FROM products WHERE active = 'true')";
        let result = condition_term(sql);
        assert!(result.is_ok());
        let (_, cond) = result.unwrap();
        assert!(matches!(cond, Condition::NotInSubquery(_, _)));
    }

    #[test]
    fn test_parse_scalar_subquery_equals() {
        let sql = "price = (SELECT MAX(price) FROM products)";
        let result = condition_term(sql);
        assert!(result.is_ok());
        let (_, cond) = result.unwrap();
        assert!(matches!(cond, Condition::EqualsSubquery(_, _)));
    }

    #[test]
    fn test_parse_scalar_subquery_greater_than() {
        let sql = "age > (SELECT AVG(age) FROM users)";
        let result = condition_term(sql);
        assert!(result.is_ok());
        let (_, cond) = result.unwrap();
        assert!(matches!(cond, Condition::GreaterThanSubquery(_, _)));
    }

    #[test]
    fn test_parse_scalar_subquery_in_select() {
        let sql = "SELECT name, (SELECT COUNT(*) FROM orders) AS order_count FROM users";
        let result = select(sql);
        assert!(result.is_ok());
        let (_, stmt) = result.unwrap();
        if let Statement::Select { columns, .. } = stmt {
            assert_eq!(columns.len(), 2);
            assert!(matches!(columns[1], SelectColumn::Subquery { .. }));
        } else {
            panic!("Expected Select statement");
        }
    }

    #[test]
    fn test_parse_complex_subquery_condition() {
        let sql = "SELECT * FROM users WHERE id IN (SELECT user_id FROM orders WHERE status = 'active')";
        let result = select(sql);
        assert!(result.is_ok());
        let (_, stmt) = result.unwrap();
        if let Statement::Select { filter, .. } = stmt {
            assert!(filter.is_some());
            assert!(matches!(filter.unwrap(), Condition::InSubquery(_, _)));
        } else {
            panic!("Expected Select statement");
        }
    }

    #[test]
    fn test_parse_exists_in_select() {
        let sql = "SELECT * FROM users WHERE EXISTS (SELECT 1 FROM orders)";
        let result = select(sql);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());
        let (remaining, stmt) = result.unwrap();
        assert!(remaining.trim().is_empty(), "Remaining input: {}", remaining);
        if let Statement::Select { filter, .. } = stmt {
            assert!(filter.is_some());
            assert!(matches!(filter.unwrap(), Condition::Exists(_)));
        } else {
            panic!("Expected Select statement");
        }
    }

    #[test]
    fn test_where_clause_with_exists() {
        let sql = "WHERE EXISTS (SELECT 1 FROM orders)";
        let result = where_clause(sql);
        assert!(result.is_ok(), "Failed to parse WHERE: {:?}", result.err());
        let (remaining, filter) = result.unwrap();
        assert!(remaining.trim().is_empty(), "Remaining after WHERE: {}", remaining);
        assert!(filter.is_some());
        assert!(matches!(filter.unwrap(), Condition::Exists(_)));
    }

    #[test]
    fn test_condition_with_exists() {
        let sql = "EXISTS (SELECT 1 FROM orders)";
        let result = condition(sql);
        assert!(result.is_ok(), "Failed to parse condition: {:?}", result.err());
        let (remaining, cond) = result.unwrap();
        assert!(remaining.trim().is_empty(), "Remaining after condition: {}", remaining);
        assert!(matches!(cond, Condition::Exists(_)));
    }
}
