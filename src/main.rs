use postgrustql::Server;
use std::env;

/// Конфигурация сервера из ENV переменных
struct ServerConfig {
    user: String,
    password: String,
    database: String,
    host: String,
    port: u16,
    data_dir: String,
    initdb: bool,
}

impl ServerConfig {
    fn from_env() -> Self {
        Self {
            user: env::var("POSTGRUSTQL_USER").unwrap_or_else(|_| "postgres".to_string()),
            password: env::var("POSTGRUSTQL_PASSWORD").unwrap_or_else(|_| "postgres".to_string()),
            database: env::var("POSTGRUSTQL_DATABASE").unwrap_or_else(|_| "postgres".to_string()),
            host: env::var("POSTGRUSTQL_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            port: env::var("POSTGRUSTQL_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5432),
            data_dir: env::var("POSTGRUSTQL_DATA_DIR").unwrap_or_else(|_| "./data".to_string()),
            initdb: env::var("POSTGRUSTQL_INITDB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(true),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ServerConfig::from_env();

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║          PostgrustSQL Server Starting...                 ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!("║ Configuration:                                           ║");
    println!("║  • Superuser:    {:<39} ║", config.user);
    println!("║  • Database:     {:<39} ║", config.database);
    println!("║  • Host:Port:    {}:{:<29} ║", config.host, config.port);
    println!("║  • Data Dir:     {:<39} ║", config.data_dir);
    println!("║  • Init DB:      {:<39} ║", config.initdb);
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
