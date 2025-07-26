use std::sync::Arc;

// Bring the procedural macro into scope
use mcp_macros::tool;
use rust_mcp_sdk::ServerCapabilities;

// Define the tool's logic as a simple, native async function.
// The `#[tool]` macro handles all the boilerplate.
#[tool(
    title = "Simple Calculator",
    description = "A tool for adding or subtracting two numbers."
)]
pub async fn calculator(operation: String, a: f64, b: f64) -> Result<f64, String> {
    eprintln!("Executing 'calculator' tool with: {} {} {}", operation, a, b);
    match operation.as_str() {
        "add" => Ok(a + b),
        "subtract" => Ok(a - b),
        _ => Err(format!("Invahlid operation: '{}'", operation)),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("Starting MCP Simple HTTP Server example...");

    const ADDR: &str = "127.0.0.1:8080";

    let server_capabilities = ServerCapabilities {
        logging: None,//Some(json!({})),
        prompts: None,
        resources: None,
        tools: Some(rust_mcp_sdk::ServerToolsCapability {
            list_changed: Some(true),
        }),
        completions: None,
        experimental: None,
    };

    let mut app_server = rust_mcp_sdk::server::McpServer::new(
        "RustMcpSdkSimpleServer".to_string(),
        Some("MyServer".to_string()),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities.clone(),
        Some("This is a simple Rust MCP server example. It handles initialize and calculator.".to_string()),
    )
    .await;

    // --- Discover and register all tools from the macro ---
    app_server.register_discovered_tools().await;

    // ... (You can add prompts and other handlers here as before) ...

    if let Err(e) = rust_mcp_sdk::server::McpHttpServer::start_listener(ADDR, Arc::new(app_server)).await {
        eprintln!("HTTP Server: Listener terminated with error: {:?}", e);
    }

    eprintln!("HTTP Server example finished.");
    Ok(())
}
