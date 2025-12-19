use clap::{Parser, ValueEnum};
use postgrustql::storage::StorageEngine;
use postgrustql::types::{Database, DataType, Table, Value};
use std::io::{self, Write};
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DumpFormat {
    Sql,
    Binary,
}

#[derive(Debug, Parser)]
#[command(name = "pgr_dump")]
#[command(about = "Export PostgrustQL database to SQL or binary format", long_about = None)]
struct Args {
    /// Database name to dump
    database: String,

    /// Data directory path
    #[arg(short = 'd', long, default_value = "./data")]
    data_dir: PathBuf,

    /// Export only schema (CREATE statements)
    #[arg(long)]
    schema_only: bool,

    /// Export only data (INSERT statements)
    #[arg(long)]
    data_only: bool,

    /// Output format: sql or binary
    #[arg(short = 'f', long, value_enum, default_value = "sql")]
    format: DumpFormat,

    /// Output file (default: stdout)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Validate: schema_only and data_only are mutually exclusive
    if args.schema_only && args.data_only {
        eprintln!("Error: --schema-only and --data-only cannot be used together");
        std::process::exit(1);
    }

    // Load database from storage
    let storage = StorageEngine::new(&args.data_dir)?;
    let db = storage.load_database(&args.database)?;

    // Determine output writer (stdout or file)
    let mut output: Box<dyn Write> = if let Some(path) = &args.output {
        Box::new(File::create(path)?)
    } else {
        Box::new(io::stdout())
    };

    // Dump database in specified format
    match args.format {
        DumpFormat::Sql => {
            dump_sql(&db, &mut output, args.schema_only, args.data_only)?;
        }
        DumpFormat::Binary => {
            dump_binary(&db, &mut output)?;
        }
    }

    Ok(())
}

/// Dump database as SQL statements
fn dump_sql(
    db: &Database,
    output: &mut dyn Write,
    schema_only: bool,
    data_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // SQL header
    writeln!(output, "--")?;
    writeln!(output, "-- RustDB database dump")?;
    writeln!(output, "-- Database: {}", db.name)?;
    writeln!(output, "--")?;
    writeln!(output)?;

    if !data_only {
        // Export schema
        dump_schema(db, output)?;
    }

    if !schema_only {
        // Export data
        dump_data(db, output)?;
    }

    Ok(())
}

/// Dump schema: CREATE TYPE, CREATE TABLE, CREATE INDEX, CREATE VIEW
fn dump_schema(
    db: &Database,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(output, "-- Schema")?;
    writeln!(output)?;

    // 1. CREATE TYPE for ENUMs
    if !db.enums.is_empty() {
        writeln!(output, "-- Enums")?;
        for (enum_name, values) in &db.enums {
            let values_str = values.iter()
                .map(|v| format!("'{}'", escape_sql_string(v)))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(output, "CREATE TYPE {} AS ENUM ({});", enum_name, values_str)?;
        }
        writeln!(output)?;
    }

    // 2. CREATE TABLE
    if !db.tables.is_empty() {
        writeln!(output, "-- Tables")?;
        for table in db.tables.values() {
            dump_create_table(table, output)?;
        }
        writeln!(output)?;
    }

    // 3. CREATE INDEX
    if !db.indexes.is_empty() {
        writeln!(output, "-- Indexes")?;
        for index in db.indexes.values() {
            dump_create_index(index, output)?;
        }
        writeln!(output)?;
    }

    // 4. CREATE VIEW
    if !db.views.is_empty() {
        writeln!(output, "-- Views")?;
        for (view_name, query) in &db.views {
            writeln!(output, "CREATE VIEW {} AS {};", view_name, query)?;
        }
        writeln!(output)?;
    }

    Ok(())
}

/// Dump CREATE TABLE statement
fn dump_create_table(
    table: &Table,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(output, "CREATE TABLE {} (", table.name)?;

    let column_definitions: Vec<String> = table.columns.iter().map(|col| {
        let mut def = format!("  {} {}", col.name, datatype_to_sql(&col.data_type));

        if !col.nullable {
            def.push_str(" NOT NULL");
        }

        if col.primary_key {
            def.push_str(" PRIMARY KEY");
        }

        if col.unique {
            def.push_str(" UNIQUE");
        }

        if let Some(ref fk) = col.foreign_key {
            def.push_str(&format!(" REFERENCES {}({})", fk.referenced_table, fk.referenced_column));
        }

        def
    }).collect();

    writeln!(output, "{}", column_definitions.join(",\n"))?;
    writeln!(output, ");")?;

    Ok(())
}

