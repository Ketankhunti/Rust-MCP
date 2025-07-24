// examples/simple_http_client.rs

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;
use anyhow::Result;

// The URL of the MCP HTTP endpoint
const SERVER_URL: &str = "http://127.0.0.1:8080/mcp";

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting MCP Simple HTTP Client example...");

    // Create an HTTP client
    let http_client = Client::builder()
        .timeout(Duration::from_secs(10)) // Set a reasonable timeout for HTTP requests
        .build()?;
    println!("Client: HTTP client created.");

    // --- 1. Perform Initialize Handshake (HTTP POST) ---
    println!("\n--- Client: Sending Initialize Request ---");
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
                "clientInfo": { "name": "SimpleHttpClient", "version": "1.0" }
            }
        }
    });

    let http_response = http_client.post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&initialize_request)
        .send()
        .await?;
    let status = http_response.status();
    println!("Client: Sent Initialize POST request. Status: {}", http_response.status());
    let response_body: Value = http_response.json().await?;
    println!("Client: Received Initialize response: {:#?}", response_body);

    // Basic validation of the response
    if status != StatusCode::OK {
        eprintln!("Client: Initialize failed with HTTP status: {}", status);
        return Err(anyhow::anyhow!("Initialize failed: {:#?}", response_body));
    }
    if response_body["id"].as_i64() != Some(initialize_request_id as i64) {
        eprintln!("Client: Initialize response ID mismatch.");
        return Err(anyhow::anyhow!("Initialize response ID mismatch: {:#?}", response_body));
    }
    println!("--- Client: Initialize Handshake Complete ---");


    // --- 2. Send Initialized Notification (HTTP POST) ---
    println!("\n--- Client: Sending Initialized Notification ---");
    let initialized_notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });

    let http_response = http_client.post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&initialized_notification)
        .send()
        .await?;

    println!("Client: Sent Initialized Notification. Status: {}", http_response.status());
    // Notifications expect 202 Accepted, and no body
    if http_response.status() != StatusCode::ACCEPTED {
        eprintln!("Client: Initialized notification failed with HTTP status: {}", http_response.status());
        return Err(anyhow::anyhow!("Initialized notification failed: {}", http_response.status()));
    }
    println!("--- Client: Initialized Notification Sent ---");


    // --- 3. Call Calculator Tool (HTTP POST) ---
    println!("\n--- Client: Calling Calculator Tool (add 5 + 3) ---");
    let calculator_request_id = 1;
    let calculator_request = json!({
        "jsonrpc": "2.0",
        "id": calculator_request_id,
        "method": "tools/call",
        "params": {
            "name": "calculator",
            "arguments": { "operation": "add", "a": 5.0, "b": 3.0 }
        }
    });

    let http_response = http_client.post(SERVER_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(&calculator_request)
        .send()
        .await?;

    println!("Client: Sent Calculator tool call. Status: {}", http_response.status());
    let status = http_response.status();
    let response_body: Value = http_response.json().await?;
    println!("Client: Received Calculator tool response: {:#?}", response_body);

    // Validate calculator response
    if status != StatusCode::OK {
        eprintln!("Client: Calculator call failed with HTTP status: {}", status);
        return Err(anyhow::anyhow!("Calculator call failed: {:#?}", response_body));
    }
    if response_body["id"].as_i64() != Some(calculator_request_id as i64) {
        eprintln!("Client: Calculator response ID mismatch.");
        return Err(anyhow::anyhow!("Calculator response ID mismatch: {:#?}", response_body));
    }
    if response_body["result"]["isError"].as_bool() != Some(false) {
        eprintln!("Client: Calculator reported error.");
        return Err(anyhow::anyhow!("Calculator reported error: {:#?}", response_body));
    }
    if response_body["result"]["content"][0]["text"].as_str().map(|s| s.contains("{\"value\":8.0}")) != Some(true) {
        eprintln!("Client: Calculator result content mismatch.");
        return Err(anyhow::anyhow!("Calculator result content mismatch: {:#?}", response_body));
    }
    println!("--- Client: Calculator Tool Call Verified ---");


    println!("\nSimple HTTP Client example finished.");
    Ok(())
}