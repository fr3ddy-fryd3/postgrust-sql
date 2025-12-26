use crate::executor::{QueryExecutor, QueryResult};
use crate::network::pg_protocol::{self, Message, StartupMessage, frontend, transaction_status};
use crate::network::prepared_statements::{PreparedStatementCache, substitute_parameters};
use crate::parser::parse_statement;
use crate::storage::StorageEngine;
use crate::transaction::{GlobalTransactionManager, Transaction};
use crate::types::{DatabaseError, ServerInstance, Value};
use comfy_table::{Cell, Table as ComfyTable, presets::UTF8_FULL};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Контекст сессии пользователя
struct SessionContext {
    username: String,
    database_name: String,
    is_authenticated: bool,
    prepared_statements: PreparedStatementCache, // v2.4.0: Extended Query Protocol
}

impl SessionContext {
    fn new() -> Self {
        Self {
            username: String::new(),
            database_name: String::new(),
            is_authenticated: false,
            prepared_statements: PreparedStatementCache::new(),
        }
    }

    fn authenticate(&mut self, username: String, database_name: String) {
        self.username = username;
        self.database_name = database_name;
        self.is_authenticated = true;
    }
}

pub struct Server {
    instance: Arc<Mutex<ServerInstance>>,
    storage: Arc<Mutex<StorageEngine>>,
    tx_manager: GlobalTransactionManager,
    database_storage: Option<Arc<Mutex<crate::storage::DatabaseStorage>>>,
}

