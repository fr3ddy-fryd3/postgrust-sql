use crate::executor::{QueryExecutor, QueryResult};
use crate::parser::parse_statement;
use crate::network::pg_protocol::{self, frontend, transaction_status, Message, StartupMessage};
use crate::storage::StorageEngine;
use crate::transaction::{Transaction, TransactionManager};
use crate::types::{DatabaseError, ServerInstance};
use comfy_table::{Cell, Table as ComfyTable, presets::UTF8_FULL};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

/// Контекст сессии пользователя
#[derive(Clone)]
struct SessionContext {
    username: String,
    database_name: String,
    is_authenticated: bool,
}

impl SessionContext {
    fn new() -> Self {
        Self {
            username: String::new(),
            database_name: String::new(),
            is_authenticated: false,
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
    tx_manager: TransactionManager,
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
                        println!("  - Creating superuser: {}", superuser);
                        existing.users.insert(
                            superuser.to_string(),
                            crate::types::User::new(superuser.to_string(), password, true),
                        );
                    }

                    // Проверяем, есть ли начальная БД
                    if !existing.databases.contains_key(initial_db) {
                        println!("  - Creating initial database: {}", initial_db);
                        existing.create_database(initial_db, superuser)?;
                    }

                    existing
                }
                _ => {
                    // Создаем новый
                    println!("✓ Initializing new server instance");
                    println!("  - Superuser: {}", superuser);
                    println!("  - Initial database: {}", initial_db);
                    ServerInstance::initialize(superuser, password, initial_db)
                }
            }
        } else {
            // Только загружаем существующий
            storage.load_server_instance()?
        };

        // Сохраняем начальный snapshot
        storage.create_checkpoint_instance(&instance)?;

        let tx_manager = TransactionManager::new();

        // Check if page-based storage should be used (runtime selection via env var)
        let use_page_storage = std::env::var("RUSTDB_USE_PAGE_STORAGE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let database_storage = if use_page_storage {
            const BUFFER_POOL_SIZE: usize = 1000;  // 1000 pages * 8KB = 8MB cache
            match crate::storage::DatabaseStorage::new(data_dir, BUFFER_POOL_SIZE) {
                Ok(db_storage) => {
                    println!("✓ Page-based storage enabled (8MB buffer pool)");
                    Some(Arc::new(Mutex::new(db_storage)))
                }
                Err(e) => {
                    eprintln!("✗ Failed to initialize page storage: {}", e);
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
        println!("
╔══════════════════════════════════════════════════════════╗
║       🚀 PostgrustSQL Server is Ready!                  ║
║                                                          ║
║  Listening on: {:<41} ║
╚══════════════════════════════════════════════════════════╝
", addr);

        loop {
            let (socket, addr) = listener.accept().await?;
            println!("→ New connection from {}", addr);

            let instance = Arc::clone(&self.instance);
            let storage = Arc::clone(&self.storage);
            let tx_manager = self.tx_manager.clone();
            let database_storage = self.database_storage.as_ref().map(Arc::clone);

            tokio::spawn(async move {
                if let Err(e) = Self::handle_client_auto(socket, instance, storage, tx_manager, database_storage).await {
                    eprintln!("✗ Error handling client {}: {}", addr, e);
                }
            });
        }
    }

    async fn handle_client_auto(
        mut socket: TcpStream,
        instance: Arc<Mutex<ServerInstance>>,
        storage: Arc<Mutex<StorageEngine>>,
        tx_manager: TransactionManager,
        database_storage: Option<Arc<Mutex<crate::storage::DatabaseStorage>>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Peek at the first 8 bytes to determine protocol
        let mut peek_buf = [0u8; 8];
        socket.peek(&mut peek_buf).await?;

        // PostgreSQL protocol starts with Int32 length followed by Int32 code
        // Code can be:
        // - Protocol version 3.0: 196608 (0x00030000)
        // - SSL request: 80877103 (0x04D2162F)
        // Text protocol starts with ASCII text
        let length = i32::from_be_bytes([peek_buf[0], peek_buf[1], peek_buf[2], peek_buf[3]]);
        let code = i32::from_be_bytes([peek_buf[4], peek_buf[5], peek_buf[6], peek_buf[7]]);

        // If length is reasonable (< 10000) and code matches PostgreSQL protocol or SSL request
        if length > 0 && length < 10000 &&
           (code == pg_protocol::PROTOCOL_VERSION || code == pg_protocol::SSL_REQUEST_CODE) {
            Self::handle_postgres_client(socket, instance, storage, tx_manager, database_storage).await
        } else {
            Self::handle_text_client(socket, instance, storage, tx_manager, database_storage).await
        }
    }

    async fn handle_postgres_client(
        socket: TcpStream,
        instance: Arc<Mutex<ServerInstance>>,
        storage: Arc<Mutex<StorageEngine>>,
        tx_manager: TransactionManager,
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

            // Continue with normal flow
            let user = startup.parameters.get("user").map(|s| s.to_string()).unwrap_or_else(|| "postgres".to_string());
            let password = startup.parameters.get("password").map(|s| s.to_string()).unwrap_or_else(|| "postgres".to_string());
            let database_name = startup.parameters.get("database").map(|s| s.to_string()).unwrap_or_else(|| "postgres".to_string());

            // Authenticate
            let inst = instance.lock().await;
            if inst.authenticate(&user, &password) {
                session.authenticate(user.clone(), database_name.clone());
                println!("✓ PostgreSQL client authenticated: user={}, database={}", user, database_name);
            } else {
                drop(inst);
                Message::error_response("Authentication failed").send(&mut writer).await?;
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

            let user = parameters.get("user").map(|s| s.to_string()).unwrap_or_else(|| "postgres".to_string());
            let password = parameters.get("password").map(|s| s.to_string()).unwrap_or_else(|| "postgres".to_string());
            let database_name = parameters.get("database").map(|s| s.to_string()).unwrap_or_else(|| "postgres".to_string());

            // Authenticate
            let inst = instance.lock().await;
            if inst.authenticate(&user, &password) {
                session.authenticate(user.clone(), database_name.clone());
                println!("✓ PostgreSQL client authenticated: user={}, database={}", user, database_name);
            } else {
                drop(inst);
                Message::error_response("Authentication failed").send(&mut writer).await?;
                return Ok(());
            }
        } else {
            return Err(format!("Unknown protocol code: {}", code).into());
        }

        // Send AuthenticationOk
        Message::authentication_ok().send(&mut writer).await?;

        // Send ParameterStatus messages
        Message::parameter_status("server_version", "14.0 (PostgrustSQL)").send(&mut writer).await?;
        Message::parameter_status("server_encoding", "UTF8").send(&mut writer).await?;
        Message::parameter_status("client_encoding", "UTF8").send(&mut writer).await?;
        Message::parameter_status("is_superuser", "on").send(&mut writer).await?;
        Message::parameter_status("session_authorization", &session.username).send(&mut writer).await?;

        // Send ReadyForQuery
        Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;

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
                    let query = match pg_protocol::extract_cstring(&data) {
                        Some((q, _)) => q,
                        None => {
                            Message::error_response("Invalid query format").send(&mut writer).await?;
                            Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                            continue;
                        }
                    };

                    let query = query.trim();
                    if query.is_empty() {
                        Message::command_complete("EMPTY").send(&mut writer).await?;
                        Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                        continue;
                    }

                    // Execute query
                    match parse_statement(&query) {
                        Ok(stmt) => {
                            let mut inst = instance.lock().await;

                            match stmt {
                                // User management commands
                                crate::parser::Statement::CreateUser { username, password, is_superuser } => {
                                    match inst.create_user(&username, &password, is_superuser) {
                                        Ok(_) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                            } else {
                                                Message::command_complete("CREATE USER").send(&mut writer).await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{}", e)).send(&mut writer).await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                crate::parser::Statement::DropUser { username } => {
                                    match inst.drop_user(&username) {
                                        Ok(_) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                            } else {
                                                Message::command_complete("DROP USER").send(&mut writer).await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{}", e)).send(&mut writer).await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                crate::parser::Statement::AlterUser { username, password } => {
                                    match inst.users.get_mut(&username) {
                                        Some(user) => {
                                            user.set_password(&password);
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                            } else {
                                                Message::command_complete("ALTER USER").send(&mut writer).await?;
                                            }
                                        }
                                        None => {
                                            Message::error_response(&format!("User '{}' not found", username)).send(&mut writer).await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                // Database management commands
                                crate::parser::Statement::CreateDatabase { name, owner } => {
                                    let owner = owner.unwrap_or_else(|| session.username.clone());
                                    match inst.create_database(&name, &owner) {
                                        Ok(_) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                            } else {
                                                Message::command_complete("CREATE DATABASE").send(&mut writer).await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{}", e)).send(&mut writer).await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                crate::parser::Statement::DropDatabase { name } => {
                                    match inst.drop_database(&name) {
                                        Ok(_) => {
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                            } else {
                                                Message::command_complete("DROP DATABASE").send(&mut writer).await?;
                                            }
                                        }
                                        Err(e) => {
                                            Message::error_response(&format!("{}", e)).send(&mut writer).await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                // Privilege commands
                                crate::parser::Statement::Grant { privilege, on_database, to_user } => {
                                    let priv_type = Self::convert_privilege(&privilege);
                                    match inst.get_database_metadata_mut(&on_database) {
                                        Some(meta) => {
                                            meta.grant(&to_user, priv_type);
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                            } else {
                                                Message::command_complete("GRANT").send(&mut writer).await?;
                                            }
                                        }
                                        None => {
                                            Message::error_response(&format!("Database '{}' not found", on_database)).send(&mut writer).await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                crate::parser::Statement::Revoke { privilege, on_database, from_user } => {
                                    let priv_type = Self::convert_privilege(&privilege);
                                    match inst.get_database_metadata_mut(&on_database) {
                                        Some(meta) => {
                                            meta.revoke(&from_user, &priv_type);
                                            let mut storage_guard = storage.lock().await;
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                            } else {
                                                Message::command_complete("REVOKE").send(&mut writer).await?;
                                            }
                                        }
                                        None => {
                                            Message::error_response(&format!("Database '{}' not found", on_database)).send(&mut writer).await?;
                                        }
                                    }
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                // Metadata queries
                                crate::parser::Statement::ShowUsers => {
                                    let mut rows = vec![];
                                    for (username, user) in &inst.users {
                                        rows.push(vec![
                                            username.clone(),
                                            if user.is_superuser { "yes".to_string() } else { "no".to_string() },
                                            if user.can_create_db { "yes".to_string() } else { "no".to_string() },
                                        ]);
                                    }
                                    let columns = vec!["username".to_string(), "superuser".to_string(), "createdb".to_string()];

                                    Message::row_description(&columns).send(&mut writer).await?;
                                    for row in &rows {
                                        Message::data_row(row).send(&mut writer).await?;
                                    }
                                    Message::command_complete(&format!("SELECT {}", rows.len())).send(&mut writer).await?;
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                crate::parser::Statement::ShowDatabases => {
                                    let mut rows = vec![];
                                    for (name, meta) in &inst.database_metadata {
                                        rows.push(vec![
                                            name.clone(),
                                            meta.owner.clone(),
                                        ]);
                                    }
                                    let columns = vec!["name".to_string(), "owner".to_string()];

                                    Message::row_description(&columns).send(&mut writer).await?;
                                    for row in &rows {
                                        Message::data_row(row).send(&mut writer).await?;
                                    }
                                    Message::command_complete(&format!("SELECT {}", rows.len())).send(&mut writer).await?;
                                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                }
                                // Regular table operations need database access
                                other_stmt => {
                                    // Получаем текущую БД из сессии
                                    let db = match inst.get_database_mut(&session.database_name) {
                                        Some(db) => db,
                                        None => {
                                            Message::error_response(&format!("Database '{}' not found", session.database_name)).send(&mut writer).await?;
                                            Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                            continue;
                                        }
                                    };

                                    match other_stmt {
                                        crate::parser::Statement::Begin => {
                                            if transaction.is_active() {
                                                Message::error_response("Transaction already active").send(&mut writer).await?;
                                            } else {
                                                let tx_id = tx_manager.begin_transaction();
                                                transaction.begin(tx_id, db);
                                                Message::command_complete("BEGIN").send(&mut writer).await?;
                                            }
                                            Message::ready_for_query(transaction_status::IN_TRANSACTION).send(&mut writer).await?;
                                        }
                                        crate::parser::Statement::Commit => {
                                            if !transaction.is_active() {
                                                Message::error_response("No active transaction").send(&mut writer).await?;
                                            } else {
                                                transaction.commit();
                                                let mut storage_guard = storage.lock().await;
                                                if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                    Message::error_response(&format!("Failed to persist: {}", e)).send(&mut writer).await?;
                                                } else {
                                                    Message::command_complete("COMMIT").send(&mut writer).await?;
                                                }
                                            }
                                            Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                        }
                                        crate::parser::Statement::Rollback => {
                                            if !transaction.is_active() {
                                                Message::error_response("No active transaction").send(&mut writer).await?;
                                            } else {
                                                transaction.rollback(db);
                                                Message::command_complete("ROLLBACK").send(&mut writer).await?;
                                            }
                                            Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
                                        }
                                        _ => {
                                            let mut storage_guard = storage.lock().await;
                                            let storage_option = if !transaction.is_active() {
                                                Some(&mut *storage_guard)
                                            } else {
                                                None
                                            };

                                            // Get database_storage if available
                                            let mut db_storage_guard = if let Some(ref db_storage) = database_storage {
                                                Some(db_storage.lock().await)
                                            } else {
                                                None
                                            };
                                            let db_storage_option = db_storage_guard.as_deref_mut();

                                            match QueryExecutor::execute(db, other_stmt, storage_option, &tx_manager, db_storage_option) {
                                                Ok(result) => {
                                                    if !transaction.is_active() {
                                                        if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                            Message::error_response(&format!("Checkpoint failed: {}", e)).send(&mut writer).await?;
                                                        } else {
                                                            Self::send_postgres_result(result, &mut writer).await?;
                                                        }
                                                    } else {
                                                        Self::send_postgres_result(result, &mut writer).await?;
                                                    }

                                                    let status = if transaction.is_active() {
                                                        transaction_status::IN_TRANSACTION
                                                    } else {
                                                        transaction_status::IDLE
                                                    };
                                                    Message::ready_for_query(status).send(&mut writer).await?;
                                                }
                                                Err(e) => {
                                                    Message::error_response(&format!("{}", e)).send(&mut writer).await?;
                                                    let status = if transaction.is_active() {
                                                        transaction_status::IN_TRANSACTION
                                                    } else {
                                                        transaction_status::IDLE
                                                    };
                                                    Message::ready_for_query(status).send(&mut writer).await?;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            Message::error_response(&format!("Parse error: {}", e)).send(&mut writer).await?;
                            let status = if transaction.is_active() {
                                transaction_status::IN_TRANSACTION
                            } else {
                                transaction_status::IDLE
                            };
                            Message::ready_for_query(status).send(&mut writer).await?;
                        }
                    }
                }
                frontend::TERMINATE => {
                    break;
                }
                _ => {
                    Message::error_response(&format!("Unknown message type: {}", msg_type)).send(&mut writer).await?;
                    Message::ready_for_query(transaction_status::IDLE).send(&mut writer).await?;
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
        tx_manager: TransactionManager,
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
        writer.write_all(b"postgrustql> \n").await?;
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
                writer.write_all(b"postgrustql> \n").await?;
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
                    if !inst.databases.contains_key(&session.database_name) {
                        format!("Error: Database '{}' not found\n", session.database_name)
                    } else {
                        // Получаем мутабельную ссылку на БД
                        let db = inst.get_database_mut(&session.database_name).unwrap();

                        match stmt {
                            crate::parser::Statement::Begin => {
                                if transaction.is_active() {
                                    "Warning: Transaction already active\n".to_string()
                                } else {
                                    let tx_id = tx_manager.begin_transaction();
                                    transaction.begin(tx_id, db);
                                    format!("Transaction started (ID: {})\n", tx_id)
                                }
                            }
                            crate::parser::Statement::Commit => {
                                if !transaction.is_active() {
                                    "Error: No active transaction\n".to_string()
                                } else {
                                    transaction.commit();
                                    // Save server instance after commit
                                    let mut storage_guard = storage.lock().await;
                                    if let Err(e) = storage_guard.save_server_instance(&inst) {
                                        format!("Warning: Failed to persist changes: {}\n", e)
                                    } else {
                                        "Transaction committed\n".to_string()
                                    }
                                }
                            }
                            crate::parser::Statement::Rollback => {
                                if !transaction.is_active() {
                                    "Error: No active transaction\n".to_string()
                                } else {
                                    transaction.rollback(db);
                                    "Transaction rolled back\n".to_string()
                                }
                            }
                            other_stmt => {
                                // Get storage lock for WAL logging and checkpointing
                                let mut storage_guard = storage.lock().await;

                                // Execute with WAL logging (only if not in transaction)
                                let storage_option = if !transaction.is_active() {
                                    Some(&mut *storage_guard)
                                } else {
                                    None
                                };

                                // Get database_storage if available
                                let mut db_storage_guard = if let Some(ref db_storage) = database_storage {
                                    Some(db_storage.lock().await)
                                } else {
                                    None
                                };
                                let db_storage_option = db_storage_guard.as_deref_mut();

                                match QueryExecutor::execute(db, other_stmt, storage_option, &tx_manager, db_storage_option) {
                                    Ok(result) => {
                                        // Checkpoint if needed (only if not in transaction)
                                        if !transaction.is_active() {
                                            if let Err(e) = storage_guard.save_server_instance(&inst) {
                                                format!("Warning: Failed to checkpoint: {}\n", e)
                                            } else {
                                                Self::format_result(result)
                                            }
                                        } else {
                                            Self::format_result(result)
                                        }
                                    }
                                    Err(e) => format!("Error: {}\n", e),
                                }
                            }
                        }
                    }
                }
                Err(e) => format!("Parse error: {}\n", e),
            };

            writer.write_all(response.as_bytes()).await?;
            writer.write_all(b"postgrustql> \n").await?;
            writer.flush().await?;
        }

        Ok(())
    }

    fn format_result(result: QueryResult) -> String {
        match result {
            QueryResult::Success(msg) => format!("{}\n", msg),
            QueryResult::Rows(rows, columns) => {
                if rows.is_empty() {
                    return "(0 rows)\n".to_string();
                }

                let mut table = ComfyTable::new();
                table.load_preset(UTF8_FULL);

                // Add header
                table.set_header(columns.iter().map(|c| Cell::new(c)));

                // Add rows
                for row in rows {
                    table.add_row(row.iter().map(|c| Cell::new(c)));
                }

                format!("{}\n({} rows)\n", table, table.row_iter().count() - 1)
            }
        }
    }

    fn convert_privilege(priv_type: &crate::parser::PrivilegeType) -> crate::types::Privilege {
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
}
