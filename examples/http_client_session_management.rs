use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

const SERVER_URL: &str = "http://127.0.0.1:8080/mcp";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("\n--- Starting Simple HTTP Client ---");

    let http_client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client.");
    println!("Client: HTTP client created.");

    // 1. Perform Initialize Handshake
    println!("\n--- Client: Performing Initialize Handshake ---");
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
                "elicitation": {},
                "experimental": {},
                "clientInfo": {
                    "name": "SimpleTestClient",
                    "version": "1.0"
                }
            }
        }
    });

    let http_response = http_client
        .post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&initialize_request)
        .send()
        .await
        .expect("Failed to send initialize POST request.");

    println!("Client: Sent initialize POST request.");

    let session_id_header = http_response
        .headers()
        .get("Mcp-Session-Id")
        .expect("Mcp-Session-Id header not found in initialize response")
        .to_str()?
        .to_string();
    let session_id =
        Uuid::parse_str(&session_id_header).expect("Failed to parse Mcp-Session-Id");

    println!("Client: Established session with ID: {}", session_id);

    let response_body: Value = http_response
        .json()
        .await
        .expect("Failed to parse initialize response JSON.");
    println!(
        "Client: Received initialize response: {:#?}",
        response_body
    );

    // 2. Send Initialized Notification
    println!("\n--- Client: Sending Initialized Notification ---");
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    let http_response = http_client
        .post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Session-Id", session_id.to_string())
        .json(&initialized_notification)
        .send()
        .await
        .expect("Failed to send initialized notification POST request.");

    println!("Client: Sent initialized notification.");
    println!(
        "Client: Received status for notification: {}",
        http_response.status()
    );

    // 3. Call the calculator tool
    println!("\n--- Client: Calling tools/call (calculator: add 5 + 3) ---");
    let calculator_call_request_id = 1;
    let calculator_call_request = json!({
        "jsonrpc": "2.0",
        "id": calculator_call_request_id,
        "method": "tools/call",
        "params": {
            "name": "calculator",
            "arguments": {
                "operation": "add",
                "a": 5.0,
                "b": 3.0
            }
        }
    });

    let http_response = http_client
        .post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("Mcp-Session-Id", session_id.to_string())
        .json(&calculator_call_request)
        .send()
        .await
        .expect("Failed to send calculator call POST request.");

    println!("Client: Sent calculator call POST request.");
    let response_body: Value = http_response
        .json()
        .await
        .expect("Failed to parse calculator call response JSON.");
    println!(
        "Client: Received calculator call response: {:#?}",
        response_body
    );

    println!("\n--- Simple HTTP Client finished successfully. ---");

    Ok(())
}