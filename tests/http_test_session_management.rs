use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use futures::StreamExt;

const SERVER_URL: &str = "http://127.0.0.1:8080/mcp";

#[tokio::test]
async fn test_http_server_with_sse() {
    println!("\n--- Starting HTTP Server with SSE Test ---");

    // 1. Spawn the server process
    let mut server_process = Command::new(env!("CARGO_BIN_EXE_simple_http_server"))
        .spawn()
        .expect("Failed to spawn simple_http_server process");

    tokio::time::sleep(Duration::from_secs(1)).await; // Give server time to start
    println!("Test: Server process spawned.");

    let http_client = Client::new();

    // 2. Initialize session via POST to get a session ID
    println!("Test: Initializing session...");
    let init_response = http_client
        .post(SERVER_URL)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": { "clientInfo": { "name": "SseTestClient" } }
            }
        }))
        .send()
        .await
        .expect("Failed to send initialize request");

    assert_eq!(init_response.status(), StatusCode::OK);
    let session_id = init_response
        .headers()
        .get("Mcp-Session-Id")
        .expect("Session ID not in initialize response")
        .to_str()
        .unwrap()
        .to_string();
    println!("Test: Session initialized with ID: {}", session_id);

    // 3. Spawn a background task to listen for SSE events
    let received_events = Arc::new(Mutex::new(Vec::new()));
    let received_events_clone = received_events.clone();
    let sse_session_id = session_id.clone();

    let sse_listener_handle = tokio::spawn(async move {
        println!("Test (SSE Task): Connecting to SSE endpoint...");
        let mut stream = Client::new()
            .get(SERVER_URL)
            .header("Mcp-Session-Id", sse_session_id)
            .send()
            .await
            .expect("Failed to connect to SSE endpoint")
            .bytes_stream();
        
        println!("Test (SSE Task): Connection established. Waiting for events...");
        // Listen for incoming data chunks
        while let Some(item) = stream.next().await {
            let chunk = item.expect("Error while reading from SSE stream");
            let line = String::from_utf8(chunk.to_vec()).unwrap();
            
            // A single SSE message might be split across multiple lines (`event:`, `data:`, etc.)
            // We are looking for the 'data:' line.
            for part in line.split('\n').filter(|s| !s.is_empty()) {
                 println!("Test (SSE Task): Received raw line: {}", part);
                 if let Some(data) = part.strip_prefix("data: ") {
                     println!("Test (SSE Task): Extracted data: {}", data);
                     received_events_clone.lock().unwrap().push(data.to_string());
                 }
            }
        }
        println!("Test (SSE Task): Stream ended.");
    });
    
    // Give the SSE GET connection a moment to establish
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 4. Trigger the server to send a notification via a POST request
    println!("Test: Triggering server-sent event via POST to test/sse_push...");
    let trigger_response = http_client
        .post(SERVER_URL)
        .header("Mcp-Session-Id", &session_id)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "test/sse_push",
            "params": {}
        }))
        .send()
        .await
        .expect("Failed to send trigger request");
    
    assert_eq!(trigger_response.status(), StatusCode::OK);
    println!("Test: Trigger request successful.");

    // 5. Verify the event was received by the listener task
    println!("Test: Verifying received event...");
    // Wait up to 2 seconds for an event to appear in our shared vector
    if let Ok(_) = timeout(Duration::from_secs(2), async {
        loop {
            if !received_events.lock().unwrap().is_empty() {
                break; // Exit loop once an event is found
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await {
        println!("Test: Event was received within timeout.");
    } else {
        panic!("Test failed: Did not receive SSE event within timeout period.");
    }
    
    // Lock the mutex and assert the content is correct
    let events = received_events.lock().unwrap();
    assert_eq!(events.len(), 1, "Expected to receive exactly one event");

    let received_json: Value = serde_json::from_str(&events[0]).expect("Failed to parse received JSON");
    assert_eq!(received_json["method"], "server/test_notification");
    assert_eq!(received_json["params"]["message"], "hello from sse");

    println!("Test: SSE event verified successfully!");

    // 6. Clean up
    sse_listener_handle.abort(); // Stop the listener task
    server_process
        .kill()
        .await
        .expect("Failed to kill server process");

    println!("\n--- HTTP Server with SSE Test completed successfully. ---");
}