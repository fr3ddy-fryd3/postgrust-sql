use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("PostgrustSQL CLI Client");
    println!("Connecting to 127.0.0.1:5432...\n");

    let mut stream = TcpStream::connect("127.0.0.1:5432").await?;
    println!("Connected!");

    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    // Read and display welcome messages
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).await?;
        print!("{}", line);
        // Server prompt can be "postgres=# " or "postgrustql> "
        if line.trim().ends_with('#') || line.trim().ends_with('>') {
            // Server sent initial prompt
            break;
        }
    }

    // Initialize rustyline editor for history and line editing
    let mut rl = DefaultEditor::new()?;

    // Try to load history from file (if it exists)
    let history_file = dirs::home_dir()
        .map(|mut p| {
            p.push(".rustdb_history");
            p
        });

    if let Some(ref path) = history_file {
        let _ = rl.load_history(path); // Ignore error if file doesn't exist
    }

    println!(); // Newline after server messages

    loop {
        // Use rustyline to read input (supports history, editing, etc.)
        let readline = rl.readline("rustdb> ");

        match readline {
            Ok(query) => {
                let query = query.trim();

                if query.is_empty() {
                    continue;
                }

                // Add to history
                let _ = rl.add_history_entry(query);

                // Check for quit/exit
                if query.eq_ignore_ascii_case("quit") || query.eq_ignore_ascii_case("exit") {
                    writer.write_all(b"quit\n").await?;
                    writer.flush().await?;

                    // Read goodbye message
                    loop {
                        line.clear();
                        let n = reader.read_line(&mut line).await?;
                        if n == 0 {
                            break;
                        }
                        print!("{}", line);
                        if line.contains("Goodbye") {
                            break;
                        }
                    }

                    // Save history before exiting
                    if let Some(ref path) = history_file {
                        let _ = rl.save_history(path);
                    }

                    return Ok(());
                }

                // Send query
                writer.write_all(query.as_bytes()).await?;
                writer.write_all(b"\n").await?;
                writer.flush().await?;

                // Read response until we see the prompt
                loop {
                    line.clear();
                    let n = reader.read_line(&mut line).await?;

                    if n == 0 {
                        eprintln!("\nConnection closed by server");

                        // Save history before exiting
                        if let Some(ref path) = history_file {
                            let _ = rl.save_history(path);
                        }

                        return Ok(());
                    }

                    // Check if this line contains the prompt
                    if line.trim().ends_with('>') {
                        // Server sent prompt - ready for next input
                        break;
                    } else {
                        // Print the response line
                        print!("{}", line);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C
                println!("^C");
                writer.write_all(b"quit\n").await?;
                writer.flush().await?;

                // Save history before exiting
                if let Some(ref path) = history_file {
                    let _ = rl.save_history(path);
                }

                return Ok(());
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D
                println!("quit");
                writer.write_all(b"quit\n").await?;
                writer.flush().await?;

                // Save history before exiting
                if let Some(ref path) = history_file {
                    let _ = rl.save_history(path);
                }

                return Ok(());
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                return Err(err.into());
            }
        }
    }
}
