use crate::types::{DataType, Value};
use chrono::{NaiveDate, NaiveDateTime, DateTime, Utc, TimeZone};
use uuid::Uuid;
use rust_decimal::Decimal;
use std::str::FromStr;
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while, take_while1},
    character::complete::{alpha1, char, digit1, multispace0},
    combinator::{map, map_res, opt, recognize},
    multi::separated_list1,
    sequence::{delimited, pair, preceded, tuple},
    IResult,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    CreateTable {
        name: String,
        columns: Vec<ColumnDef>,
    },
    DropTable {
        name: String,
    },
    Insert {
        table: String,
        columns: Option<Vec<String>>,
        values: Vec<Value>,
    },
    Select {
        columns: Vec<SelectColumn>,
        from: String,
        joins: Vec<JoinClause>,
        filter: Option<Condition>,
        group_by: Option<Vec<String>>,
        order_by: Option<(String, SortOrder)>,
        limit: Option<usize>,
    },
    Update {
        table: String,
        assignments: Vec<(String, Value)>,
        filter: Option<Condition>,
    },
    Delete {
        from: String,
        filter: Option<Condition>,
    },
    Begin,
    Commit,
    Rollback,
    ShowTables,
    // User management
    CreateUser {
        username: String,
        password: String,
        is_superuser: bool,
    },
    DropUser {
        username: String,
    },
    AlterUser {
        username: String,
        password: String,
    },
    // Database management
    CreateDatabase {
        name: String,
        owner: Option<String>,
    },
    DropDatabase {
        name: String,
    },
    // Privileges
    Grant {
        privilege: PrivilegeType,
        on_database: String,
        to_user: String,
    },
    Revoke {
        privilege: PrivilegeType,
        on_database: String,
        from_user: String,
    },
    // Metadata queries
    ShowUsers,
    ShowDatabases,
    // Enum types
    CreateType {
        name: String,
        values: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrivilegeType {
    Connect,
    Create,
    Select,
    Insert,
    Update,
    Delete,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub foreign_key: Option<crate::types::ForeignKey>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Equals(String, Value),
    NotEquals(String, Value),
    GreaterThan(String, Value),
    LessThan(String, Value),
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectColumn {
    Regular(String),              // Regular column name or *
    Aggregate(AggregateFunction), // Aggregate function
}

#[derive(Debug, Clone, PartialEq)]
pub enum AggregateFunction {
    Count(CountTarget),
    Sum(String),
    Avg(String),
    Min(String),
    Max(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CountTarget {
    All,           // COUNT(*)
    Column(String), // COUNT(column)
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinType {
    Inner,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinClause {
    pub join_type: JoinType,
    pub table: String,
    pub on_left: String,  // left_table.column
    pub on_right: String, // right_table.column
}

fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    delimited(multispace0, inner, multispace0)
}

fn identifier(input: &str) -> IResult<&str, String> {
    map(
        recognize(pair(
            alt((alpha1, tag("_"))),
            take_while(|c: char| c.is_alphanumeric() || c == '_'),
        )),
        |s: &str| s.to_string(),
    )(input)
}

fn data_type(input: &str) -> IResult<&str, DataType> {
    alt((
        // Auto-increment types
        map(tag_no_case("BIGSERIAL"), |_| DataType::BigSerial),
        map(tag_no_case("SERIAL"), |_| DataType::Serial),
        // Numeric types with precision/scale
        map(
            tuple((
                alt((tag_no_case("NUMERIC"), tag_no_case("DECIMAL"))),
                opt(delimited(
                    ws(char('(')),
                    tuple((
                        ws(map_res(digit1, |s: &str| s.parse::<u8>())),
                        opt(preceded(
                            ws(char(',')),
                            ws(map_res(digit1, |s: &str| s.parse::<u8>())),
                        )),
                    )),
                    ws(char(')')),
                )),
            )),
            |(_, params)| match params {
                Some((p, Some(s))) => DataType::Numeric { precision: p, scale: s },
                Some((p, None)) => DataType::Numeric { precision: p, scale: 0 },
                None => DataType::Numeric { precision: 10, scale: 0 },
            }
        ),
        // Integer types
        map(tag_no_case("SMALLINT"), |_| DataType::SmallInt),
        map(tag_no_case("INTEGER"), |_| DataType::Integer),
        map(tag_no_case("INT"), |_| DataType::Integer),
        map(tag_no_case("BIGINT"), |_| DataType::Integer), // Same as INTEGER for now
        // Floating point
        map(alt((tag_no_case("REAL"), tag_no_case("FLOAT"))), |_| DataType::Real),
        map(tag_no_case("DOUBLE PRECISION"), |_| DataType::Real),
        // String types with length
        map(
            tuple((
                tag_no_case("VARCHAR"),
                opt(delimited(
                    ws(char('(')),
                    ws(map_res(digit1, |s: &str| s.parse::<usize>())),
                    ws(char(')')),
                )),
            )),
            |(_, len)| DataType::Varchar { max_length: len.unwrap_or(255) }
        ),
        map(
            tuple((
                tag_no_case("CHAR"),
                opt(delimited(
                    ws(char('(')),
                    ws(map_res(digit1, |s: &str| s.parse::<usize>())),
                    ws(char(')')),
                )),
            )),
            |(_, len)| DataType::Char { length: len.unwrap_or(1) }
        ),
        map(tag_no_case("TEXT"), |_| DataType::Text),
        // Boolean
        map(alt((tag_no_case("BOOLEAN"), tag_no_case("BOOL"))), |_| DataType::Boolean),
        // Date/Time types
        map(tag_no_case("TIMESTAMPTZ"), |_| DataType::TimestampTz),
        map(tag_no_case("TIMESTAMP"), |_| DataType::Timestamp),
        map(tag_no_case("DATE"), |_| DataType::Date),
        // Special types
        map(tag_no_case("UUID"), |_| DataType::Uuid),
        map(tag_no_case("JSONB"), |_| DataType::Jsonb),
        map(tag_no_case("JSON"), |_| DataType::Json),
        map(tag_no_case("BYTEA"), |_| DataType::Bytea),
        // Custom ENUM types - fallback that captures any identifier
        // This will be resolved to DataType::Enum during execution
        map(identifier, |name| DataType::Enum {
            name,
            values: vec![] // Empty values, will be resolved from Database.enums
        }),
    ))(input)
}

fn value(input: &str) -> IResult<&str, Value> {
    alt((
        // NULL
        map(tag_no_case("NULL"), |_| Value::Null),

        // Boolean
        map(tag_no_case("TRUE"), |_| Value::Boolean(true)),
        map(tag_no_case("FALSE"), |_| Value::Boolean(false)),

        // UUID: '550e8400-e29b-41d4-a716-446655440000'
        map_res(
            delimited(
                char('\''),
                recognize(tuple((
                    take_while1(|c: char| c.is_ascii_hexdigit() || c == '-'),
                ))),
                char('\'')
            ),
            |s: &str| {
                Uuid::parse_str(s).map(Value::Uuid)
            }
        ),

        // Date/Timestamp/Text in quotes
        map_res(
            delimited(char('\''), take_while1(|c| c != '\''), char('\'')),
            |s: &str| -> Result<Value, String> {
                // Try to parse as date first
                if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
                    return Ok(Value::Date(d));
                }
                // Try timestamp with timezone
                if let Ok(t) = DateTime::parse_from_rfc3339(s) {
                    return Ok(Value::TimestampTz(t.with_timezone(&Utc)));
                }
                // Try timestamp without timezone
                if let Ok(t) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                    return Ok(Value::Timestamp(t));
                }
                // Otherwise, treat as text
                Ok(Value::Text(s.to_string()))
            }
        ),

        // Numeric with decimal point - try Decimal first, then Real
        map_res(
            recognize(tuple((
                opt(char('-')),
                digit1,
                char('.'),
                digit1,
            ))),
            |s: &str| -> Result<Value, String> {
                // Try as Decimal first for exact precision
                if let Ok(d) = Decimal::from_str(s) {
                    Ok(Value::Numeric(d))
                } else {
                    Ok(Value::Real(s.parse().map_err(|e| format!("{:?}", e))?))
                }
            }
        ),

        // Integer - smart parsing (SmallInt vs Integer based on value)
        map_res(
            recognize(pair(opt(char('-')), digit1)),
            |s: &str| -> Result<Value, String> {
                let num = s.parse::<i64>().map_err(|e| format!("{:?}", e))?;
                if num >= i16::MIN as i64 && num <= i16::MAX as i64 {
                    Ok(Value::SmallInt(num as i16))
                } else {
                    Ok(Value::Integer(num))
                }
            }
        ),
    ))(input)
}

// Парсер для строковых литералов (используется в CREATE USER и т.д.)
fn string_literal(input: &str) -> IResult<&str, String> {
    map(
        delimited(char('\''), take_while1(|c| c != '\''), char('\'')),
        |s: &str| s.to_string(),
    )(input)
}

fn column_def(input: &str) -> IResult<&str, ColumnDef> {
    let (input, name) = ws(identifier)(input)?;
    let (input, data_type) = ws(data_type)(input)?;
    let (input, primary_key) = opt(ws(tag_no_case("PRIMARY KEY")))(input)?;
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

    Ok((
        input,
        ColumnDef {
            name,
            data_type,
            nullable,
            primary_key,
            foreign_key,
        },
    ))
}

fn create_table(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE TABLE"))(input)?;
    let (input, name) = ws(identifier)(input)?;
    let (input, columns) = delimited(
        ws(char('(')),
        separated_list1(ws(char(',')), column_def),
        ws(char(')')),
    )(input)?;

    Ok((input, Statement::CreateTable { name, columns }))
}

fn drop_table(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP TABLE"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    Ok((input, Statement::DropTable { name }))
}

fn insert(input: &str) -> IResult<&str, Statement> {
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
fn condition(input: &str) -> IResult<&str, Condition> {
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
fn join_clause(input: &str) -> IResult<&str, JoinClause> {
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

fn select(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("SELECT"))(input)?;
    let (input, columns) = separated_list1(ws(char(',')), select_column)(input)?;
    let (input, _) = ws(tag_no_case("FROM"))(input)?;
    let (input, from) = ws(identifier)(input)?;

    // Parse optional JOIN clauses
    let (input, joins) = nom::multi::many0(join_clause)(input)?;

    let (input, filter) = opt(preceded(ws(tag_no_case("WHERE")), condition))(input)?;

    // Parse optional GROUP BY clause
    let (input, group_by) = opt(preceded(
        ws(tag_no_case("GROUP BY")),
        separated_list1(ws(char(',')), ws(identifier)),
    ))(input)?;

    // Parse optional ORDER BY clause
    let (input, order_by) = opt(preceded(
        ws(tag_no_case("ORDER BY")),
        tuple((
            ws(identifier),
            opt(alt((
                map(ws(tag_no_case("ASC")), |_| SortOrder::Asc),
                map(ws(tag_no_case("DESC")), |_| SortOrder::Desc),
            ))),
        )),
    ))(input)?;

    let order_by = order_by.map(|(col, sort)| (col, sort.unwrap_or(SortOrder::Asc)));

    // Parse optional LIMIT clause
    let (input, limit) = opt(preceded(
        ws(tag_no_case("LIMIT")),
        map(
            take_while1(|c: char| c.is_numeric()),
            |s: &str| s.parse::<usize>().unwrap(),
        ),
    ))(input)?;

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

fn update(input: &str) -> IResult<&str, Statement> {
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

fn delete(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DELETE FROM"))(input)?;
    let (input, from) = ws(identifier)(input)?;
    let (input, filter) = opt(preceded(ws(tag_no_case("WHERE")), condition))(input)?;

    Ok((input, Statement::Delete { from, filter }))
}

fn begin(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(alt((
        tag_no_case("BEGIN"),
        tag_no_case("BEGIN TRANSACTION"),
        tag_no_case("START TRANSACTION"),
    )))(input)?;
    Ok((input, Statement::Begin))
}

fn commit(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(alt((
        tag_no_case("COMMIT"),
        tag_no_case("COMMIT TRANSACTION"),
    )))(input)?;
    Ok((input, Statement::Commit))
}

fn rollback(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(alt((
        tag_no_case("ROLLBACK"),
        tag_no_case("ROLLBACK TRANSACTION"),
    )))(input)?;
    Ok((input, Statement::Rollback))
}

fn show_tables(input: &str) -> IResult<&str, Statement> {
    // Support both "SHOW TABLES" (MySQL-style) and "\dt" or "\d" (psql-style)
    let (input, _) = ws(alt((
        tag_no_case("SHOW TABLES"),
        tag("\\dt"),
        tag("\\d"),
    )))(input)?;
    Ok((input, Statement::ShowTables))
}

fn show_users(input: &str) -> IResult<&str, Statement> {
    // Support both "SHOW USERS" and "\du" (psql-style)
    let (input, _) = ws(alt((
        tag_no_case("SHOW USERS"),
        tag("\\du"),
    )))(input)?;
    Ok((input, Statement::ShowUsers))
}

fn show_databases(input: &str) -> IResult<&str, Statement> {
    // Support both "SHOW DATABASES" and "\l" (psql-style)
    let (input, _) = ws(alt((
        tag_no_case("SHOW DATABASES"),
        tag("\\l"),
    )))(input)?;
    Ok((input, Statement::ShowDatabases))
}

// CREATE USER username WITH PASSWORD 'password' [SUPERUSER]
fn create_user(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE USER"))(input)?;
    let (input, username) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("WITH PASSWORD"))(input)?;
    let (input, password) = ws(string_literal)(input)?;
    let (input, is_superuser) = opt(ws(tag_no_case("SUPERUSER")))(input)?;

    Ok((input, Statement::CreateUser {
        username: username.to_string(),
        password,
        is_superuser: is_superuser.is_some(),
    }))
}

// DROP USER username
fn drop_user(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP USER"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::DropUser {
        username: username.to_string(),
    }))
}

// ALTER USER username WITH PASSWORD 'new_password'
fn alter_user(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("ALTER USER"))(input)?;
    let (input, username) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("WITH PASSWORD"))(input)?;
    let (input, password) = ws(string_literal)(input)?;

    Ok((input, Statement::AlterUser {
        username: username.to_string(),
        password,
    }))
}

// CREATE DATABASE dbname [WITH OWNER username]
fn create_database(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("CREATE DATABASE"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    // Support both "WITH OWNER" (PostgreSQL) and "OWNER" (backwards compat)
    let (input, owner) = opt(alt((
        preceded(ws(tag_no_case("WITH OWNER")), ws(identifier)),
        preceded(ws(tag_no_case("OWNER")), ws(identifier)),
    )))(input)?;

    Ok((input, Statement::CreateDatabase {
        name: name.to_string(),
        owner: owner.map(|s| s.to_string()),
    }))
}

// DROP DATABASE dbname
fn drop_database(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("DROP DATABASE"))(input)?;
    let (input, name) = ws(identifier)(input)?;

    Ok((input, Statement::DropDatabase {
        name: name.to_string(),
    }))
}

// Parse privilege type
fn privilege_type(input: &str) -> IResult<&str, PrivilegeType> {
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

// GRANT privilege ON DATABASE dbname TO username
fn grant(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("GRANT"))(input)?;
    let (input, privilege) = ws(privilege_type)(input)?;
    let (input, _) = ws(tag_no_case("ON DATABASE"))(input)?;
    let (input, db_name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("TO"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::Grant {
        privilege,
        on_database: db_name.to_string(),
        to_user: username.to_string(),
    }))
}

// REVOKE privilege ON DATABASE dbname FROM username
fn revoke(input: &str) -> IResult<&str, Statement> {
    let (input, _) = ws(tag_no_case("REVOKE"))(input)?;
    let (input, privilege) = ws(privilege_type)(input)?;
    let (input, _) = ws(tag_no_case("ON DATABASE"))(input)?;
    let (input, db_name) = ws(identifier)(input)?;
    let (input, _) = ws(tag_no_case("FROM"))(input)?;
    let (input, username) = ws(identifier)(input)?;

    Ok((input, Statement::Revoke {
        privilege,
        on_database: db_name.to_string(),
        from_user: username.to_string(),
    }))
}

// CREATE TYPE mood AS ENUM ('happy', 'sad', 'neutral')
fn create_type(input: &str) -> IResult<&str, Statement> {
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

pub fn parse_statement(input: &str) -> Result<Statement, String> {
    let input = input.trim();
    let input = input.trim_end_matches(';');

    let result = alt((
        show_users,
        show_databases,
        show_tables,
        begin,
        commit,
        rollback,
        create_type,
        create_user,
        drop_user,
        alter_user,
        create_database,
        drop_database,
        grant,
        revoke,
        create_table,
        drop_table,
        insert,
        select,
        update,
        delete,
    ))(input);

    match result {
        Ok((remaining, stmt)) => {
            if remaining.trim().is_empty() {
                Ok(stmt)
            } else {
                Err(format!("Unexpected input after statement: {}", remaining))
            }
        }
        Err(e) => Err(format!("Parse error: {:?}", e)),
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
}
