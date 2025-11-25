use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Connecting to RustDB...");
    let mut stream = TcpStream::connect("127.0.0.1:5432").await?;
    println!("Connected!");

    sleep(Duration::from_millis(100)).await;

    let queries = vec![
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, age INTEGER);",
        "INSERT INTO users (id, name, age) VALUES (1, 'Alice', 30);",
        "INSERT INTO users (id, name, age) VALUES (2, 'Bob', 25);",
        "INSERT INTO users (id, name, age) VALUES (3, 'Charlie', 35);",
        "SELECT * FROM users;",
        "UPDATE users SET age = 31 WHERE name = 'Alice';",
        "SELECT * FROM users WHERE name = 'Alice';",
        "DELETE FROM users WHERE age < 30;",
        "SELECT * FROM users;",
        "quit",
    ];

    for query in queries {
        println!("Sending: {}", query);
        stream.write_all(query.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;
        sleep(Duration::from_millis(200)).await;
    }

    println!("\nAll queries sent!");
    sleep(Duration::from_millis(500)).await;

    Ok(())
}
