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
    // Extended Query Protocol (v2.4.0)
    pub const PARSE_COMPLETE: u8 = b'1';
    pub const BIND_COMPLETE: u8 = b'2';
    pub const CLOSE_COMPLETE: u8 = b'3';
    pub const NO_DATA: u8 = b'n';
    pub const PARAMETER_DESCRIPTION: u8 = b't';
    // COPY Protocol (v2.4.0)
    pub const COPY_IN_RESPONSE: u8 = b'G';
    pub const COPY_OUT_RESPONSE: u8 = b'H';
    pub const COPY_DONE: u8 = b'c';
    pub const COPY_DATA: u8 = b'd';
}

/// Message types (from frontend to backend)
pub mod frontend {
    pub const QUERY: u8 = b'Q';
    pub const TERMINATE: u8 = b'X';
    pub const PASSWORD: u8 = b'p'; // v2.0.0: Password message
    // Extended Query Protocol (v2.4.0)
    pub const PARSE: u8 = b'P';
    pub const BIND: u8 = b'B';
    pub const DESCRIBE: u8 = b'D';
    pub const EXECUTE: u8 = b'E';
    pub const CLOSE: u8 = b'C';
    pub const SYNC: u8 = b'S';
    // COPY Protocol (v2.4.0)
    pub const COPY_DATA: u8 = b'd';
    pub const COPY_DONE: u8 = b'c';
    pub const COPY_FAIL: u8 = b'f';
}

/// Transaction status indicators
pub mod transaction_status {
    pub const IDLE: u8 = b'I'; // Not in transaction
    pub const IN_TRANSACTION: u8 = b'T'; // In transaction block
    pub const FAILED: u8 = b'E'; // In failed transaction
}

/// `PostgreSQL` data type OIDs (for COPY binary format and protocol compatibility)
pub mod oid {
    // Numeric types
    pub const INT2: i32 = 21;        // SmallInt
    pub const INT4: i32 = 23;        // Integer (existing)
    pub const INT8: i32 = 20;        // BigInt/BigSerial
    pub const FLOAT8: i32 = 701;     // Real (f64)
    pub const NUMERIC: i32 = 1700;   // Numeric(p,s) / Decimal

    // String types
    pub const TEXT: i32 = 25;        // Text (existing)
    pub const BPCHAR: i32 = 1042;    // Char(n) - blank-padded char
    pub const VARCHAR: i32 = 1043;   // Varchar(n)

    // Boolean
    pub const BOOL: i32 = 16;        // Boolean (existing)

    // Date/Time types
    pub const DATE: i32 = 1082;      // Date
    pub const TIMESTAMP: i32 = 1114; // Timestamp (without timezone)
    pub const TIMESTAMPTZ: i32 = 1184; // TimestampTz (with timezone)

    // Special types
    pub const UUID: i32 = 2950;      // UUID
    pub const JSON: i32 = 114;       // JSON
    pub const JSONB: i32 = 3802;     // JSONB
    pub const BYTEA: i32 = 17;       // Bytea (binary data)

    // Note: ENUM types use dynamically assigned OIDs per enum type
    // Note: SERIAL/BIGSERIAL use INT4/INT8 at runtime
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

/// `ParseMessage` from client (v2.4.0 - Extended Query Protocol)
/// Format: 'P' + Int32(length) + statement_name (cstring) + query (cstring) + Int16(num_params) + [Int32(param_oid), ...]
pub struct ParseMessage {
    pub statement_name: String,
    pub query: String,
    pub param_types: Vec<i32>,
}

impl ParseMessage {
    pub fn from_data(data: &[u8]) -> std::io::Result<Self> {
        let mut pos = 0;

        // Read statement name (null-terminated)
        let (statement_name, bytes_read) = extract_cstring(&data[pos..])
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid statement name"))?;
        pos += bytes_read;

        // Read query (null-terminated)
        let (query, bytes_read) = extract_cstring(&data[pos..])
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid query"))?;
        pos += bytes_read;

        // Read parameter type count
        if pos + 2 > data.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing param count"));
        }
        let num_params = i16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        // Read parameter OIDs
        let mut param_types = Vec::with_capacity(num_params);
        for _ in 0..num_params {
            if pos + 4 > data.len() {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing param type"));
            }
            let oid = i32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            param_types.push(oid);
            pos += 4;
        }

        Ok(Self {
            statement_name,
            query,
            param_types,
        })
    }
}

/// `BindMessage` from client (v2.4.0 - Extended Query Protocol)
/// Format: 'B' + Int32(length) + portal (cstring) + statement (cstring) + param_formats + param_values + result_formats
pub struct BindMessage {
    pub portal_name: String,
    pub statement_name: String,
    pub param_values: Vec<Option<Vec<u8>>>,
}

impl BindMessage {
    pub fn from_data(data: &[u8]) -> std::io::Result<Self> {
        let mut pos = 0;

        // Read portal name
        let (portal_name, bytes_read) = extract_cstring(&data[pos..])
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid portal name"))?;
        pos += bytes_read;

        // Read statement name
        let (statement_name, bytes_read) = extract_cstring(&data[pos..])
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid statement name"))?;
        pos += bytes_read;

        // Read parameter format codes count
        if pos + 2 > data.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing format codes count"));
        }
        let num_format_codes = i16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        // Skip format codes (we'll assume text format for simplicity)
        pos += num_format_codes * 2;

