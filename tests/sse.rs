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
    let mut server_process = Command::new(env!("CARGO_BIN_EXE_http_sse"))
        .spawn()
        .expect("Failed to spawn simple_http_server process");

    tokio::time::sleep(Duration::from_secs(1)).await;
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
    let trigger_request_id = 1; // Define request ID for the trigger

    let sse_listener_handle = tokio::spawn(async move {
        println!("Test (SSE Task): Connecting to SSE endpoint...");
        let mut stream = Client::new()
            .get(SERVER_URL)
            .header("Mcp-Session-Id", sse_session_id)
            .send()
            .await
            .expect("Failed to connect to SSE endpoint")
            .bytes_stream();
        
        while let Some(item) = stream.next().await {
            let chunk = item.expect("Error while reading from SSE stream");
            let line = String::from_utf8(chunk.to_vec()).unwrap();
            
            for part in line.split('\n').filter(|s| !s.is_empty()) {
                 if let Some(data) = part.strip_prefix("data: ") {
                     received_events_clone.lock().unwrap().push(data.to_string());
                 }
            }
        }
    });
    
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 4. Trigger the server to send a notification via a POST request
    println!("Test: Triggering server-sent event via POST to sse_push...");
    let trigger_response = http_client
        .post(SERVER_URL)
        .header("Mcp-Session-Id", &session_id)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": trigger_request_id, // Use the variable here
            "method": "sse_push",
            "params": {}
        }))
        .send()
        .await
        .expect("Failed to send trigger request");
    
    println!("Test: Trigger request successful.");

    // --- NEW: Verify the body of the POST response ---
    assert_eq!(trigger_response.status(), StatusCode::OK);
    let trigger_body: Value = trigger_response.json().await.expect("Failed to parse trigger response JSON");
    println!("Test: Received direct POST response body: {:#?}", trigger_body);
    
    assert_eq!(trigger_body["id"], trigger_request_id);
    assert!(trigger_body["error"].is_null(), "POST response should not be an error.");
    assert_eq!(trigger_body["result"]["status"], "notification_sent");
    println!("Test: Direct POST response body verified.");
    // --- END OF NEW SECTION ---

    // 5. Verify the SSE event was received by the listener task
    println!("Test: Verifying received SSE event...");
    if timeout(Duration::from_secs(2), async {
        loop {
            if !received_events.lock().unwrap().is_empty() { break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }).await.is_err() {
        panic!("Test failed: Did not receive SSE event within timeout period.");
    }
    
    let events = received_events.lock().unwrap();
    assert_eq!(events.len(), 1, "Expected to receive exactly one SSE event");

    let received_json: Value = serde_json::from_str(&events[0]).expect("Failed to parse received SSE JSON");
    assert_eq!(received_json["method"], "server/test_notification");
    assert_eq!(received_json["params"]["message"], "hello from sse");

    println!("Test: SSE event verified successfully!");

    // 6. Clean up
    sse_listener_handle.abort();
    server_process
        .kill()
        .await
        .expect("Failed to kill server process");

    println!("\n--- HTTP Server with SSE Test completed successfully. ---");
}