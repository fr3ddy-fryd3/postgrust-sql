/// Simple test client for PostgreSQL wire protocol
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("PostgreSQL Protocol Test Client");
    println!("Connecting to 127.0.0.1:5432...\n");

    let mut stream = TcpStream::connect("127.0.0.1:5432").await?;

    // Send StartupMessage
    println!("Sending StartupMessage...");
    send_startup(&mut stream).await?;

    // Read authentication response
    println!("Reading authentication response...");
    let (msg_type, _data) = read_message(&mut stream).await?;
    if msg_type == b'R' {
        println!("✓ Received AuthenticationOk");
    } else {
        println!("✗ Unexpected message type: {}", msg_type as char);
    }

    // Read parameter status messages and ReadyForQuery
    loop {
        let (msg_type, data) = read_message(&mut stream).await?;
        match msg_type {
            b'S' => {
                // ParameterStatus
                if let Some(params) = parse_parameter_status(&data) {
                    println!("✓ Parameter: {} = {}", params.0, params.1);
                }
            }
            b'Z' => {
                // ReadyForQuery
                println!("✓ Received ReadyForQuery\n");
                break;
            }
            _ => {
                println!("  Skipping message type: {}", msg_type as char);
            }
        }
    }

    // Send a simple query
    println!("Sending query: SHOW TABLES;");
    send_query(&mut stream, "SHOW TABLES;").await?;

    // Read response
    let mut row_count = 0;
    loop {
        let (msg_type, data) = read_message(&mut stream).await?;
        match msg_type {
            b'T' => {
                // RowDescription
                println!("✓ Received RowDescription");
            }
            b'D' => {
                // DataRow
                row_count += 1;
                if let Some(values) = parse_data_row(&data) {
                    println!("  Row {}: {:?}", row_count, values);
                }
            }
            b'C' => {
                // CommandComplete
                let tag = String::from_utf8_lossy(&data[..data.len() - 1]);
                println!("✓ CommandComplete: {}", tag);
            }
            b'Z' => {
                // ReadyForQuery
                println!("✓ ReadyForQuery\n");
                break;
            }
            b'E' => {
                // ErrorResponse
                println!("✗ Error: {:?}", String::from_utf8_lossy(&data));
                break;
            }
            _ => {
                println!("  Unknown message type: {}", msg_type as char);
            }
        }
    }

    // Send Terminate
    println!("Sending Terminate...");
    stream.write_u8(b'X').await?;
    stream.write_i32(4).await?;
    stream.flush().await?;

    println!("\n✓ Test completed successfully!");
    Ok(())
}

async fn send_startup(stream: &mut TcpStream) -> Result<(), Box<dyn std::error::Error>> {
    let mut params = HashMap::new();
    params.insert("user", "rustdb");
    params.insert("database", "main");

    // Calculate length
    let mut param_bytes = Vec::new();
    for (key, value) in &params {
        param_bytes.extend_from_slice(key.as_bytes());
        param_bytes.push(0);
        param_bytes.extend_from_slice(value.as_bytes());
        param_bytes.push(0);
    }
    param_bytes.push(0); // Terminator

    let length = 4 + 4 + param_bytes.len(); // length field + protocol version + params

    stream.write_i32(length as i32).await?;
    stream.write_i32(196608).await?; // Protocol version 3.0
    stream.write_all(&param_bytes).await?;
    stream.flush().await?;

    Ok(())
}

async fn send_query(stream: &mut TcpStream, query: &str) -> Result<(), Box<dyn std::error::Error>> {
    stream.write_u8(b'Q').await?;
    let query_bytes = query.as_bytes();
    let length = 4 + query_bytes.len() + 1; // length field + query + null terminator
    stream.write_i32(length as i32).await?;
    stream.write_all(query_bytes).await?;
    stream.write_u8(0).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_message(stream: &mut TcpStream) -> Result<(u8, Vec<u8>), Box<dyn std::error::Error>> {
    let msg_type = stream.read_u8().await?;
    let length = stream.read_i32().await? as usize;
    let data_length = length - 4;
    let mut data = vec![0u8; data_length];
    stream.read_exact(&mut data).await?;
    Ok((msg_type, data))
}

fn parse_parameter_status(data: &[u8]) -> Option<(String, String)> {
    let mut i = 0;

    // Read first string (name)
    let name_start = i;
    while i < data.len() && data[i] != 0 {
        i += 1;
    }
    if i >= data.len() {
        return None;
    }
    let name = String::from_utf8_lossy(&data[name_start..i]).to_string();
    i += 1;

    // Read second string (value)
    let value_start = i;
    while i < data.len() && data[i] != 0 {
        i += 1;
    }
    let value = String::from_utf8_lossy(&data[value_start..i]).to_string();

    Some((name, value))
}

fn parse_data_row(data: &[u8]) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut i = 0;

    // Read column count
    if data.len() < 2 {
        return None;
    }
    let col_count = i16::from_be_bytes([data[i], data[i + 1]]) as usize;
    i += 2;

    for _ in 0..col_count {
        if i + 4 > data.len() {
            break;
        }

        // Read value length
        let val_len = i32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        i += 4;

        if val_len < 0 {
            // NULL value
            values.push("NULL".to_string());
        } else {
            let val_len = val_len as usize;
            if i + val_len > data.len() {
                break;
            }

            let value = String::from_utf8_lossy(&data[i..i + val_len]).to_string();
            values.push(value);
            i += val_len;
        }
    }

    Some(values)
}