impl Server {
    /// Создает новый сервер с конфигурацией
    pub fn new_with_config(
        superuser: &str,
        password: &str,
        initial_db: &str,
        data_dir: &str,
        init_db: bool,
    ) -> Result<Self, DatabaseError> {
        let mut storage = StorageEngine::new(data_dir)?;

        // Загружаем существующий ServerInstance или создаем новый
        let instance = if init_db {
            // Пробуем загрузить существующий
            match storage.load_server_instance() {
                Ok(mut existing) if !existing.databases.is_empty() => {
                    println!("✓ Loaded existing server instance");
                    println!("  - Databases: {}", existing.databases.len());
                    println!("  - Users: {}", existing.users.len());

                    // Проверяем, есть ли суперпользователь
                    if !existing.users.contains_key(superuser) {
                        println!("  - Creating superuser: {superuser}");
                        existing.users.insert(
                            superuser.to_string(),
                            crate::types::User::new(superuser.to_string(), password, true),
                        );
                    }

                    // Проверяем, есть ли начальная БД
                    if !existing.databases.contains_key(initial_db) {
                        println!("  - Creating initial database: {initial_db}");
                        existing.create_database(initial_db, superuser)?;
                    }

                    existing
                }
                _ => {
                    // Создаем новый
                    println!("✓ Initializing new server instance");
                    println!("  - Superuser: {superuser}");
                    println!("  - Initial database: {initial_db}");
                    ServerInstance::initialize(superuser, password, initial_db)
                }
            }
        } else {
            // Только загружаем существующий
            storage.load_server_instance()?
        };

        // Сохраняем начальный snapshot
        storage.create_checkpoint_instance(&instance)?;

        let tx_manager = GlobalTransactionManager::new();

        // v2.0.2: Page-based storage is now mandatory (always enabled)
        let use_page_storage = std::env::var("RUSTDB_USE_PAGE_STORAGE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true); // Changed from false to true in v2.0.2

        let database_storage = if use_page_storage {
            const BUFFER_POOL_SIZE: usize = 1000; // 1000 pages * 8KB = 8MB cache
            match crate::storage::DatabaseStorage::new(data_dir, BUFFER_POOL_SIZE) {
                Ok(db_storage) => {
                    println!("✓ Page-based storage enabled (8MB buffer pool)");
                    Some(Arc::new(Mutex::new(db_storage)))
                }
                Err(e) => {
                    eprintln!("✗ Failed to initialize page storage: {e}");
                    eprintln!("  Falling back to legacy Vec<Row> storage");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            instance: Arc::new(Mutex::new(instance)),
            storage: Arc::new(Mutex::new(storage)),
            tx_manager,
            database_storage,
        })
    }

    pub async fn start(&self, addr: &str) -> Result<(), Box<dyn std::error::Error>> {
        let listener = TcpListener::bind(addr).await?;
        println!(
            "
╔══════════════════════════════════════════════════════════╗
║       🚀 PostgrustSQL Server is Ready!                   ║
║                                                          ║
║  Listening on: {addr:<41} ║
╚══════════════════════════════════════════════════════════╝
"
        );

        loop {
            let (socket, addr) = listener.accept().await?;
            println!("→ New connection from {addr}");

            let instance = Arc::clone(&self.instance);
            let storage = Arc::clone(&self.storage);
            let tx_manager = self.tx_manager.clone();
            let database_storage = self.database_storage.as_ref().map(Arc::clone);

            tokio::spawn(async move {
                if let Err(e) = Self::handle_client_auto(
                    socket,
                    instance,
                    storage,
                    tx_manager,
                    database_storage,
                )
                .await
                {
                    eprintln!("✗ Error handling client {addr}: {e}");
                }
            });
        }
    }

    async fn handle_client_auto(
        socket: TcpStream,
        instance: Arc<Mutex<ServerInstance>>,
        storage: Arc<Mutex<StorageEngine>>,
        tx_manager: GlobalTransactionManager,
        database_storage: Option<Arc<Mutex<crate::storage::DatabaseStorage>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Peek at the first 8 bytes to determine protocol
        // Use timeout to avoid deadlock with clients that expect server to speak first
        let mut peek_buf = [0u8; 8];
        let peek_result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            socket.peek(&mut peek_buf)
        ).await;

        // If timeout or no data, assume text protocol (client expects server greeting)
        let is_postgres = if let Ok(Ok(_)) = peek_result {
            // PostgreSQL protocol starts with Int32 length followed by Int32 code
            // Code can be:
            // - Protocol version 3.0: 196608 (0x00030000)
            // - SSL request: 80877103 (0x04D2162F)
            // Text protocol starts with ASCII text
            let length = i32::from_be_bytes([peek_buf[0], peek_buf[1], peek_buf[2], peek_buf[3]]);
            let code = i32::from_be_bytes([peek_buf[4], peek_buf[5], peek_buf[6], peek_buf[7]]);

            // If length is reasonable (< 10000) and code matches PostgreSQL protocol or SSL request
            length > 0
                && length < 10000
                && (code == pg_protocol::PROTOCOL_VERSION || code == pg_protocol::SSL_REQUEST_CODE)
        } else {
            false
        };

        if is_postgres {
            Self::handle_postgres_client(socket, instance, storage, tx_manager, database_storage)
                .await
        } else {
            Self::handle_text_client(socket, instance, storage, tx_manager, database_storage).await
        }
    }

    async fn handle_postgres_client(
        socket: TcpStream,
        instance: Arc<Mutex<ServerInstance>>,
        storage: Arc<Mutex<StorageEngine>>,
        tx_manager: GlobalTransactionManager,
        database_storage: Option<Arc<Mutex<crate::storage::DatabaseStorage>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (mut reader, mut writer) = socket.into_split();

        // Check for SSLRequest first
        // Read length
        let length = reader.read_i32().await?;
        let code = reader.read_i32().await?;

        let mut session = SessionContext::new();

        if code == pg_protocol::SSL_REQUEST_CODE {
            // Reject SSL - send 'N'
            writer.write_u8(b'N').await?;
            writer.flush().await?;

            // Now read the actual startup message
            let startup = StartupMessage::read(&mut reader).await?;

            // v2.0.0: Standard PostgreSQL authentication flow
            let user = startup
                .parameters
                .get("user")
                .map_or_else(|| "postgres".to_string(), std::string::ToString::to_string);
            let database_name = startup
                .parameters
                .get("database")
                .map_or_else(|| "postgres".to_string(), std::string::ToString::to_string);

            // Request password from client
            Message::authentication_cleartext_password()
                .send(&mut writer)
                .await?;

            // Read PasswordMessage
            let msg_type = reader.read_u8().await?;
            if msg_type != pg_protocol::frontend::PASSWORD {
                Message::error_response("Expected password message")
                    .send(&mut writer)
                    .await?;
                return Ok(());
            }

            let password_msg = pg_protocol::PasswordMessage::read(&mut reader).await?;

            // Authenticate
            let inst = instance.lock().await;
            if inst.authenticate(&user, &password_msg.password) {
                session.authenticate(user.clone(), database_name.clone());
                println!(
                    "✓ PostgreSQL client authenticated: user={user}, database={database_name}"
                );
            } else {
                drop(inst);
                Message::error_response("Authentication failed")
                    .send(&mut writer)
                    .await?;
                return Ok(());
            }
        } else if code == pg_protocol::PROTOCOL_VERSION {
            // This was a regular startup message, parse the rest
            let params_length = (length - 8) as usize;
            let mut params_buf = vec![0u8; params_length];
            reader.read_exact(&mut params_buf).await?;

            // Parse parameters manually
            let mut parameters = HashMap::new();
            let mut i = 0;
            while i < params_buf.len() {
                let key_start = i;
                while i < params_buf.len() && params_buf[i] != 0 {
                    i += 1;
                }
                if i >= params_buf.len() {
                    break;
                }
                let key = String::from_utf8_lossy(&params_buf[key_start..i]).to_string();
                i += 1;

                if i >= params_buf.len() {
                    break;
                }

                let value_start = i;
                while i < params_buf.len() && params_buf[i] != 0 {
                    i += 1;
                }
                let value = String::from_utf8_lossy(&params_buf[value_start..i]).to_string();
                i += 1;

                if !key.is_empty() {
                    parameters.insert(key, value);
                }
            }

            // v2.0.0: Standard PostgreSQL authentication flow
            let user = parameters
                .get("user")
                .map_or_else(|| "postgres".to_string(), std::string::ToString::to_string);
            let database_name = parameters
                .get("database")
                .map_or_else(|| "postgres".to_string(), std::string::ToString::to_string);

            // Request password from client
            Message::authentication_cleartext_password()
                .send(&mut writer)
                .await?;

            // Read PasswordMessage
            let msg_type = reader.read_u8().await?;
            if msg_type != pg_protocol::frontend::PASSWORD {
                Message::error_response("Expected password message")
                    .send(&mut writer)
                    .await?;
                return Ok(());
            }

            let password_msg = pg_protocol::PasswordMessage::read(&mut reader).await?;

            // Authenticate
            let inst = instance.lock().await;
            if inst.authenticate(&user, &password_msg.password) {
                session.authenticate(user.clone(), database_name.clone());
                println!(
                    "✓ PostgreSQL client authenticated: user={user}, database={database_name}"
                );
            } else {
                drop(inst);
                Message::error_response("Authentication failed")
                    .send(&mut writer)
                    .await?;
                return Ok(());
            }
        } else {
            return Err(format!("Unknown protocol code: {code}").into());
        }

        // Send AuthenticationOk
        Message::authentication_ok().send(&mut writer).await?;

        // Send ParameterStatus messages
        Message::parameter_status("server_version", "14.0 (PostgrustSQL)")
            .send(&mut writer)
            .await?;
        Message::parameter_status("server_encoding", "UTF8")
            .send(&mut writer)
            .await?;
        Message::parameter_status("client_encoding", "UTF8")
            .send(&mut writer)
            .await?;
        Message::parameter_status("is_superuser", "on")
            .send(&mut writer)
            .await?;
        Message::parameter_status("session_authorization", &session.username)
            .send(&mut writer)
            .await?;

        // Send ReadyForQuery
        Message::ready_for_query(transaction_status::IDLE)
            .send(&mut writer)
            .await?;

        let mut transaction = Transaction::new();

        loop {
            // Read message from client
            let (msg_type, data) = match pg_protocol::read_frontend_message(&mut reader).await {
                Ok(msg) => msg,
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            };

            match msg_type {
                frontend::QUERY => {
                    // Extract query string
                    let query = if let Some((q, _)) = pg_protocol::extract_cstring(&data) {
                        q
                    } else {
                        Message::error_response("Invalid query format")
                            .send(&mut writer)
                            .await?;
                        Message::ready_for_query(transaction_status::IDLE)
                            .send(&mut writer)
                            .await?;
                        continue;
                    };

                    let query = query.trim();
                    if query.is_empty() {
                        Message::command_complete("EMPTY").send(&mut writer).await?;
                        Message::ready_for_query(transaction_status::IDLE)
                            .send(&mut writer)
                            .await?;
                        continue;
                    }

                    // Execute query
                    match parse_statement(query) {
                        Ok(stmt) => {
                            let mut inst = instance.lock().await;

                            match stmt {
                                // User management commands
                                crate::parser::Statement::CreateUser {
                                    username,
                                    password,
                                    is_superuser,
                                } => {
                                    match inst.create_user(&username, &password, is_superuser) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("CREATE USER")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::DropUser { username } => {
                                    match inst.drop_user(&username) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("DROP USER")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::AlterUser { username, password } => {
                                    match inst.users.get_mut(&username) {
                                        Some(user) => {
                                            user.set_password(&password);
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("ALTER USER")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        None => {
                                            Message::error_response(&format!(
                                                "User '{username}' not found"
                                            ))
                                            .send(&mut writer)
                                            .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                // Role management commands
                                crate::parser::Statement::CreateRole { role_name, is_superuser } => {
                                    match inst.create_role(&role_name, is_superuser) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("CREATE ROLE")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::DropRole { role_name } => {
                                    match inst.drop_role(&role_name) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("DROP ROLE")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::GrantRole { role_name, to_user } => {
                                    match inst.grant_role_to_user(&role_name, &to_user) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("GRANT")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::RevokeRole { role_name, from_user } => {
                                    match inst.revoke_role_from_user(&role_name, &from_user) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("REVOKE")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                // Database management commands
                                crate::parser::Statement::CreateDatabase { name, owner } => {
                                    let owner = owner.unwrap_or_else(|| session.username.clone());
                                    match inst.create_database(&name, &owner) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("CREATE DATABASE")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::DropDatabase { name } => {
                                    match inst.drop_database(&name) {
                                        Ok(()) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) =
                                                storage_guard.save_server_instance(&inst)
                                            {
                                                Message::error_response(&format!(
                                                    "Failed to persist: {e}"
                                                ))
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                Message::command_complete("DROP DATABASE")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                // Privilege commands
                                crate::parser::Statement::Grant {
                                    privilege,
                                    on,
                                    to_user,
                                } => {
                                    use crate::parser::GrantObject;
                                    let priv_type = Self::convert_privilege(&privilege);

                                    let result = match on {
                                        GrantObject::Database(db_name) => {
                                            // Grant on database
                                            inst.get_database_metadata_mut(&db_name)
                                                .map(|meta| {
                                                    meta.grant(&to_user, priv_type);
                                                    format!("Granted {privilege:?} on database {db_name} to {to_user}")
                                                })
                                                .ok_or_else(|| format!("Database '{db_name}' not found"))
                                        }
                                        GrantObject::Table(table_name) => {
                                            // Grant on table (v2.3.0)
                                            inst.get_database_mut(&session.database_name)
                                                .and_then(|db| db.table_metadata.get_mut(&table_name))
                                                .map(|meta| {
                                                    meta.grant(&to_user, priv_type);
                                                    format!("Granted {privilege:?} on table {table_name} to {to_user}")
                                                })
                                                .ok_or_else(|| format!("Table '{table_name}' not found"))
                                        }
                                    };

                                    match result {
                                        Ok(_msg) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {e}"))
                                                    .send(&mut writer)
                                                    .await?;
                                            } else {
                                                Message::command_complete("GRANT")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(msg) => {
                                            Message::error_response(&msg)
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::Revoke {
                                    privilege,
                                    on,
                                    from_user,
                                } => {
                                    use crate::parser::GrantObject;
                                    let priv_type = Self::convert_privilege(&privilege);

                                    let result = match on {
                                        GrantObject::Database(db_name) => {
                                            // Revoke from database
                                            inst.get_database_metadata_mut(&db_name)
                                                .map(|meta| {
                                                    meta.revoke(&from_user, &priv_type);
                                                    format!("Revoked {privilege:?} on database {db_name} from {from_user}")
                                                })
                                                .ok_or_else(|| format!("Database '{db_name}' not found"))
                                        }
                                        GrantObject::Table(table_name) => {
                                            // Revoke from table (v2.3.0)
                                            inst.get_database_mut(&session.database_name)
                                                .and_then(|db| db.table_metadata.get_mut(&table_name))
                                                .map(|meta| {
                                                    meta.revoke(&from_user, &priv_type);
                                                    format!("Revoked {privilege:?} on table {table_name} from {from_user}")
                                                })
                                                .ok_or_else(|| format!("Table '{table_name}' not found"))
                                        }
                                    };

                                    match result {
                                        Ok(_msg) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {e}"))
                                                    .send(&mut writer)
                                                    .await?;
                                            } else {
                                                Message::command_complete("REVOKE")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(msg) => {
                                            Message::error_response(&msg)
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                // Metadata queries
                                crate::parser::Statement::ShowUsers => {
                                    let mut rows = vec![];
                                    for (username, user) in &inst.users {
                                        rows.push(vec![
                                            username.clone(),
                                            if user.is_superuser {
                                                "yes".to_string()
                                            } else {
                                                "no".to_string()
                                            },
                                            if user.can_create_db {
                                                "yes".to_string()
                                            } else {
                                                "no".to_string()
                                            },
                                        ]);
                                    }
                                    let columns = vec![
                                        "username".to_string(),
                                        "superuser".to_string(),
                                        "createdb".to_string(),
                                    ];

                                    Message::row_description(&columns).send(&mut writer).await?;
                                    for row in &rows {
                                        Message::data_row(row).send(&mut writer).await?;
                                    }
                                    Message::command_complete(&format!("SELECT {}", rows.len()))
                                        .send(&mut writer)
                                        .await?;
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                crate::parser::Statement::ShowDatabases => {
                                    let mut rows = vec![];
                                    for (name, meta) in &inst.database_metadata {
                                        rows.push(vec![name.clone(), meta.owner.clone()]);
                                    }
                                    let columns = vec!["name".to_string(), "owner".to_string()];

                                    Message::row_description(&columns).send(&mut writer).await?;
                                    for row in &rows {
                                        Message::data_row(row).send(&mut writer).await?;
                                    }
                                    Message::command_complete(&format!("SELECT {}", rows.len()))
                                        .send(&mut writer)
                                        .await?;
                                    Message::ready_for_query(transaction_status::IDLE)
                                        .send(&mut writer)
                                        .await?;
                                }
                                // Regular table operations need database access
                                other_stmt => {
                                    // v2.3.0: First transform CREATE TABLE to add owner before permission check
                                    let stmt_with_owner_early = match other_stmt {
                                        crate::parser::Statement::CreateTable { name, columns, owner: None } => {
                                            crate::parser::Statement::CreateTable {
                                                name,
                                                columns,
                                                owner: Some(session.username.clone()),
                                            }
                                        }
                                        other => other,
                                    };

                                    // v2.3.0: Check permissions BEFORE getting mutable database reference
                                    // This avoids borrow checker issues
                                    let needs_permission_check = matches!(
                                        stmt_with_owner_early,
                                        crate::parser::Statement::Select { .. }
                                            | crate::parser::Statement::Insert { .. }
                                            | crate::parser::Statement::Update { .. }
                                            | crate::parser::Statement::Delete { .. }
                                            | crate::parser::Statement::AlterTable { .. }
                                            | crate::parser::Statement::DropTable { .. }
                                    );

                                    if needs_permission_check {
                                        if let Some(err_msg) = Self::check_statement_permissions(
                                            &inst,
                                            &session.database_name,
                                            &session.username,
                                            &stmt_with_owner_early,
                                        ) {
                                            Message::error_response(&err_msg)
                                                .send(&mut writer)
                                                .await?;
                                            Message::ready_for_query(transaction_status::IDLE)
                                                .send(&mut writer)
                                                .await?;
                                            continue;
                                        }
                                    }

                                    // Получаем текущую БД из сессии
                                    let db = if let Some(db) =
                                        inst.get_database_mut(&session.database_name)
                                    {
                                        db
                                    } else {
                                        Message::error_response(&format!(
                                            "Database '{}' not found",
                                            session.database_name
                                        ))
                                        .send(&mut writer)
                                        .await?;
                                        Message::ready_for_query(transaction_status::IDLE)
                                            .send(&mut writer)
                                            .await?;
                                        continue;
                                    };

                                    match stmt_with_owner_early {
                                        crate::parser::Statement::Begin => {
                                            if transaction.is_active() {
                                                Message::error_response(
                                                    "Transaction already active",
                                                )
                                                .send(&mut writer)
                                                .await?;
                                            } else {
                                                let (tx_id, snapshot) =
                                                    tx_manager.begin_transaction();
                                                transaction.begin(tx_id, snapshot, db);
                                                Message::command_complete("BEGIN")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                            Message::ready_for_query(
                                                transaction_status::IN_TRANSACTION,
                                            )
                                            .send(&mut writer)
                                            .await?;
                                        }
                                        crate::parser::Statement::Commit => {
                                            if transaction.is_active() {
                                                // Remove from active transactions in GlobalTransactionManager
                                                if let Some(tx_id) = transaction.tx_id() {
                                                    tx_manager.commit_transaction(tx_id);
                                                }
                                                transaction.commit();
                                                let mut storage_guard = storage.lock().await;
                                                if let Err(e) =
                                                    storage_guard.save_server_instance(&inst)
                                                {
                                                    Message::error_response(&format!(
                                                        "Failed to persist: {e}"
                                                    ))
                                                    .send(&mut writer)
                                                    .await?;
                                                } else {
                                                    Message::command_complete("COMMIT")
                                                        .send(&mut writer)
                                                        .await?;
                                                }
                                            } else {
                                                Message::error_response("No active transaction")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                            Message::ready_for_query(transaction_status::IDLE)
                                                .send(&mut writer)
                                                .await?;
                                        }
                                        crate::parser::Statement::Rollback => {
                                            if transaction.is_active() {
                                                // Remove from active transactions in GlobalTransactionManager
                                                if let Some(tx_id) = transaction.tx_id() {
                                                    tx_manager.rollback_transaction(tx_id);
                                                }
                                                transaction.rollback(db);
                                                Message::command_complete("ROLLBACK")
                                                    .send(&mut writer)
                                                    .await?;
                                            } else {
                                                Message::error_response("No active transaction")
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                            Message::ready_for_query(transaction_status::IDLE)
                                                .send(&mut writer)
                                                .await?;
                                        }
                                        _ => {
                                            let mut storage_guard = storage.lock().await;
                                            let storage_option = if transaction.is_active() {
                                                None
                                            } else {
                                                Some(&mut *storage_guard)
                                            };

                                            // v2.0.0: database_storage is now required
                                            let db_storage = database_storage
                                                .as_ref()
                                                .expect("v2.0.0: database_storage is required");
                                            let mut db_storage_guard = db_storage.lock().await;

                                            // Permission checks already done earlier
                                            match QueryExecutor::execute(
                                                db,
                                                stmt_with_owner_early,
                                                storage_option,
                                                &tx_manager,
                                                &mut db_storage_guard,
                                                transaction.tx_id(),
                                            ) {
                                                Ok(result) => {
                                                    if transaction.is_active() {
                                                        Self::send_postgres_result(
                                                            result,
                                                            &mut writer,
                                                        )
                                                        .await?;
                                                    } else if let Err(e) =
                                                        storage_guard.save_server_instance(&inst)
                                                    {
                                                        Message::error_response(&format!(
                                                            "Checkpoint failed: {e}"
                                                        ))
                                                        .send(&mut writer)
                                                        .await?;
                                                    } else {
                                                        Self::send_postgres_result(
                                                            result,
                                                            &mut writer,
                                                        )
                                                        .await?;
                                                    }

                                                    let status = if transaction.is_active() {
                                                        transaction_status::IN_TRANSACTION
                                                    } else {
                                                        transaction_status::IDLE
                                                    };
                                                    Message::ready_for_query(status)
                                                        .send(&mut writer)
                                                        .await?;
                                                }
                                                Err(e) => {
                                                    Message::error_response(&format!("{e}"))
                                                        .send(&mut writer)
                                                        .await?;
                                                    let status = if transaction.is_active() {
                                                        transaction_status::IN_TRANSACTION
                                                    } else {
                                                        transaction_status::IDLE
                                                    };
                                                    Message::ready_for_query(status)
                                                        .send(&mut writer)
                                                        .await?;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            Message::error_response(&format!("Parse error: {e}"))
                                .send(&mut writer)
                                .await?;
                            let status = if transaction.is_active() {
                                transaction_status::IN_TRANSACTION
                            } else {
                                transaction_status::IDLE
                            };
                            Message::ready_for_query(status).send(&mut writer).await?;
                        }
                    }
                }
                // Extended Query Protocol (v2.4.0)
                frontend::PARSE => {
                    match pg_protocol::ParseMessage::from_data(&data) {
                        Ok(parse_msg) => {
                            // Store the prepared statement
                            session.prepared_statements.add_statement(
                                parse_msg.statement_name.clone(),
                                parse_msg.query.clone(),
                                parse_msg.param_types.clone(),
                            );

                            // Try to parse the statement now for validation
                            if !parse_msg.query.is_empty() {
                                if let Ok(stmt) = parse_statement(&parse_msg.query) {
                                    if let Some(prep_stmt) = session.prepared_statements.get_statement_mut(&parse_msg.statement_name) {
                                        prep_stmt.statement = Some(stmt);
                                    }
                                }
                            }

                            // Send ParseComplete
                            Message::parse_complete().send(&mut writer).await?;
                        }
                        Err(e) => {
                            Message::error_response(&format!("Parse error: {e}"))
                                .send(&mut writer)
                                .await?;
                        }
                    }
                }
                frontend::BIND => {
                    match pg_protocol::BindMessage::from_data(&data) {
                        Ok(bind_msg) => {
                            // Convert binary parameter values to Value enum
                            let mut param_values = Vec::new();
                            for param_bytes in &bind_msg.param_values {
                                match param_bytes {
                                    None => param_values.push(None),
                                    Some(bytes) => {
                                        // Simple text format parsing - convert bytes to string
                                        let value_str = String::from_utf8_lossy(bytes);
                                        // For simplicity, store as Text (proper type inference would be more complex)
                                        param_values.push(Some(Value::Text(value_str.to_string())));
                                    }
                                }
                            }

                            // Store the portal
                            session.prepared_statements.add_portal(
                                bind_msg.portal_name.clone(),
                                bind_msg.statement_name.clone(),
                                param_values,
                            );

                            // Send BindComplete
                            Message::bind_complete().send(&mut writer).await?;
                        }
                        Err(e) => {
                            Message::error_response(&format!("Bind error: {e}"))
                                .send(&mut writer)
                                .await?;
                        }
                    }
                }
                frontend::DESCRIBE => {
                    match pg_protocol::DescribeMessage::from_data(&data) {
                        Ok(_desc_msg) => {
                            // For now, we don't provide detailed column descriptions
                            // Just send NoData (statement has no result columns)
                            Message::no_data().send(&mut writer).await?;
                        }
                        Err(e) => {
                            Message::error_response(&format!("Describe error: {e}"))
                                .send(&mut writer)
                                .await?;
                        }
                    }
                }
                frontend::EXECUTE => {
                    match pg_protocol::ExecuteMessage::from_data(&data) {
                        Ok(exec_msg) => {
                            // Get the portal
                            let portal = session.prepared_statements.get_portal(&exec_msg.portal_name).cloned();

                            if let Some(portal) = portal {
                                // Get the prepared statement
                                let prep_stmt = session.prepared_statements.get_statement(&portal.statement_name).cloned();

                                if let Some(prep_stmt) = prep_stmt {
                                    // Substitute parameters in the query
                                    let query = substitute_parameters(&prep_stmt.query, &portal.param_values);

                                    // Execute the query (similar to QUERY handling)
                                    match parse_statement(&query) {
                                        Ok(stmt) => {
                                            let mut inst = instance.lock().await;
                                            let db = inst.get_database_mut(&session.database_name);

                                            if let Some(db) = db {
                                                let db_storage = database_storage
                                                    .as_ref()
                                                    .expect("v2.0.0: database_storage is required");
                                                let mut db_storage_guard = db_storage.lock().await;
                                                let mut storage_guard = storage.lock().await;

                                                match QueryExecutor::execute(
                                                    db,
                                                    stmt,
                                                    Some(&mut *storage_guard),
                                                    &tx_manager,
                                                    &mut db_storage_guard,
                                                    transaction.tx_id(),
                                                ) {
                                                    Ok(result) => {
                                                        Self::send_postgres_result(result, &mut writer).await?;
                                                    }
                                                    Err(e) => {
                                                        Message::error_response(&format!("{e}"))
                                                            .send(&mut writer)
                                                            .await?;
                                                    }
                                                }
                                            } else {
                                                Message::error_response(&format!("Database '{}' not found", session.database_name))
                                                    .send(&mut writer)
                                                    .await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{e}"))
                                                .send(&mut writer)
                                                .await?;
                                        }
                                    }
                                } else {
                                    Message::error_response(&format!("Prepared statement '{}' not found", portal.statement_name))
                                        .send(&mut writer)
                                        .await?;
                                }
                            } else {
                                Message::error_response(&format!("Portal '{}' not found", exec_msg.portal_name))
                                    .send(&mut writer)
                                    .await?;
                            }
                        }
                        Err(e) => {
                            Message::error_response(&format!("Execute error: {e}"))
                                .send(&mut writer)
                                .await?;
                        }
                    }
                }
                frontend::CLOSE => {
                    match pg_protocol::CloseMessage::from_data(&data) {
                        Ok(close_msg) => {
                            let success = if close_msg.close_type == 'S' {
                                // Close statement
                                session.prepared_statements.remove_statement(&close_msg.name)
                            } else {
                                // Close portal
                                session.prepared_statements.remove_portal(&close_msg.name)
                            };

                            if success {
                                Message::close_complete().send(&mut writer).await?;
                            } else {
                                Message::error_response(&format!("{} '{}' not found",
                                    if close_msg.close_type == 'S' { "Statement" } else { "Portal" },
                                    close_msg.name))
                                    .send(&mut writer)
                                    .await?;
                            }
                        }
                        Err(e) => {
                            Message::error_response(&format!("Close error: {e}"))
                                .send(&mut writer)
                                .await?;
                        }
                    }
                }
                frontend::SYNC => {
                    // Send ReadyForQuery
                    let tx_status = if transaction.is_active() {
                        transaction_status::IN_TRANSACTION
                    } else {
                        transaction_status::IDLE
                    };
                    Message::ready_for_query(tx_status).send(&mut writer).await?;
                }
                frontend::TERMINATE => {
                    break;
                }
                _ => {
                    Message::error_response(&format!("Unknown message type: {msg_type}"))
                        .send(&mut writer)
                        .await?;
                    Message::ready_for_query(transaction_status::IDLE)
                        .send(&mut writer)
                        .await?;
                }
            }
        }

        Ok(())
    }

    async fn send_postgres_result<W: AsyncWriteExt + Unpin>(
        result: QueryResult,
        writer: &mut W,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match result {
            QueryResult::Success(msg) => {
                // For non-SELECT queries, send CommandComplete
                Message::command_complete(&msg).send(writer).await?;
            }
            QueryResult::Rows(rows, columns) => {
                // Send RowDescription
                Message::row_description(&columns).send(writer).await?;

                // Send DataRow for each row
                for row in &rows {
                    Message::data_row(row).send(writer).await?;
                }

                // Send CommandComplete with row count
                let tag = format!("SELECT {}", rows.len());
                Message::command_complete(&tag).send(writer).await?;
            }
        }
        Ok(())
    }

    async fn handle_text_client(
        mut socket: TcpStream,
        instance: Arc<Mutex<ServerInstance>>,
        storage: Arc<Mutex<StorageEngine>>,
        tx_manager: GlobalTransactionManager,
        database_storage: Option<Arc<Mutex<crate::storage::DatabaseStorage>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (reader, mut writer) = socket.split();
        let mut reader = BufReader::new(reader);

        // Text protocol: простая аутентификация через первые команды или использование дефолтного пользователя
        let mut session = SessionContext::new();
        session.authenticate("postgres".to_string(), "postgres".to_string());

        writer
            .write_all(b"Welcome to PostgrustSQL!\nType your SQL queries (end with semicolon)\nSupports: BEGIN, COMMIT, ROLLBACK for transactions\n")
            .await?;
        writer.write_all(b"postgrustql>\n").await?;
        writer.flush().await?;

        let mut line = String::new();
        let mut transaction = Transaction::new();

        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;

            if n == 0 {
                break;
            }

            let query = line.trim();

            if query.is_empty() {
                writer.write_all(b"postgrustql>\n").await?;
                writer.flush().await?;
                continue;
            }

            if query.eq_ignore_ascii_case("quit") || query.eq_ignore_ascii_case("exit") {
                writer.write_all(b"Goodbye!\n").await?;
                break;
            }

            // Execute query
            let response = match parse_statement(query) {
                Ok(stmt) => {
                    let mut inst = instance.lock().await;

                    // Проверяем, существует ли БД
                    if inst.databases.contains_key(&session.database_name) {
                        // Получаем мутабельную ссылку на БД
                        let db = inst.get_database_mut(&session.database_name).unwrap();

                        match stmt {
                            // User management commands (v2.2.2)
                            crate::parser::Statement::CreateUser {
                                username,
                                password,
                                is_superuser,
                            } => {
                                match inst.create_user(&username, &password, is_superuser) {
                                    Ok(()) => {
                                        let mut storage_guard = storage.lock().await;
                                        if let Err(e) = storage_guard.save_server_instance(&inst) {
                                            format!("Error: Failed to persist user: {e}\n")
                                        } else {
                                            "CREATE USER\n".to_string()
                                        }
                                    }
                                    Err(e) => format!("Error: {e}\n"),
                                }
                            }
                            crate::parser::Statement::DropUser { username } => {
                                match inst.drop_user(&username) {
                                    Ok(()) => {
                                        let mut storage_guard = storage.lock().await;
                                        if let Err(e) = storage_guard.save_server_instance(&inst) {
                                            format!("Error: Failed to persist: {e}\n")
                                        } else {
                                            "DROP USER\n".to_string()
                                        }
                                    }
                                    Err(e) => format!("Error: {e}\n"),
                                }
                            }
                            crate::parser::Statement::AlterUser { username, password } => {
                                match inst.users.get_mut(&username) {
                                    Some(user) => {
                                        user.set_password(&password);
                                        let mut storage_guard = storage.lock().await;
                                        if let Err(e) = storage_guard.save_server_instance(&inst) {
                                            format!("Error: Failed to persist: {e}\n")
                                        } else {
                                            "ALTER USER\n".to_string()
                                        }
                                    }
                                    None => format!("Error: User '{}' not found\n", username),
                                }
                            }
                            crate::parser::Statement::Begin => {
                                if transaction.is_active() {
                                    "Warning: Transaction already active\n".to_string()
                                } else {
                                    let (tx_id, snapshot) = tx_manager.begin_transaction();
                                    transaction.begin(tx_id, snapshot, db);
                                    format!("Transaction started (ID: {tx_id})\n")
                                }
                            }
                            crate::parser::Statement::Commit => {
                                if transaction.is_active() {
                                    // Remove from active transactions in GlobalTransactionManager
                                    if let Some(tx_id) = transaction.tx_id() {
                                        tx_manager.commit_transaction(tx_id);
                                    }
                                    transaction.commit();
                                    // Save server instance after commit
                                    let mut storage_guard = storage.lock().await;
                                    if let Err(e) = storage_guard.save_server_instance(&inst) {
                                        format!("Warning: Failed to persist changes: {e}\n")
                                    } else {
                                        "Transaction committed\n".to_string()
                                    }
                                } else {
                                    "Error: No active transaction\n".to_string()
                                }
                            }
                            crate::parser::Statement::Rollback => {
                                if transaction.is_active() {
                                    // Remove from active transactions in GlobalTransactionManager
                                    if let Some(tx_id) = transaction.tx_id() {
                                        tx_manager.rollback_transaction(tx_id);
                                    }
                                    transaction.rollback(db);
                                    "Transaction rolled back\n".to_string()
                                } else {
                                    "Error: No active transaction\n".to_string()
                                }
                            }
                            other_stmt => {
                                // Get storage lock for WAL logging and checkpointing
                                let mut storage_guard = storage.lock().await;

                                // Execute with WAL logging (only if not in transaction)
                                let storage_option = if transaction.is_active() {
                                    None
                                } else {
                                    Some(&mut *storage_guard)
                                };

                                // v2.0.0: database_storage is now required
                                let db_storage = database_storage
                                    .as_ref()
                                    .expect("v2.0.0: database_storage is required");
                                let mut db_storage_guard = db_storage.lock().await;

                                match QueryExecutor::execute(
                                    db,
                                    other_stmt,
                                    storage_option,
                                    &tx_manager,
                                    &mut db_storage_guard,
                                    transaction.tx_id(),
                                ) {
                                    Ok(result) => {
                                        // Checkpoint if needed (only if not in transaction)
                                        if transaction.is_active() {
                                            Self::format_result(result)
                                        } else if let Err(e) =
                                            storage_guard.save_server_instance(&inst)
                                        {
                                            format!("Warning: Failed to checkpoint: {e}\n")
                                        } else {
                                            Self::format_result(result)
                                        }
                                    }
                                    Err(e) => format!("Error: {e}\n"),
                                }
                            }
                        }
                    } else {
                        format!("Error: Database '{}' not found\n", session.database_name)
                    }
                }
                Err(e) => format!("Parse error: {e}\n"),
            };

            writer.write_all(response.as_bytes()).await?;
            writer.write_all(b"postgrustql>\n").await?;
            writer.flush().await?;
        }

        Ok(())
    }

    fn format_result(result: QueryResult) -> String {
        match result {
            QueryResult::Success(msg) => format!("{msg}\n"),
            QueryResult::Rows(rows, columns) => {
                if rows.is_empty() {
                    return "(0 rows)\n".to_string();
                }

                let mut table = ComfyTable::new();
                table.load_preset(UTF8_FULL);

                // Add header
                table.set_header(columns.iter().map(Cell::new));

                // Add rows
                for row in rows {
                    table.add_row(row.iter().map(Cell::new));
                }

                format!("{}\n({} rows)\n", table, table.row_iter().count() - 1)
            }
        }
    }

    const fn convert_privilege(
        priv_type: &crate::parser::PrivilegeType,
    ) -> crate::types::Privilege {
        match priv_type {
            crate::parser::PrivilegeType::Connect => crate::types::Privilege::Connect,
            crate::parser::PrivilegeType::Create => crate::types::Privilege::Create,
            crate::parser::PrivilegeType::Select => crate::types::Privilege::Select,
            crate::parser::PrivilegeType::Insert => crate::types::Privilege::Insert,
            crate::parser::PrivilegeType::Update => crate::types::Privilege::Update,
            crate::parser::PrivilegeType::Delete => crate::types::Privilege::Delete,
            crate::parser::PrivilegeType::All => crate::types::Privilege::All,
        }
    }

    /// v2.3.0: Check permissions for a statement before execution
    ///
    /// Returns None if permission is granted, Some(error_message) if denied
    fn check_statement_permissions(
        instance: &ServerInstance,
        db_name: &str,
        username: &str,
        stmt: &crate::parser::Statement,
    ) -> Option<String> {
        use crate::parser::Statement;
        use crate::types::Privilege;

        match stmt {
            // SELECT - check SELECT privilege
            Statement::Select { from, .. } => {
                if !instance.check_table_permission(username, db_name, from, &Privilege::Select) {
                    return Some(format!(
                        "Permission denied: User '{}' does not have SELECT privilege on table '{}'",
                        username, from
                    ));
                }
            }

            // INSERT - check INSERT privilege
            Statement::Insert { table, .. } => {
                if !instance.check_table_permission(username, db_name, table, &Privilege::Insert) {
                    return Some(format!(
                        "Permission denied: User '{}' does not have INSERT privilege on table '{}'",
                        username, table
                    ));
                }
            }

            // UPDATE - check UPDATE privilege
            Statement::Update { table, .. } => {
                if !instance.check_table_permission(username, db_name, table, &Privilege::Update) {
                    return Some(format!(
                        "Permission denied: User '{}' does not have UPDATE privilege on table '{}'",
                        username, table
                    ));
                }
            }

            // DELETE - check DELETE privilege
            Statement::Delete { from, .. } => {
                if !instance.check_table_permission(username, db_name, from, &Privilege::Delete) {
                    return Some(format!(
                        "Permission denied: User '{}' does not have DELETE privilege on table '{}'",
                        username, from
                    ));
                }
            }

            // ALTER TABLE - check owner or superuser
            Statement::AlterTable { name, .. } => {
                if !instance.is_table_owner_or_superuser(username, db_name, name) {
                    return Some(format!(
                        "Permission denied: User '{}' must be table owner or superuser to ALTER TABLE '{}'",
                        username, name
                    ));
                }
            }

            // DROP TABLE - check owner or superuser
            Statement::DropTable { name } => {
                if !instance.is_table_owner_or_superuser(username, db_name, name) {
                    return Some(format!(
                        "Permission denied: User '{}' must be table owner or superuser to DROP TABLE '{}'",
                        username, name
                    ));
                }
            }

            // Other statements - no table-level permissions required
            _ => {}
        }

        None // Permission granted
    }
}
