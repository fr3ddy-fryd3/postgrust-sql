use clap::{Parser, ValueEnum};
use postgrustql::parser::parse_statement;
use postgrustql::executor::QueryExecutor;
use postgrustql::storage::{DatabaseStorage, StorageEngine};
use postgrustql::types::Database;
use postgrustql::transaction::GlobalTransactionManager;
use std::sync::Arc;
use std::io::{self, Read};
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum RestoreFormat {
    Sql,
    Binary,
    Auto, // Auto-detect based on content
}

#[derive(Debug, Parser)]
#[command(name = "pgr_restore")]
#[command(about = "Import PostgrustQL database from SQL or binary dump", long_about = None)]
struct Args {
    /// Database name to restore into
    database: String,

    /// Data directory path
    #[arg(short = 'd', long, default_value = "./data")]
    data_dir: PathBuf,

    /// Input format: sql, binary, or auto (default: auto-detect)
    #[arg(short = 'f', long, value_enum, default_value = "auto")]
    format: RestoreFormat,

    /// Input file (default: stdin)
    #[arg(short = 'i', long)]
    input: Option<PathBuf>,

    /// Dry run: validate SQL without executing
    #[arg(long)]
    dry_run: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Read input from file or stdin
    let mut input_data = Vec::new();
    if let Some(path) = &args.input {
        File::open(path)?.read_to_end(&mut input_data)?;
    } else {
        io::stdin().read_to_end(&mut input_data)?;
    }

    // Auto-detect format if needed
    let format = match args.format {
        RestoreFormat::Auto => detect_format(&input_data),
        f => f,
    };

    // Load or create database
    let mut storage = StorageEngine::new(&args.data_dir)?;
    let mut db = storage.load_database(&args.database)?;

    match format {
        RestoreFormat::Binary => {
            restore_binary(&input_data, &args.database, &mut storage)?;
            println!("Binary restore completed successfully");
        }
        RestoreFormat::Sql | RestoreFormat::Auto => {
            let input_str = String::from_utf8(input_data)?;
            let statements_executed = restore_sql(
                &input_str,
                &mut db,
                &mut storage,
                &args.data_dir,
                args.dry_run
            )?;

            if args.dry_run {
                println!("Dry run: {} SQL statements validated successfully", statements_executed);
            } else {
                println!("SQL restore completed: {} statements executed", statements_executed);
            }
        }
    }

    Ok(())
}

/// Auto-detect format based on content
fn detect_format(data: &[u8]) -> RestoreFormat {
    // Try to parse as UTF-8 first
    if let Ok(s) = std::str::from_utf8(data) {
        // If it starts with SQL comments or keywords, it's SQL
        let trimmed = s.trim_start();
        if trimmed.starts_with("--")
            || trimmed.starts_with("CREATE")
            || trimmed.starts_with("INSERT")
            || trimmed.starts_with("BEGIN") {
            return RestoreFormat::Sql;
        }
    }

    // Otherwise assume binary
    RestoreFormat::Binary
}

/// Restore from SQL dump
fn restore_sql(
    input: &str,
    db: &mut Database,
    storage: &mut StorageEngine,
    data_dir: &std::path::Path,
    dry_run: bool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut statements_executed = 0;
    const BUFFER_POOL_SIZE: usize = 1000; // 1000 pages * 8KB = 8MB cache
    let mut db_storage = DatabaseStorage::new(data_dir, BUFFER_POOL_SIZE)?;
    let tx_manager = Arc::new(GlobalTransactionManager::new());

    // Split input into individual SQL statements
    // Simple splitting by semicolon (good enough for dumps)
    for statement_str in split_sql_statements(input) {
        let trimmed = statement_str.trim();

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }

        // Parse SQL statement
        let statement = match parse_statement(trimmed) {
            Ok(stmt) => stmt,
            Err(e) => {
                eprintln!("Parse error in statement:\n{}\nError: {:?}", trimmed, e);
                return Err(format!("Parse error: {:?}", e).into());
            }
        };

        if dry_run {
            // Just validate, don't execute
            statements_executed += 1;
            continue;
        }

        // Execute statement (auto-commit mode: active_tx_id = None)
        match QueryExecutor::execute(
            db,
            statement,
            Some(storage),
            &tx_manager,
            &mut db_storage,
            None,  // auto-commit
        ) {
            Ok(_result) => {
                statements_executed += 1;
            }
            Err(e) => {
                eprintln!("Execution error in statement:\n{}\nError: {:?}", trimmed, e);
                return Err(format!("Execution error: {:?}", e).into());
            }
        }
    }

    if !dry_run {
        // Save database to disk
        storage.create_checkpoint(db)?;
    }

    Ok(statements_executed)
}

/// Split SQL input into individual statements
/// Handles multi-line statements and comments
fn split_sql_statements(input: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current_statement = String::new();
    let mut in_string = false;
    let mut escape_next = false;

    for line in input.lines() {
        let trimmed = line.trim();

        // Skip comment-only lines
        if trimmed.starts_with("--") && current_statement.is_empty() {
            continue;
        }

        // Process line character by character
        for ch in line.chars() {
            if escape_next {
                current_statement.push(ch);
                escape_next = false;
                continue;
            }

            match ch {
                '\'' => {
                    in_string = !in_string;
                    current_statement.push(ch);
                }
                '\\' if in_string => {
                    escape_next = true;
                    current_statement.push(ch);
                }
                ';' if !in_string => {
                    // End of statement
                    current_statement.push(ch);
                    statements.push(current_statement.trim().to_string());
                    current_statement.clear();
                }
                _ => {
                    current_statement.push(ch);
                }
            }
        }

        // Add newline if we're in the middle of a statement
        if !current_statement.is_empty() {
            current_statement.push('\n');
        }
    }

    // Add final statement if exists
    if !current_statement.trim().is_empty() {
        statements.push(current_statement.trim().to_string());
    }

    statements
}

/// Restore from binary dump
fn restore_binary(
    data: &[u8],
    db_name: &str,
    storage: &mut StorageEngine,
) -> Result<(), Box<dyn std::error::Error>> {
    // Deserialize database
    let db: Database = bincode::deserialize(data)?;

    // Verify database name matches
    if db.name != db_name {
        eprintln!(
            "Warning: Database name mismatch. Dump: '{}', Target: '{}'",
            db.name, db_name
        );
    }

    // Save to disk
    storage.create_checkpoint(&db)?;

    Ok(())
}
