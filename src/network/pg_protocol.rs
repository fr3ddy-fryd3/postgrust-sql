use bytes::{BufMut, BytesMut};
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// `PostgreSQL` protocol version 3.0
pub const PROTOCOL_VERSION: i32 = 196_608; // (3 << 16) | 0

/// SSL request code
pub const SSL_REQUEST_CODE: i32 = 80_877_103; // Special code for SSL negotiation

/// Message types (from backend to frontend)
pub mod backend {
    pub const AUTHENTICATION: u8 = b'R';
    pub const READY_FOR_QUERY: u8 = b'Z';
    pub const ROW_DESCRIPTION: u8 = b'T';
    pub const DATA_ROW: u8 = b'D';
    pub const COMMAND_COMPLETE: u8 = b'C';
    pub const ERROR_RESPONSE: u8 = b'E';
    pub const PARAMETER_STATUS: u8 = b'S';
}

/// Message types (from frontend to backend)
pub mod frontend {
    pub const QUERY: u8 = b'Q';
    pub const TERMINATE: u8 = b'X';
    pub const PASSWORD: u8 = b'p'; // v2.0.0: Password message
}

/// Transaction status indicators
pub mod transaction_status {
    pub const IDLE: u8 = b'I'; // Not in transaction
    pub const IN_TRANSACTION: u8 = b'T'; // In transaction block
    pub const FAILED: u8 = b'E'; // In failed transaction
}

/// `PostgreSQL` data type OIDs (simplified)
pub mod oid {
    pub const INT4: i32 = 23;
    pub const TEXT: i32 = 25;
    pub const BOOL: i32 = 16;
}

/// Error field codes
pub mod error_field {
    pub const SEVERITY: u8 = b'S';
    pub const CODE: u8 = b'C';
    pub const MESSAGE: u8 = b'M';
}

pub struct StartupMessage {
    pub parameters: HashMap<String, String>,
}

/// `PasswordMessage` from client (v2.0.0)
pub struct PasswordMessage {
    pub password: String,
}

impl PasswordMessage {
    /// Read `PasswordMessage` from client
    /// Format: 'p' + Int32(length) + password (null-terminated string)
    pub async fn read<R: AsyncReadExt + Unpin>(reader: &mut R) -> std::io::Result<Self> {
        // Message type already read by caller
        // Read length
        let length = reader.read_i32().await?;

        // Read password (null-terminated)
        let password_len = (length - 4) as usize; // -4 for length field itself
        let mut password_buf = vec![0u8; password_len];
        reader.read_exact(&mut password_buf).await?;

        // Remove null terminator if present
        if password_buf.last() == Some(&0) {
            password_buf.pop();
        }

        let password = String::from_utf8_lossy(&password_buf).to_string();
        Ok(Self { password })
    }
}

impl StartupMessage {
    pub async fn read<R: AsyncReadExt + Unpin>(reader: &mut R) -> std::io::Result<Self> {
        // Read length (Int32)
        let length = reader.read_i32().await?;

        // Read protocol version (Int32)
        let protocol_version = reader.read_i32().await?;

        if protocol_version != PROTOCOL_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Unsupported protocol version: {protocol_version}"),
            ));
        }

        // Read parameters (length - 8 bytes for the two Int32s we already read)
        let params_length = (length - 8) as usize;
        let mut params_buf = vec![0u8; params_length];
        reader.read_exact(&mut params_buf).await?;

        // Parse null-terminated string pairs
        let mut parameters = HashMap::new();
        let mut i = 0;
        while i < params_buf.len() {
            // Read key
            let key_start = i;
            while i < params_buf.len() && params_buf[i] != 0 {
                i += 1;
            }
            if i >= params_buf.len() {
                break; // Reached end or terminator
            }
            let key = String::from_utf8_lossy(&params_buf[key_start..i]).to_string();
            i += 1; // Skip null terminator

            if i >= params_buf.len() {
                break; // No value after key
            }

            // Read value
            let value_start = i;
            while i < params_buf.len() && params_buf[i] != 0 {
                i += 1;
            }
            let value = String::from_utf8_lossy(&params_buf[value_start..i]).to_string();
            i += 1; // Skip null terminator

            if !key.is_empty() {
                parameters.insert(key, value);
            }
        }

        Ok(Self { parameters })
    }
}

pub struct Message {
    buf: BytesMut,
}

impl Default for Message {
    fn default() -> Self {
        Self::new()
    }
}

impl Message {
    #[must_use] 
    pub fn new() -> Self {
        Self {
            buf: BytesMut::new(),
        }
    }

    /// Write message type and reserve space for length
    fn start(&mut self, msg_type: u8) -> usize {
        self.buf.put_u8(msg_type);
        let len_pos = self.buf.len();
        self.buf.put_i32(0); // Placeholder for length
        len_pos
    }

