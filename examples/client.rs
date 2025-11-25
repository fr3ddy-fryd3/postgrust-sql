use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to RustDB server...");
    let mut stream = TcpStream::connect("127.0.0.1:5432").await?;
    println!("Connected!");

    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    // Read welcome message
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    print!("{}", line);
    line.clear();
    reader.read_line(&mut line).await?;
    print!("{}", line);

    // Example queries
    let queries = vec![
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);",
        "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30);",
        "INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25);",
        "INSERT INTO users (id, name, age) VALUES (3, 'Charlie', 35);",
        "SELECT * FROM users;",
        "SELECT name, age FROM users WHERE age > 25;",
        "UPDATE users SET age = 31 WHERE name = 'Alice';",
        "SELECT * FROM users WHERE name = 'Alice';",
        "DELETE FROM users WHERE age < 30;",
        "SELECT * FROM users;",
    ];

    for query in queries {
        println!("\nExecuting: {}", query);
        writer.write_all(query.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        loop {
            line.clear();
            reader.read_line(&mut line).await?;
            print!("{}", line);
            if line.trim().ends_with('>') {
                break;
            }
        }

        // Small delay between queries
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Send quit
    writer.write_all(b"quit\n").await?;
    writer.flush().await?;

    // Read goodbye message
    line.clear();
    reader.read_line(&mut line).await?;
    print!("{}", line);

    Ok(())
}
