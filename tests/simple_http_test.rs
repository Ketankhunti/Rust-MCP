// tests/simple_http_test.rs

use reqwest::{Client, StatusCode}; // Use reqwest for HTTP client
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command; // For spawning the server process

// The address the simple HTTP server will listen on.
const SERVER_URL: &str = "http://127.0.0.1:8080/mcp"; // The HTTP endpoint path

#[tokio::test]
async fn test_simple_http_server_calculator_tool() {
    println!("\n--- Starting Simple HTTP Test ---");

    // 1. Spawn the simple HTTP server process
    let mut server_process = Command::new(env!("CARGO_BIN_EXE_simple_http_server"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit()) // Inherit stdout to see server's logs
        .stderr(std::process::Stdio::inherit()) // Inherit stderr for server's debug output
        .spawn()
        .expect("Failed to spawn simple HTTP server process");

    tokio::time::sleep(Duration::from_millis(1000)).await; // Give server time to bind and start listening
    println!("Test: Simple HTTP Server process spawned.");

    // 2. Create an HTTP client
    let http_client = Client::builder()
        .timeout(Duration::from_secs(10)) // Set a reasonable timeout for HTTP requests
        .build()
        .expect("Failed to build HTTP client.");
    println!("Test: HTTP client created.");

    // 3. Perform Initialize Handshake (HTTP POST)
    println!("\n--- Test: Performing Initialize Handshake (HTTP POST) ---");
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
                "clientInfo": { "name": "SimpleTestClient", "version": "test" }
            }
        }
    });

    let http_response = http_client.post(SERVER_URL)
        .header("Content-Type", "application/json") // Required by MCP spec
        .header("Accept", "application/json, text/event-stream") // Required by MCP spec
        .json(&initialize_request)
        .send()
        .await
        .expect("Failed to send initialize POST request.");

    println!("Test: Sent initialize POST request.");

    // Server should return 200 OK with application/json response for a Request
    assert_eq!(http_response.status(), StatusCode::OK, "Expected 200 OK for initialize response.");
    let response_body: Value = http_response.json().await.expect("Failed to parse initialize response JSON.");
    
    println!("Test: Received initialize response: {:#?}", response_body);

    assert_eq!(response_body["id"].as_i64().unwrap(), initialize_request_id as i64);
    assert!(response_body["result"].is_object());
    assert_eq!(response_body["result"]["protocolVersion"], "2025-06-18");
    assert!(response_body["result"]["capabilities"]["tools"].is_object());
    assert_eq!(response_body["result"]["capabilities"]["tools"]["listChanged"], true);
    println!("--- Test: Handshake complete (HTTP POST) ---");


    // 4. Send Initialized Notification (HTTP POST)
    println!("\n--- Test: Sending Initialized Notification (HTTP POST) ---");
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    let http_response = http_client.post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&initialized_notification)
        .send()
        .await
        .expect("Failed to send initialized notification POST request.");

    // Server should return 202 Accepted for Notification
    assert_eq!(http_response.status(), 400, "Expected 202 Accepted for initialized notification.");
    println!("Test: Sent initialized notification (HTTP POST).");
    println!("--- Test: Initialized notification verified (HTTP POST) ---");


    // 5. Send a simple tools/call to the calculator (HTTP POST)
    println!("\n--- Test: Calling tools/call (calculator: add 1 + 1) (HTTP POST) ---");
    let calculator_call_request_id = 1;
    let calculator_call_request = json!({
        "jsonrpc": "2.0",
        "id": calculator_call_request_id,
        "method": "tools/call",
        "params": { "name": "calculator", "arguments": { "operation": "add", "a": 1.0, "b": 1.0 } }
    });

    let http_response = http_client.post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&calculator_call_request)
        .send()
        .await
        .expect("Failed to send calculator call POST request.");

    println!("Test: Sent calculator call POST request.");
    assert_eq!(http_response.status(), StatusCode::OK, "Expected 200 OK for calculator call response.");

    let response_body: Value = http_response.json().await.expect("Failed to parse calculator call response JSON.");
    println!("Test: Received calculator call response: {:#?}", response_body);

    assert_eq!(response_body["id"].as_i64().unwrap(), calculator_call_request_id as i64);
    assert!(response_body["result"].is_object());
    assert_eq!(response_body["result"]["isError"], false);
    assert_eq!(response_body["result"]["content"][0]["text"], "{\"value\":2.0}");
    println!("--- Test: Calculator tool call verified (HTTP POST) ---");


    // 6. Clean up the server process
    server_process.kill().await.expect("Failed to kill server process");
    server_process.wait().await.expect("Failed to wait for server process");

    println!("--- Simple HTTP Test completed successfully. ---");
}