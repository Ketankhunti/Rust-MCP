// examples/simple_server.rs
use rust_mcp_sdk::server::{McpServer, McpServerInternal, TransportHandle};
use rust_mcp_sdk::transport::StdioTransport;
use rust_mcp_sdk::{ServerCapabilities, ServerInfo, McpError};
use serde_json::json;
use anyhow::Result;
use std::sync::Arc; // Needed for Arc::new

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("Starting MCP Server example...");

    // Define the server's capabilities
    let server_capabilities = ServerCapabilities {
        logging: Some(json!({})), // Server supports basic logging
        prompts: None,
        resources: None,
        tools: None, // No tools implemented yet for this example
        completions: None,
        experimental: None,
    };

    // Create a new server instance
    let server = McpServer::new(
        TransportHandle::Stdio(StdioTransport::new()),
        "RustMcpSdkExampleServer".to_string(),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities,
        Some("This is a simple Rust MCP server example. It only handles initialize messages.".to_string()),
    ).await; // new() is now async, so await it

    eprintln!("Server initialized. Listening for client connections on stdin/stdout...");

    // Register a dummy handler for a custom request, just to show it works
    let internal_for_custom = server.internal.clone();
    server.register_request_handler(
        "custom/ping",
        Arc::new(move |request, _| {
            let internal = internal_for_custom.clone();
            Box::pin(async move {
                eprintln!("Server: Received custom/ping request: {:?}", request);
                
                // Accessing internal state here if needed
                let current_negotiated_version = internal.negotiated_protocol_version.lock().await;
                eprintln!("Server: Negotiated protocol version (in custom handler): {:?}", *current_negotiated_version);

                Ok(rust_mcp_sdk::Response::new_success(
                    request.id.clone(),
                    Some(json!({"pong": true, "message": "Hello from custom handler!"}))
                ))
            })
        })
    ).await;


    // Run the server loop
    if let Err(e) = server.run().await {
        eprintln!("Server run loop terminated with error: {:?}", e);
    }

    eprintln!("Server example finished.");
    Ok(())
}