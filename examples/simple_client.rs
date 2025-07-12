// examples/simple_client.rs

use rust_mcp_sdk::client::McpClient;
use rust_mcp_sdk::{ClientCapabilities, ClientInfo, ClientRootsCapability};
use tokio::task;
use anyhow::Result;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<()> {
    println!("Starting MCP Client example...");

    // Create a new client instance
    let mut client = McpClient::new();
    // Spawn the client's main loop in a background task.
    // This task will handle all incoming messages from the server.
    

    // Give the client's internal tasks a moment to spin up and connect.
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // --- Phase 1: Initialization ---
    println!("\n--- Sending initialize request ---");
    let client_capabilities = ClientCapabilities {
        // Client declares its supported features
        roots: Some(ClientRootsCapability { list_changed: Some(true) }),
        sampling: Some(json!({})),
        elicitation: None,
        experimental: None,
    };
    let client_info = Some(ClientInfo {
        name: "RustMcpSdkExampleClient".to_string(),
        title: Some("Rust MCP Example Client".to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    });

    match client.initialize(client_capabilities, client_info).await {
        Ok(server_init_result) => {
            println!("Initialization successful!");
            println!("Negotiated Protocol Version: {}", server_init_result.protocol_version);
            println!("Server Info: {:?}", server_init_result.server_info);
            println!("Server Capabilities: {:?}", server_init_result.capabilities);
            if let Some(instructions) = server_init_result.instructions {
                println!("Server Instructions: {}", instructions);
            }

            // --- Phase 2: Operation (after successful initialization) ---
            println!("\n--- Attempting to call custom/ping method ---");
            // This calls a custom method you registered on the server example.
            match client.call_method("custom/ping", Some(json!({"client_message": "Hello from Rust Client!"}))).await {
                Ok(response) => {
                    println!("'custom/ping' response received: {:?}", response);
                    // You can further process the response.result if it's a success
                    // For example: if let Some(value) = response.result { println!("Ping result: {:?}", value); }
                },
                Err(e) => {
                    eprintln!("'custom/ping' call failed: {:?}", e);
                }
            }

            println!("\n--- Attempting to call a non-existent method ---");
            // This will likely result in a "Method not found" error from the server
            match client.call_method("non_existent/method", Some(json!({"data": "test"}))).await {
                Ok(response) => {
                    println!("'non_existent/method' response received: {:?}", response);
                },
                Err(e) => {
                    eprintln!("'non_existent/method' call failed: {:?}", e);
                }
            }
        }
        Err(e) => {
            eprintln!("Initialization failed: {:?}", e);
        }
    }
    
    let client_run_handle = task::spawn(async move {
        if let Err(e) = client.run().await {
            eprintln!("Client run loop terminated with error: {:?}", e);
        }
    });

    // Wait for the client's run loop to complete (e.g., if transport disconnects due to server exit)
    // In a real application, you might have a graceful shutdown mechanism here.
    client_run_handle.await?;

    println!("\nClient example finished.");
    Ok(())
}