/// Dump CREATE INDEX statement
fn dump_create_index(
    index: &postgrustql::index::Index,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    use postgrustql::index::Index;

    let (name, table_name, column_names, is_unique, index_type) = match index {
        Index::BTree(idx) => (
            &idx.name,
            &idx.table_name,
            &idx.column_names,
            idx.is_unique,
            "BTREE",
        ),
        Index::Hash(idx) => (
            &idx.name,
            &idx.table_name,
            &idx.column_names,
            idx.is_unique,
            "HASH",
        ),
    };

    let unique_str = if is_unique { "UNIQUE " } else { "" };
    let columns_str = column_names.join(", ");

    writeln!(
        output,
        "CREATE {}INDEX {} ON {}({}) USING {};",
        unique_str, name, table_name, columns_str, index_type
    )?;

    Ok(())
}

/// Dump data as INSERT statements
fn dump_data(
    db: &Database,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    writeln!(output, "-- Data")?;
    writeln!(output)?;

    for table in db.tables.values() {
        #[allow(deprecated)]
        if table.rows.is_empty() {
            continue;
        }

        writeln!(output, "-- Table: {}", table.name)?;

        // Get column names
        let column_names: Vec<String> = table.columns.iter()
            .map(|c| c.name.clone())
            .collect();

        // Batch INSERTs for performance (100 rows per batch)
        const BATCH_SIZE: usize = 100;
        #[allow(deprecated)]
        for chunk in table.rows.chunks(BATCH_SIZE) {
            for row in chunk {
                let values_str = row.values.iter()
                    .map(|v| value_to_sql(v))
                    .collect::<Vec<_>>()
                    .join(", ");

                writeln!(
                    output,
                    "INSERT INTO {} ({}) VALUES ({});",
                    table.name,
                    column_names.join(", "),
                    values_str
                )?;
            }
        }

        writeln!(output)?;
    }

    Ok(())
}

/// Dump database as binary (bincode serialization)
fn dump_binary(
    db: &Database,
    output: &mut dyn Write,
) -> Result<(), Box<dyn std::error::Error>> {
    let encoded = bincode::serialize(db)?;
    output.write_all(&encoded)?;
    Ok(())
}

/// Convert DataType to SQL string
fn datatype_to_sql(dt: &DataType) -> String {
    match dt {
        DataType::Integer => "INTEGER".to_string(),
        DataType::SmallInt => "SMALLINT".to_string(),
        DataType::Serial => "SERIAL".to_string(),
        DataType::BigSerial => "BIGSERIAL".to_string(),
        DataType::Text => "TEXT".to_string(),
        DataType::Varchar { max_length } => format!("VARCHAR({})", max_length),
        DataType::Char { length } => format!("CHAR({})", length),
        DataType::Boolean => "BOOLEAN".to_string(),
        DataType::Date => "DATE".to_string(),
        DataType::Timestamp => "TIMESTAMP".to_string(),
        DataType::TimestampTz => "TIMESTAMPTZ".to_string(),
        DataType::Real => "REAL".to_string(),
        DataType::Numeric { precision, scale } => format!("NUMERIC({}, {})", precision, scale),
        DataType::Uuid => "UUID".to_string(),
        DataType::Json => "JSON".to_string(),
        DataType::Jsonb => "JSONB".to_string(),
        DataType::Bytea => "BYTEA".to_string(),
        DataType::Enum { name, .. } => name.clone(),
    }
}

/// Convert Value to SQL string (with proper escaping)
fn value_to_sql(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Integer(i) => i.to_string(),
        Value::SmallInt(i) => i.to_string(),
        Value::Real(f) => f.to_string(),
        Value::Numeric(d) => d.to_string(),
        Value::Text(s) | Value::Char(s) => {
            format!("'{}'", escape_sql_string(s))
        }
        Value::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Date(d) => format!("'{}'", d.format("%Y-%m-%d")),
        Value::Timestamp(ts) => format!("'{}'", ts.format("%Y-%m-%d %H:%M:%S")),
        Value::TimestampTz(ts) => format!("'{}'", ts.format("%Y-%m-%d %H:%M:%S%:z")),
        Value::Uuid(u) => format!("'{}'", u),
        Value::Json(j) => {
            format!("'{}'", escape_sql_string(j))
        }
        Value::Bytea(b) => {
            // PostgreSQL hex format: \x...
            format!("'\\x{}'", hex::encode(b))
        }
        Value::Enum(_enum_name, value) => format!("'{}'", escape_sql_string(value)),
    }
}

/// Escape single quotes in SQL strings
fn escape_sql_string(s: &str) -> String {
    s.replace('\'', "''")
}
