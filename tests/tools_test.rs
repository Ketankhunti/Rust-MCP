// tests/integration_tests.rs

use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json::{json, Value};
use std::time::Duration;

// Helper function to read a single JSON line from an AsyncBufRead
async fn read_json_line<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Option<Value> {
    let mut line = String::new();
    let read_timeout = Duration::from_secs(5); // Increased timeout for potentially more server processing
    let read_fut = reader.read_line(&mut line);

    match tokio::time::timeout(read_timeout, read_fut).await {
        Ok(Ok(bytes_read)) => {
            if bytes_read == 0 {
                println!("Test: EOF encountered when reading line.");
                return None; // EOF
            }
            if line.trim().is_empty() {
                println!("Test: Read empty line, skipping. (Raw: '{}')", line.trim());
                return None; // Skip empty lines
            } else {
                match serde_json::from_str(&line) {
                    Ok(val) => {
                        println!("Test: Successfully parsed JSON: '{}'", line.trim());
                        Some(val)
                    },
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
async fn test_server_client_full_tools_integration() {
    println!("\n--- Starting test_server_client_full_tools_integration ---");

    // 1. Spawn the server process
    let mut server_process = Command::new(env!("CARGO_BIN_EXE_tools_server"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit()) // Inherit stderr to see server's debug output
        .spawn()
        .expect("Failed to spawn server process");

    let server_stdin = server_process.stdin.take().expect("Failed to get server stdin");
    let server_stdout = server_process.stdout.take().expect("Failed to get server stdout");

    let mut server_stdin_writer = server_stdin;
    let mut server_stdout_reader = BufReader::new(server_stdout);

    println!("Test: Server spawned.");

    // 2. Perform Initialize Handshake
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
    server_stdin_writer.flush().await.expect("Failed to flush after initialize request");
    println!("Test: Sent initialize request.");

    let response_val = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read initialize response or server closed connection");

    println!("Test: Received initialize response: {}", response_val);

    assert_eq!(response_val["id"], initialize_request_id);
    assert!(response_val["result"].is_object());
    assert_eq!(response_val["result"]["protocolVersion"], "2025-06-18");
    assert!(response_val["result"]["capabilities"]["tools"].is_object(), "Server should advertise tools capability.");

    // Send initialized notification
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    let notif_str = initialized_notification.to_string();
    server_stdin_writer.write_all(notif_str.as_bytes()).await.expect("Failed to write initialized notification");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline after notification");
    server_stdin_writer.flush().await.expect("Failed to flush after initialized notification");
    println!("Test: Sent initialized notification.");

    println!("--- Test: Handshake complete ---");

    // 3. Test tools/list
    let tools_list_request_id = 1;
    let tools_list_request = json!({
        "jsonrpc": "2.0",
        "id": tools_list_request_id,
        "method": "tools/list",
        "params": {}
    });

    let req_str = tools_list_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write tools/list request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    server_stdin_writer.flush().await.expect("Failed to flush after tools/list request");
    println!("Test: Sent tools/list request.");

    let tools_list_response_val = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read tools/list response or server closed connection");

    println!("Test: Received tools/list response: {}", tools_list_response_val);

    assert_eq!(tools_list_response_val["id"], tools_list_request_id);
    assert!(tools_list_response_val["result"]["tools"].is_array());
    let tools_array = tools_list_response_val["result"]["tools"].as_array().unwrap();
    assert_eq!(tools_array.len(), 2, "Should have 2 tools registered.");
    assert_eq!(tools_array[0]["name"], "calculator");
    assert_eq!(tools_array[1]["name"], "weather");
    println!("Test: tools/list verified.");

    // 4. Test tools/call (calculator: add)
    let calc_add_id = 2;
    let calc_add_request = json!({
        "jsonrpc": "2.0",
        "id": calc_add_id,
        "method": "tools/call",
        "params": {
            "name": "calculator",
            "arguments": { "operation": "add", "a": 5.0, "b": 3.0 }
        }
    });

    let req_str = calc_add_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write calc add request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    server_stdin_writer.flush().await.expect("Failed to flush after calc add request");
    println!("Test: Sent tools/call (calculator add) request.");

    let calc_add_response = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read calc add response or server closed connection");

    println!("Test: Received tools/call (calculator add) response: {}", calc_add_response);
    assert_eq!(calc_add_response["id"], calc_add_id);
    assert_eq!(calc_add_response["result"]["isError"], false);
    assert!(calc_add_response["result"]["content"].as_array().unwrap().len() > 0);
    // Note: The calculator tool currently returns `json!({"value": result})`
    // which then gets `to_string()`ed. We check for stringified JSON.
    assert_eq!(calc_add_response["result"]["content"][0]["text"], "{\"value\":8.0}");
    println!("Test: tools/call (calculator add) verified.");

    // 5. Test tools/call (calculator: subtract)
    let calc_sub_id = 3;
    let calc_sub_request = json!({
        "jsonrpc": "2.0",
        "id": calc_sub_id,
        "method": "tools/call",
        "params": {
            "name": "calculator",
            "arguments": { "operation": "subtract", "a": 10.0, "b": 2.0 }
        }
    });

    let req_str = calc_sub_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write calc sub request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    server_stdin_writer.flush().await.expect("Failed to flush after calc sub request");
    println!("Test: Sent tools/call (calculator subtract) request.");

    let calc_sub_response = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read calc sub response or server closed connection");

    println!("Test: Received tools/call (calculator subtract) response: {}", calc_sub_response);
    assert_eq!(calc_sub_response["id"], calc_sub_id);
    assert_eq!(calc_sub_response["result"]["isError"], false);
    assert_eq!(calc_sub_response["result"]["content"][0]["text"], "{\"value\":8.0}");
    println!("Test: tools/call (calculator subtract) verified.");

    // 6. Test tools/call (weather - not implemented execution handler)
    let weather_id = 4;
    let weather_request = json!({
        "jsonrpc": "2.0",
        "id": weather_id,
        "method": "tools/call",
        "params": {
            "name": "weather",
            "arguments": { "location": "London" }
        }
    });

    let req_str = weather_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write weather request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    server_stdin_writer.flush().await.expect("Failed to flush after weather request");
    println!("Test: Sent tools/call (weather) request.");

    let weather_response = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read weather response or server closed connection");

    println!("Test: Received tools/call (weather) response: {}", weather_response);
    assert_eq!(weather_response["id"], weather_id);
    assert!(weather_response["error"].is_object(), "Expected an error response for unimplemented tool.");
    assert_eq!(weather_response["error"]["code"], -32601); // Method not found / Tool not found
    assert!(weather_response["error"]["message"].as_str().unwrap().contains("Tool 'weather' not found or execution handler not registered."));
    println!("Test: tools/call (weather) error verified.");


    // 7. Test custom/ping
    let ping_request_id = 5;
    let ping_request = json!({
        "jsonrpc": "2.0",
        "id": ping_request_id,
        "method": "custom/ping",
        "params": { "message": "hello server" }
    });

    let req_str = ping_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write custom/ping request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    server_stdin_writer.flush().await.expect("Failed to flush after custom/ping request");
    println!("Test: Sent custom/ping request.");

    let ping_response_val = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read custom/ping response or server closed connection");

    println!("Test: Received custom/ping response: {}", ping_response_val);

    assert_eq!(ping_response_val["id"], ping_request_id);
    assert!(ping_response_val["result"]["pong"].as_bool().unwrap());
    assert_eq!(ping_response_val["result"]["message"], "Hello from custom handler!");
    println!("Test: custom/ping verified.");


    // 8. Test a non-existent method
    let non_existent_id = 6;
    let non_existent_request = json!({
        "jsonrpc": "2.0",
        "id": non_existent_id,
        "method": "non_existent/method",
        "params": { "data": "test" }
    });

    let req_str = non_existent_request.to_string();
    server_stdin_writer.write_all(req_str.as_bytes()).await.expect("Failed to write non-existent request");
    server_stdin_writer.write_all(b"\n").await.expect("Failed to write newline");
    server_stdin_writer.flush().await.expect("Failed to flush after non-existent request");
    println!("Test: Sent non-existent method request.");

    let non_existent_response = read_json_line(&mut server_stdout_reader).await
        .expect("Failed to read non-existent method response or server closed connection");

    println!("Test: Received non-existent method response: {}", non_existent_response);
    assert_eq!(non_existent_response["id"], non_existent_id);
    assert!(non_existent_response["error"].is_object());
    assert_eq!(non_existent_response["error"]["code"], -32601); // Method not found
    println!("Test: Non-existent method error verified.");


    // 9. Clean up the server process
    // Close stdin to signal EOF to the server
    drop(server_stdin_writer);
    println!("Test: Server stdin writer dropped, signaling EOF.");

    tokio::time::sleep(Duration::from_millis(50)).await;

    let output = server_process.wait_with_output().await.expect("Failed to wait for server process");
    println!("Test: Server process exited with status: {:?}", output.status);
    if !output.stdout.is_empty() {
        println!("Server stdout (after test): \n{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprintln!("Server stderr (after test): \n{}", String::from_utf8_lossy(&output.stderr));
    }

    assert!(output.status.success(), "Server process did not exit successfully. Status: {:?}", output.status);

    println!("--- Test completed successfully. ---");
}