    /// Update the length field
    fn finish(&mut self, len_pos: usize) {
        let total_len = self.buf.len() - len_pos;
        let len_bytes = (total_len as i32).to_be_bytes();
        self.buf[len_pos..len_pos + 4].copy_from_slice(&len_bytes);
    }

    /// `AuthenticationOk` message
    #[must_use] 
    pub fn authentication_ok() -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::AUTHENTICATION);
        msg.buf.put_i32(0); // 0 = AuthenticationOk
        msg.finish(len_pos);
        msg
    }

    /// `AuthenticationCleartextPassword` message (v2.0.0)
    /// Requests client to send cleartext password
    #[must_use] 
    pub fn authentication_cleartext_password() -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::AUTHENTICATION);
        msg.buf.put_i32(3); // 3 = AuthenticationCleartextPassword
        msg.finish(len_pos);
        msg
    }

    /// `ParameterStatus` message
    #[must_use] 
    pub fn parameter_status(name: &str, value: &str) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::PARAMETER_STATUS);
        msg.put_cstring(name);
        msg.put_cstring(value);
        msg.finish(len_pos);
        msg
    }

    /// `ReadyForQuery` message
    #[must_use] 
    pub fn ready_for_query(status: u8) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::READY_FOR_QUERY);
        msg.buf.put_u8(status);
        msg.finish(len_pos);
        msg
    }

    /// `ErrorResponse` message
    #[must_use] 
    pub fn error_response(message: &str) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::ERROR_RESPONSE);

        // Severity
        msg.buf.put_u8(error_field::SEVERITY);
        msg.put_cstring("ERROR");

        // SQLSTATE code
        msg.buf.put_u8(error_field::CODE);
        msg.put_cstring("42000"); // Generic syntax error

        // Message
        msg.buf.put_u8(error_field::MESSAGE);
        msg.put_cstring(message);

        // Terminator
        msg.buf.put_u8(0);

        msg.finish(len_pos);
        msg
    }

    /// `RowDescription` message
    #[must_use] 
    pub fn row_description(columns: &[String]) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::ROW_DESCRIPTION);

        msg.buf.put_i16(columns.len() as i16);

        for col in columns {
            msg.put_cstring(col);
            msg.buf.put_i32(0); // table OID
            msg.buf.put_i16(0); // column attribute number
            msg.buf.put_i32(oid::TEXT); // data type OID (default to TEXT)
            msg.buf.put_i16(-1); // data type size (-1 = variable)
            msg.buf.put_i32(-1); // type modifier
            msg.buf.put_i16(0); // format code (0 = text)
        }

        msg.finish(len_pos);
        msg
    }

    /// `DataRow` message
    #[must_use] 
    pub fn data_row(values: &[String]) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::DATA_ROW);

        msg.buf.put_i16(values.len() as i16);

        for val in values {
            let val_bytes = val.as_bytes();
            msg.buf.put_i32(val_bytes.len() as i32);
            msg.buf.put_slice(val_bytes);
        }

        msg.finish(len_pos);
        msg
    }

    /// `CommandComplete` message
    #[must_use] 
    pub fn command_complete(tag: &str) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::COMMAND_COMPLETE);
        msg.put_cstring(tag);
        msg.finish(len_pos);
        msg
    }

    /// Helper: write null-terminated string
    fn put_cstring(&mut self, s: &str) {
        self.buf.put_slice(s.as_bytes());
        self.buf.put_u8(0);
    }

    /// Send the message to a writer
    pub async fn send<W: AsyncWriteExt + Unpin>(&self, writer: &mut W) -> std::io::Result<()> {
        writer.write_all(&self.buf).await?;
        writer.flush().await?;
        Ok(())
    }
}

/// Read a frontend message
pub async fn read_frontend_message<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> std::io::Result<(u8, Vec<u8>)> {
    let msg_type = reader.read_u8().await?;
    let length = reader.read_i32().await? as usize;

    // Length includes itself (4 bytes) but not the message type
    if length < 4 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid message length",
        ));
    }

    let data_length = length - 4;
    let mut data = vec![0u8; data_length];
    reader.read_exact(&mut data).await?;

    Ok((msg_type, data))
}

/// Extract null-terminated string from byte slice
#[must_use] 
pub fn extract_cstring(data: &[u8]) -> Option<(String, usize)> {
    let mut end = 0;
    while end < data.len() && data[end] != 0 {
        end += 1;
    }

    if end >= data.len() {
        return None;
    }

    let s = String::from_utf8_lossy(&data[..end]).to_string();
    Some((s, end + 1)) // +1 to skip the null terminator
}
