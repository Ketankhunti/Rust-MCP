// tests/simple_http_test.rs

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::Command;
use uuid::Uuid; // For parsing Session ID from header

// The URL of the MCP HTTP endpoint
const SERVER_URL: &str = "http://127.0.0.1:8080/mcp";

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

    tokio::time::sleep(Duration::from_secs(1)).await; // Give server time to bind and start listening
    println!("Test: Simple HTTP Server process spawned.");

    // 2. Create an HTTP client
    let http_client = Client::builder()
        .timeout(Duration::from_secs(10)) // Set a reasonable timeout for HTTP requests
        .build()
        .expect("Failed to build HTTP client.");
    println!("Test: HTTP client created.");

    let mut session_id: Option<Uuid> = None; // Store session ID for subsequent requests

    // --- 1. Initialize Handshake ---
    println!("\n--- Test: Performing Initialize Handshake ---");
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
                "clientInfo": { "name": "SimpleTestClient", "version": "test" } // More detailed clientInfo
            }
        }
    });

    let http_response = http_client
        .post(SERVER_URL)
        .header("Content-Type", "application/json") // Explicitly set Content-Type
        .header("Accept", "application/json, text/event-stream") // Explicitly set Accept as per MCP spec
        .json(&initialize_request)
        .send()
        .await
        .expect("Failed to send initialize POST request.");

    println!("Test: Sent initialize POST request.");

    // Assert HTTP status code
    assert_eq!(http_response.status(), StatusCode::OK, "Expected 200 OK for initialize response.");
    
    // Assert response headers
    assert_eq!(http_response.headers().get("Content-Type").map(|h| h.to_str().unwrap()), Some("application/json"), "Expected Content-Type: application/json");
    let session_id_header = http_response
        .headers()
        .get("Mcp-Session-Id")
        .expect("Mcp-Session-Id header not found in initialize response")
        .to_str()
        .expect("Mcp-Session-Id header is not valid UTF-8.");
    let parsed_session_id = Uuid::parse_str(session_id_header).expect("Failed to parse UUID from Mcp-Session-Id header.");
    session_id = Some(parsed_session_id); // Store for later use
    println!("Test: Captured session ID: {}", parsed_session_id);

    // Parse and assert response body
    let response_body: Value = http_response.json().await.expect("Failed to parse initialize response JSON.");
    println!("Test: Received initialize response: {:#?}", response_body);

    assert_eq!(response_body["jsonrpc"], "2.0", "Response should be JSON-RPC 2.0.");
    assert_eq!(response_body["id"].as_i64().unwrap(), initialize_request_id as i64, "Response ID should match request ID.");
    assert!(response_body["result"].is_object(), "Response result should be an object.");
    assert_eq!(response_body["result"]["protocolVersion"], "2025-06-18", "Negotiated protocol version mismatch.");
    assert!(response_body["result"]["capabilities"].is_object(), "Server capabilities should be an object.");
    assert!(response_body["result"]["capabilities"]["tools"].is_object(), "Server should advertise tools capability.");
    assert_eq!(response_body["result"]["capabilities"]["tools"]["listChanged"], true, "Server tools capability listChanged should be true.");
    assert!(response_body["result"]["serverInfo"].is_object(), "Server info should be an object.");
    assert_eq!(response_body["result"]["serverInfo"]["name"], "RustMcpSdkSimpleServer", "Server name mismatch.");
    assert_eq!(response_body["result"]["serverInfo"]["version"], env!("CARGO_PKG_VERSION"), "Server version mismatch.");
    assert!(response_body["result"]["instructions"].is_string(), "Server instructions should be a string.");
    println!("--- Test: Initialize Handshake Complete ---");


    // --- 2. Initialized Notification ---
    println!("\n--- Test: Sending Initialized Notification ---");
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    let http_response = http_client
        .post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Session-Id", session_id.unwrap().to_string()) // Add Session ID header
        .json(&initialized_notification)
        .send()
        .await
        .expect("Failed to send initialized notification");

    // Assert HTTP status code
    assert_eq!(http_response.status(), StatusCode::ACCEPTED, "Expected 202 Accepted for initialized notification.");
    println!("Test: Sent Initialized Notification. Status: {}", http_response.status());
    println!("--- Test: Initialized notification verified ---");


    // --- 3. Calculator Tool Call ---
    println!("\n--- Test: Calling tools/call (calculator) ---");
    let calculator_call_request_id = 1;
    let calculator_call_request = json!({
        "jsonrpc": "2.0",
        "id": calculator_call_request_id,
        "method": "tools/call",
        "params": { "name": "calculator", "arguments": { "operation": "add", "a": 10.0, "b": 5.0 } }
    });

    let http_response = http_client
        .post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Session-Id", session_id.unwrap().to_string()) // Add Session ID header
        .json(&calculator_call_request)
        .send()
        .await
        .expect("Failed to send calculator call request");

    // Assert HTTP status code
    assert_eq!(http_response.status(), StatusCode::OK, "Expected 200 OK for calculator call response.");
    
    // Parse and assert response body
    let response_body: Value = http_response.json().await.expect("Failed to parse calculator call response JSON.");
    println!("Test: Received calculator call response: {:#?}", response_body);

    assert_eq!(response_body["jsonrpc"], "2.0", "Response should be JSON-RPC 2.0.");
    assert_eq!(response_body["id"].as_i64().unwrap(), calculator_call_request_id as i64, "Response ID should match request ID.");
    assert!(response_body["result"].is_object(), "Response result should be an object.");
    assert_eq!(response_body["result"]["isError"].as_bool().unwrap(), false, "Calculator result should not be an error.");
    assert!(response_body["result"]["content"].is_array(), "Calculator result content should be an array.");
    assert_eq!(response_body["result"]["content"].as_array().unwrap().len(), 1, "Calculator result content array length mismatch.");
    assert_eq!(response_body["result"]["content"][0]["type"], "text", "Calculator result content type should be 'text'.");
    assert_eq!(response_body["result"]["content"][0]["text"], "{\"value\":15.0}", "Calculator result content text mismatch.");

    println!("--- Test: Calculator tool call verified ---");


    // 4. Clean up
    server_process
        .kill()
        .await
        .expect("Failed to kill server process");

    println!("\n--- Simple HTTP Test completed successfully. ---");
}