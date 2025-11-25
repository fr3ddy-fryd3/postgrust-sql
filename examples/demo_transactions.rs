use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== RustDB Transaction Demo ===\n");
    println!("Connecting to 127.0.0.1:5432...");

    // Give server time to start if needed
    thread::sleep(Duration::from_millis(500));

    let mut stream = TcpStream::connect("127.0.0.1:5432")?;
    println!("Connected!\n");

    let mut reader = BufReader::new(stream.try_clone()?);

    // Read welcome message
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        print!("{}", line);
        if line.trim().ends_with('>') {
            break;
        }
    }

    println!("\n--- Demo: Creating table and inserting data ---");

    let commands = vec![
        "CREATE TABLE accounts (id INTEGER PRIMARY KEY, name TEXT NOT NULL, balance INTEGER)",
        "INSERT INTO accounts (id, name, balance) VALUES (1, 'Alice', 1000)",
        "INSERT INTO accounts (id, name, balance) VALUES (2, 'Bob', 500)",
        "SELECT * FROM accounts",
    ];

    for cmd in &commands {
        execute_command(&mut stream, &mut reader, cmd)?;
        thread::sleep(Duration::from_millis(200));
    }

    println!("\n--- Demo: Transaction COMMIT ---");
    println!("Starting transaction, updating Alice's balance, then committing...\n");

    let transaction_commit = vec![
        "BEGIN",
        "UPDATE accounts SET balance = 1500 WHERE name = 'Alice'",
        "SELECT * FROM accounts",
        "COMMIT",
        "SELECT * FROM accounts",
    ];

    for cmd in &transaction_commit {
        execute_command(&mut stream, &mut reader, cmd)?;
        thread::sleep(Duration::from_millis(200));
    }

    println!("\n--- Demo: Transaction ROLLBACK ---");
    println!("Starting transaction, updating Bob's balance, then rolling back...\n");

    let transaction_rollback = vec![
        "BEGIN",
        "UPDATE accounts SET balance = 9999 WHERE name = 'Bob'",
        "SELECT * FROM accounts",
        "ROLLBACK",
        "SELECT * FROM accounts",
    ];

    for cmd in &transaction_rollback {
        execute_command(&mut stream, &mut reader, cmd)?;
        thread::sleep(Duration::from_millis(200));
    }

    println!("\n--- Demo: Final cleanup ---");
    execute_command(&mut stream, &mut reader, "DROP TABLE accounts")?;

    println!("\n=== Demo Complete ===");
    execute_command(&mut stream, &mut reader, "quit")?;

    Ok(())
}

fn execute_command(
    stream: &mut TcpStream,
    reader: &mut BufReader<TcpStream>,
    cmd: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(">> {}", cmd);

    // Send command
    stream.write_all(cmd.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    // Read response
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line)?;

        if line.trim().ends_with('>') {
            break;
        }
        print!("{}", line);
    }
    println!();

    Ok(())
}
