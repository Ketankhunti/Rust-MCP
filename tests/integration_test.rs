// tests/integration_tests.rs

use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::json;
use std::time::Duration; // For tokio::time::sleep

// Helper function to read a single JSON line from an AsyncBufRead
async fn read_json_line<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<serde_json::Value> {
    let mut line = String::new();
    // Use a timeout to prevent tests from hanging indefinitely if no response
    let read_timeout = Duration::from_secs(5);
    let read_fut = reader.read_line(&mut line);

    match tokio::time::timeout(read_timeout, read_fut).await {
        Ok(Ok(0)) => {
            println!("Test: EOF encountered when reading line.");
            None // EOF
        },
        Ok(Ok(_)) => {
            if line.trim().is_empty() {
                println!("Test: Read empty line, skipping.");
                None // Skip empty lines
            } else {
                match serde_json::from_str(&line) {
                    Ok(val) => Some(val),
                    Err(e) => {
                        eprintln!("Test: Failed to parse JSON line: '{}' - Error: {}", line.trim(), e);
                        None
                    }
                }
            }
        },
        Ok(Err(e)) => {
            eprintln!("Test: Error reading line: {}", e);
            None
        }
        Err(_) => { // Timeout error
            eprintln!("Test: Read operation timed out after {:?}.", read_timeout);
            None
        }
    }
}

#[tokio::test]
async fn test_server_client_basic_handshake_and_custom_ping() {
    println!("\n--- Starting test_server_client_basic_handshake_and_custom_ping ---");

    // 1. Spawn the server process
    let mut server_process = Command::new(env!("CARGO_BIN_EXE_simple_server"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit()) // Inherit stderr to see server's debug output
        .spawn()
        .expect("Failed to spawn server process");

    let server_stdin = server_process.stdin.take().expect("Failed to get server stdin");
    let server_stdout = server_process.stdout.take().expect("Failed to get server stdout");

    let mut server_stdin_writer = server_stdin; // This is the pipe to send data to server's stdin
    let mut server_stdout_reader = BufReader::new(server_stdout); // This is the pipe to read data from server's stdout

    println!("Test: Server spawned.");

    // 2. Send initialize request to the server
    let initialize_request_id = 0;
    let initialize_request = json!({
        "jsonrpc": "2.0",
        "id": initialize_request_id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "roots": { "listChanged": true },
                "sampling": {},
                "clientInfo": { "name": "TestClient", "version": "test" }
            }
        }
    });

    let req_str = initialize_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write initialize request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    println!("Test: Sent initialize request.");

    // 3. Read initialize response from the server
    let response_val = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read initialize response or server closed connection");

    println!("Test: Received initialize response: {}", response_val);

    // Basic assertions on initialize response
    assert_eq!(response_val["jsonrpc"], "2.0");
    assert_eq!(response_val["id"], initialize_request_id);
    assert!(response_val["result"].is_object());
    assert_eq!(response_val["result"]["protocolVersion"], "2025-06-18");
    // Assert that 'tools' capability is NOT present, matching the server's config
    assert!(response_val["result"]["capabilities"]["tools"].is_null() || response_val["result"]["capabilities"]["tools"].as_object().unwrap().is_empty());

    // NEW STEP 4: Send initialized notification to the server
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    let notif_str = initialized_notification.to_string();
    server_stdin_writer.write_all(notif_str.as_bytes()).await.expect("Failed to write initialized notification");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline after notification");
    println!("Test: Sent initialized notification.");

    // (The server's `run` loop should then process this notification)
    // There is no response to an initialized notification, so we just continue.

    println!("--- Test: Handshake complete ---");

    // 5. Send custom/ping request (This was step 5 before, remains the same)
    let ping_request_id = 1;
    let ping_request = json!({
        "jsonrpc": "2.0",
        "id": ping_request_id as u64,
        "method": "custom/ping",
        "params": { "message": "hello server" }
    });

    let req_str = ping_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write custom/ping request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    println!("Test: Sent custom/ping request.");

    // 6. Read custom/ping response (This was step 6 before, remains the same)
    let ping_response_val = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read custom/ping response or server closed connection");

    println!("Test: Received custom/ping response: {}", ping_response_val);

    assert_eq!(ping_response_val["id"], ping_request_id);
    assert!(ping_response_val["result"]["pong"].as_bool().unwrap());
    assert_eq!(ping_response_val["result"]["message"], "Hello from custom handler!");

    println!("Test: custom/ping verified.");


    // 7. Clean up the server process
    // Close stdin to signal EOF to the server
    drop(server_stdin_writer); // This closes the stdin pipe to the server
    println!("Test: Server stdin closed (signaling EOF).");

    // Give server a moment to react and exit
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Wait for the server process to exit
    let status = server_process.wait().await.expect("Failed to wait for server process");
    println!("Test: Server process exited with status: {:?}", status);

    // Assert that the server exited successfully (status code 0)
    assert!(status.success(), "Server process did not exit successfully.");

    println!("--- Test completed successfully. ---");
}