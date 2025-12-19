use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use clap::Parser;
use config::{Config, File, Environment};
use serde::Deserialize;
use std::path::Path;

/// PostgrustSQL CLI Client
#[derive(Parser, Debug)]
#[command(name = "pgr_cli")]
#[command(about = "PostgrustSQL interactive CLI client", long_about = None)]
struct Args {
    /// Server host
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// Server port
    #[arg(short = 'p', long)]
    port: Option<u16>,

    /// Database user (not used for connection, reserved for future auth)
    #[arg(short = 'U', long)]
    user: Option<String>,

    /// Database name (not used for connection, reserved for future auth)
    #[arg(short = 'd', long)]
    database: Option<String>,
}

/// Client configuration
#[derive(Debug, Deserialize)]
struct ClientConfig {
    #[serde(default = "default_host")]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_user")]
    user: String,
    #[serde(default = "default_database")]
    database: String,
}

fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 5432 }
fn default_user() -> String { "postgres".to_string() }
fn default_database() -> String { "postgres".to_string() }

impl ClientConfig {
    /// Load configuration with priority: CLI args > ENV > config file > defaults
    fn load(args: &Args) -> Self {
        // 1. Try to load config file (optional)
        let config_paths = [
            "/etc/postgrustsql/postgrustsql.toml",
            "./postgrustsql.toml",
        ];

        let mut builder = Config::builder();

        for path in &config_paths {
            if Path::new(path).exists() {
                builder = builder.add_source(File::with_name(path));
                eprintln!("Loaded config from: {}", path);
                break;
            }
        }

        // 2. Override with environment variables (POSTGRUSTQL_*)
        builder = builder.add_source(
            Environment::with_prefix("POSTGRUSTQL")
                .separator("_")
        );

        // 3. Build config with defaults
        let config = builder.build().ok();
        let base_config = config
            .and_then(|c| c.try_deserialize::<ClientConfig>().ok())
            .unwrap_or_else(|| ClientConfig {
                host: default_host(),
                port: default_port(),
                user: default_user(),
                database: default_database(),
            });

        // 4. CLI args override everything
        ClientConfig {
            host: args.host.clone().unwrap_or(base_config.host),
            port: args.port.unwrap_or(base_config.port),
            user: args.user.clone().unwrap_or(base_config.user),
            database: args.database.clone().unwrap_or(base_config.database),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let config = ClientConfig::load(&args);

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║            PostgrustSQL CLI Client v2.2.2                ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!("Connecting to {}:{}...", config.host, config.port);
    println!("User: {}, Database: {}\n", config.user, config.database);

    let addr = format!("{}:{}", config.host, config.port);
    let mut stream = match TcpStream::connect(&addr).await {
        Ok(s) => {
            println!("✓ Connected!\n");
            s
        }
        Err(e) => {
            eprintln!("✗ Connection failed: {}", e);
            eprintln!("\nTroubleshooting:");
            eprintln!("  1. Check if server is running: ps aux | grep postgrustql");
            eprintln!("  2. Check server logs");
            eprintln!("  3. Verify host and port settings");
            return Err(e.into());
        }
    };

    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    // Read and display welcome messages
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).await?;
        print!("{}", line);
        // Server prompt can be "postgres=# " or "postgrustql> "
        if line.trim().ends_with('#') || line.trim().ends_with('>') {
            // Server sent initial prompt
            break;
        }
    }

    // Initialize rustyline editor for history and line editing
    let mut rl = DefaultEditor::new()?;

    // Try to load history from file (if it exists)
    let history_file = dirs::home_dir()
        .map(|mut p| {
            p.push(".pgr_cli_history");
            p
        });

    if let Some(ref path) = history_file {
        let _ = rl.load_history(path); // Ignore error if file doesn't exist
    }

    println!(); // Newline after server messages
    println!("Type 'help' for command help, 'quit' or 'exit' to quit.\n");

    loop {
        // Use rustyline to read input (supports history, editing, etc.)
        let readline = rl.readline("pgr_cli> ");

        match readline {
            Ok(query) => {
                let query = query.trim();

                if query.is_empty() {
                    continue;
                }

                // Add to history
                let _ = rl.add_history_entry(query);

                // Handle meta-commands (psql-like)
                let actual_query = if query.starts_with('\\') {
                    match query {
                        "\\q" | "\\quit" => "quit",
                        "\\l" | "\\list" => "SELECT datname FROM pg_database;",
                        "\\d" | "\\dt" => "SHOW TABLES;",
                        "\\?" | "\\h" | "\\help" => {
                            println!("Meta-commands:");
                            println!("  \\q, \\quit          - Quit");
                            println!("  \\l, \\list          - List databases");
                            println!("  \\d, \\dt            - List tables");
                            println!("  \\d <table>         - Describe table (not implemented)");
                            println!("  \\?, \\h, \\help      - Show this help");
                            println!("\nSQL commands: CREATE, INSERT, SELECT, UPDATE, DELETE, etc.");
                            continue;
                        }
                        _ if query.starts_with("\\d ") => {
                            // \d table_name - for now just pass as-is
                            // TODO: implement DESCRIBE table_name
                            query
                        }
                        _ => {
                            println!("Unknown meta-command: {}. Use \\? for help.", query);
                            continue;
                        }
                    }
                } else {
                    query
                };

                // Check for quit/exit
                if actual_query.eq_ignore_ascii_case("quit") || actual_query.eq_ignore_ascii_case("exit") {
                    writer.write_all(b"quit\n").await?;
                    writer.flush().await?;

                    // Read goodbye message
                    loop {
                        line.clear();
                        let n = reader.read_line(&mut line).await?;
                        if n == 0 {
                            break;
                        }
                        print!("{}", line);
                        if line.contains("Goodbye") {
                            break;
                        }
                    }

                    // Save history before exiting
                    if let Some(ref path) = history_file {
                        let _ = rl.save_history(path);
                    }

                    println!("\n╔══════════════════════════════════════════════════════════╗");
                    println!("║                    Session closed                        ║");
                    println!("╚══════════════════════════════════════════════════════════╝");

                    return Ok(());
                }

                // Send query
                writer.write_all(actual_query.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;

                // Read response until we see the prompt
                loop {
                    line.clear();
                    let n = reader.read_line(&mut line).await?;

                    if n == 0 {
                        eprintln!("\n✗ Connection closed by server");

                        // Save history before exiting
                        if let Some(ref path) = history_file {
                            let _ = rl.save_history(path);
                        }

                        return Ok(());
                    }

                    // Check if this line contains the prompt
                    if line.trim().ends_with('>') {
                        // Server sent prompt - ready for next input
                        break;
                    } else {
                        // Print the response line
                        print!("{}", line);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C
                println!("^C");
                writer.write_all(b"quit\n").await?;
                writer.flush().await?;

                // Save history before exiting
                if let Some(ref path) = history_file {
                    let _ = rl.save_history(path);
                }

                println!("\n╔══════════════════════════════════════════════════════════╗");
                println!("║                  Session interrupted                     ║");
                println!("╚══════════════════════════════════════════════════════════╝");

                return Ok(());
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D
                println!("quit");
                writer.write_all(b"quit\n").await?;
                writer.flush().await?;

                // Save history before exiting
                if let Some(ref path) = history_file {
                    let _ = rl.save_history(path);
                }

                println!("\n╔══════════════════════════════════════════════════════════╗");
                println!("║                    Session closed                        ║");
                println!("╚══════════════════════════════════════════════════════════╝");

                return Ok(());
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                return Err(err.into());
            }
        }
    }
}