        // Read parameter values count
        if pos + 2 > data.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing param values count"));
        }
        let num_params = i16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        // Read parameter values
        let mut param_values = Vec::with_capacity(num_params);
        for _ in 0..num_params {
            if pos + 4 > data.len() {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing param length"));
            }
            let length = i32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
            pos += 4;

            if length == -1 {
                // NULL value
                param_values.push(None);
            } else {
                let length = length as usize;
                if pos + length > data.len() {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid param value"));
                }
                let value = data[pos..pos + length].to_vec();
                param_values.push(Some(value));
                pos += length;
            }
        }

        Ok(Self {
            portal_name,
            statement_name,
            param_values,
        })
    }
}

/// `DescribeMessage` from client (v2.4.0 - Extended Query Protocol)
/// Format: 'D' + Int32(length) + type (byte: 'S' or 'P') + name (cstring)
pub struct DescribeMessage {
    pub describe_type: char, // 'S' for statement, 'P' for portal
    pub name: String,
}

impl DescribeMessage {
    pub fn from_data(data: &[u8]) -> std::io::Result<Self> {
        if data.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Empty describe message"));
        }

        let describe_type = data[0] as char;
        let (name, _) = extract_cstring(&data[1..])
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid name"))?;

        Ok(Self {
            describe_type,
            name,
        })
    }
}

/// `ExecuteMessage` from client (v2.4.0 - Extended Query Protocol)
/// Format: 'E' + Int32(length) + portal (cstring) + max_rows (Int32)
pub struct ExecuteMessage {
    pub portal_name: String,
    pub max_rows: i32,
}

impl ExecuteMessage {
    pub fn from_data(data: &[u8]) -> std::io::Result<Self> {
        let (portal_name, bytes_read) = extract_cstring(data)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid portal name"))?;

        if bytes_read + 4 > data.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Missing max_rows"));
        }

        let max_rows = i32::from_be_bytes([
            data[bytes_read],
            data[bytes_read + 1],
            data[bytes_read + 2],
            data[bytes_read + 3],
        ]);

        Ok(Self {
            portal_name,
            max_rows,
        })
    }
}

/// `CloseMessage` from client (v2.4.0 - Extended Query Protocol)
/// Format: 'C' + Int32(length) + type (byte: 'S' or 'P') + name (cstring)
pub struct CloseMessage {
    pub close_type: char, // 'S' for statement, 'P' for portal
    pub name: String,
}

impl CloseMessage {
    pub fn from_data(data: &[u8]) -> std::io::Result<Self> {
        if data.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Empty close message"));
        }

        let close_type = data[0] as char;
        let (name, _) = extract_cstring(&data[1..])
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid name"))?;

        Ok(Self {
            close_type,
            name,
        })
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

    /// `ParseComplete` message (v2.4.0 - Extended Query Protocol)
    #[must_use]
    pub fn parse_complete() -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::PARSE_COMPLETE);
        msg.finish(len_pos);
        msg
    }

    /// `BindComplete` message (v2.4.0 - Extended Query Protocol)
    #[must_use]
    pub fn bind_complete() -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::BIND_COMPLETE);
        msg.finish(len_pos);
        msg
    }

    /// `CloseComplete` message (v2.4.0 - Extended Query Protocol)
    #[must_use]
    pub fn close_complete() -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::CLOSE_COMPLETE);
        msg.finish(len_pos);
        msg
    }

    /// `NoData` message (v2.4.0 - Extended Query Protocol)
    #[must_use]
    pub fn no_data() -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::NO_DATA);
        msg.finish(len_pos);
        msg
    }

    /// `CopyInResponse` message (v2.4.0 - COPY Protocol)
    /// Server tells client to start sending COPY data
    #[must_use]
    pub fn copy_in_response(format: u8, num_columns: i16) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::COPY_IN_RESPONSE);
        msg.buf.put_u8(format); // 0 = text, 1 = binary
        msg.buf.put_i16(num_columns);
        // Column format codes (0=text, 1=binary for each column)
        for _ in 0..num_columns {
            msg.buf.put_i16(i16::from(format)); // Use same format for all columns
        }
        msg.finish(len_pos);
        msg
    }

    /// `CopyOutResponse` message (v2.4.0 - COPY Protocol)
    /// Server tells client it will start sending COPY data
    #[must_use]
    pub fn copy_out_response(format: u8, num_columns: i16) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::COPY_OUT_RESPONSE);
        msg.buf.put_u8(format); // 0 = text, 1 = binary
        msg.buf.put_i16(num_columns);
        // Column format codes (0=text, 1=binary for each column)
        for _ in 0..num_columns {
            msg.buf.put_i16(i16::from(format)); // Use same format for all columns
        }
        msg.finish(len_pos);
        msg
    }

    /// `CopyData` message (v2.4.0 - COPY Protocol)
    /// Contains a chunk of COPY data
    #[must_use]
    pub fn copy_data(data: &[u8]) -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::COPY_DATA);
        msg.buf.put_slice(data);
        msg.finish(len_pos);
        msg
    }

    /// `CopyDone` message (v2.4.0 - COPY Protocol)
    /// Indicates end of COPY data
    #[must_use]
    pub fn copy_done() -> Self {
        let mut msg = Self::new();
        let len_pos = msg.start(backend::COPY_DONE);
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
