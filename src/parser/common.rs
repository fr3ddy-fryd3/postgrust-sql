use crate::types::{DataType, Value};
use chrono::{NaiveDate, NaiveDateTime, DateTime, Utc};
use uuid::Uuid;
use rust_decimal::Decimal;
use std::str::FromStr;
use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while, take_while1},
    character::complete::{alpha1, char, digit1, multispace0},
    combinator::{map, map_res, opt, recognize},
    sequence::{delimited, pair, tuple},
    IResult,
};

pub fn ws<'a, F, O>(inner: F) -> impl FnMut(&'a str) -> IResult<&'a str, O>
where
    F: FnMut(&'a str) -> IResult<&'a str, O>,
{
    delimited(multispace0, inner, multispace0)
}

pub fn identifier(input: &str) -> IResult<&str, String> {
    map(
        recognize(pair(
            alt((alpha1, tag("_"))),
            take_while(|c: char| c.is_alphanumeric() || c == '_'),
        )),
        |s: &str| s.to_string(),
    )(input)
}

// Identifier that is not a reserved keyword (v2.6.0)
// Used in condition parsing to avoid conflicts with EXISTS, NOT, etc.
pub fn non_keyword_identifier(input: &str) -> IResult<&str, String> {
    use nom::combinator::verify;

    verify(identifier, |s: &String| {
        let upper = s.to_uppercase();
        // Check if it's NOT a keyword that could conflict with condition parsing
        !matches!(upper.as_str(), "EXISTS" | "NOT" | "AND" | "OR")
    })(input)
}

pub fn data_type(input: &str) -> IResult<&str, DataType> {
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
                        opt(nom::sequence::preceded(
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

pub fn value(input: &str) -> IResult<&str, Value> {
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
                    Ok(Value::Real(s.parse().map_err(|e| format!("{e:?}"))?))
                }
            }
        ),

        // Integer - smart parsing (SmallInt vs Integer based on value)
        map_res(
            recognize(pair(opt(char('-')), digit1)),
            |s: &str| -> Result<Value, String> {
                let num = s.parse::<i64>().map_err(|e| format!("{e:?}"))?;
                if i16::try_from(num).is_ok() {
                    Ok(Value::SmallInt(num as i16))
                } else {
                    Ok(Value::Integer(num))
                }
            }
        ),
    ))(input)
}

pub fn string_literal(input: &str) -> IResult<&str, String> {
    map(
        delimited(char('\''), take_while1(|c| c != '\''), char('\'')),
        |s: &str| s.to_string(),
    )(input)
}
