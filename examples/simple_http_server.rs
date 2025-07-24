// examples/simple_http_server.rs

use rust_mcp_sdk::server::{McpHttpServer, McpServer, McpSessionInternal}; // Import McpServer (the app config)
use rust_mcp_sdk::{InputSchema, McpError, Request, Response, ServerCapabilities, Tool, ToolOutputContentBlock, ToolsCallRequestParams, ToolsCallResult};
use serde_json::{json, Value};
use anyhow::Result;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;
use std::time::Duration;

// Explicitly import handler type aliases for clarity and casting
use rust_mcp_sdk::server::{RequestHandler, NotificationHandler, ToolExecutionHandler};


// Concrete implementation for the "calculator" tool
async fn execute_calculator_tool(params: Value, _session_internal: Arc<McpSessionInternal>, _app_config: Arc<McpServer>) -> Result<Value, McpError> {
    eprintln!("Server: Executing calculator tool with params: {:?}", params);

    let operation = params.get("operation").and_then(Value::as_str)
                           .ok_or_else(|| McpError::ProtocolError("Calculator: 'operation' missing or invalid".to_string()))?;
    let a = params.get("a").and_then(Value::as_f64)
                   .ok_or_else(|| McpError::ProtocolError("Calculator: 'a' missing or invalid".to_string()))?;
    let b = params.get("b").and_then(Value::as_f64)
                   .ok_or_else(|| McpError::ProtocolError("Calculator: 'b' missing or invalid".to_string()))?;

    let result = match operation {
        "add" => a + b,
        "subtract" => a - b,
        _ => return Err(McpError::ProtocolError(format!("Calculator: Unknown operation '{}'", operation))),
    };

    eprintln!("Server: Calculator raw result: {}", result);

    Ok(json!({"value": result})) // Return the raw result as a JSON Value
}


#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("Starting MCP Simple HTTP Server example...");

    // Listen on port 8080 directly for HTTP requests
    const ADDR: &str = "127.0.0.1:8080"; 

    // --- Define Server Capabilities ---
    let server_capabilities = ServerCapabilities {
        logging: Some(json!({})),
        prompts: None,
        resources: None,
        tools: Some(rust_mcp_sdk::ServerToolsCapability { list_changed: Some(true) }), // Advertise tools support
        completions: None,
        experimental: None,
    };

    // --- Create the main McpServer application configurator instance ---
    // This holds the global handler maps and server info.
    let mut app_server = rust_mcp_sdk::server::McpServer::new( // Make mutable to register handlers
        "RustMcpSdkSimpleServer".to_string(),
        Some("MyServer".to_string()),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities.clone(), // Clone for the initial server setup
        Some("This is a simple Rust MCP server example. It handles initialize and calculator.".to_string()),
    ).await;


    // --- Register Tool Metadata ---
    app_server.add_tool(Tool {
        name: "calculator".to_string(),
        title: Some("Simple Arithmetic Calculator".to_string()),
        description: "A tool to perform basic addition and subtraction.".to_string(),
        input_schema: InputSchema::Inline(json!({
            "type": "object", "properties": {"operation": {"type": "string", "enum": ["add", "subtract"]}, "a": {"type": "number"}, "b": {"type": "number"}}, "required": ["operation", "a", "b"]
        })),
        output_schema: Some(InputSchema::Inline(json!({"type": "object", "properties": {"value": {"type": "number"}}}))),
        requires_auth: Some(false), annotations: None, experimental: None,
    }).await;

    // --- Register Tool Execution Handlers ---
    // The handler closure now explicitly takes Arc<McpServerInternal> and Arc<McpServer>
    let handler_for_calculator = Arc::new(move |params, session_internal_arg: Arc<McpSessionInternal>, app_config_arg: Arc<McpServer>| {
        Box::pin(async move {
            execute_calculator_tool(params, session_internal_arg, app_config_arg).await
        }) as Pin<Box<(dyn Future<Output = Result<Value, McpError>> + Send + 'static)>>
    }) as ToolExecutionHandler; // Explicit cast
    app_server.register_tool_execution_handler("calculator", handler_for_calculator).await;


    // --- Start the HTTP listener ---
    // McpHttpServer::start_listener is the main HTTP entry point.
    if let Err(e) = McpHttpServer::start_listener(
        ADDR, // Use the hardcoded address 127.0.0.1:8080
        Arc::new(app_server), // Pass a clone of the Arc<McpServer> (global app config)
    ).await {
        eprintln!("HTTP Server: Listener terminated with error: {:?}", e);
    }

    eprintln!("HTTP Server example finished.");
    Ok(())
}