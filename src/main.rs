use postgrustql::Server;
use config::{Config, File, Environment};
use serde::Deserialize;
use std::path::Path;

/// Конфигурация сервера
#[derive(Debug, Deserialize)]
struct ServerConfig {
    #[serde(default = "default_user")]
    user: String,
    #[serde(default = "default_password")]
    password: String,
    #[serde(default = "default_database")]
    database: String,
    #[serde(default = "default_host")]
    host: String,
    #[serde(default = "default_port")]
    port: u16,
    #[serde(default = "default_data_dir")]
    data_dir: String,
    #[serde(default = "default_initdb")]
    initdb: bool,
}

fn default_user() -> String { "postgres".to_string() }
fn default_password() -> String { "postgres".to_string() }
fn default_database() -> String { "postgres".to_string() }
fn default_host() -> String { "127.0.0.1".to_string() }
fn default_port() -> u16 { 5432 }
fn default_data_dir() -> String { "./data".to_string() }
fn default_initdb() -> bool { true }

impl ServerConfig {
    /// Load configuration with priority: ENV > config file > defaults
    fn load() -> Result<Self, config::ConfigError> {
        let mut builder = Config::builder();

        // 1. Try to load config file (optional)
        // Check multiple locations: /etc/postgrustsql/, ./
        let config_paths = [
            "/etc/postgrustsql/postgrustsql.toml",
            "./postgrustsql.toml",
        ];

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

        // 3. Build and deserialize
        let config = builder.build()?;
        config.try_deserialize()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::load().unwrap_or_else(|e| {
        eprintln!("Warning: Failed to load config: {}. Using defaults.", e);
        ServerConfig {
            user: default_user(),
            password: default_password(),
            database: default_database(),
            host: default_host(),
            port: default_port(),
            data_dir: default_data_dir(),
            initdb: default_initdb(),
        }
    });

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║          PostgrustSQL Server Starting...                 ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║  • Superuser:    {:<39} ║", config.user);
    println!("║  • Database:     {:<39} ║", config.database);
    println!("║  • Address:      {}:{:<29} ║", config.host, config.port);
    println!("╚══════════════════════════════════════════════════════════╝");

    let server = Server::new_with_config(
        &config.user,
        &config.password,
        &config.database,
        &config.data_dir,
        config.initdb,
    )?;

    let bind_addr = format!("{}:{}", config.host, config.port);
    server.start(&bind_addr).await?;

    Ok(())
}
