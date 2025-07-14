// examples/simple_server.rs

use rust_mcp_sdk::server::{McpServer, McpServerInternal};
use rust_mcp_sdk::{ServerCapabilities, ServerInfo, McpError, Tool, InputSchema, ToolsCallRequestParams, ToolsCallResult, ToolOutputContentBlock};
use serde_json::{json, Value};
use anyhow::Result;
use std::sync::Arc;

// Define a concrete implementation for the "calculator" tool
async fn execute_calculator_tool(params: Value, _server_internal: Arc<McpServerInternal>) -> Result<Value, McpError> {
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

    Ok(json!({"value": result}))
}


#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("Starting MCP Server example (Stdio Mode)...");

    let server_capabilities = ServerCapabilities {
        logging: Some(json!({})),
        prompts: None,
        resources: None,
        tools: Some(rust_mcp_sdk::ServerToolsCapability { list_changed: Some(true) }),
        completions: None,
        experimental: None,
    };

    // Use the new new_stdio_server constructor
    let server = McpServer::start_stdio_server(
        "RustMcpSdkExampleServer".to_string(),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities,
        Some("This is a simple Rust MCP server example. It handles initialize, custom/ping, tools/list, and tools/call.".to_string()),
    ).await;

    // --- TOOL DEFINITION AND REGISTRATION ---
    server.add_tool(Tool {
        name: "calculator".to_string(),
        title: Some("Simple Arithmetic Calculator".to_string()),
        description: "A tool to perform basic addition and subtraction.".to_string(),
        input_schema: InputSchema::Inline(json!({
            "type": "object", "properties": {"operation": {"type": "string", "enum": ["add", "subtract"]}, "a": {"type": "number"}, "b": {"type": "number"}}, "required": ["operation", "a", "b"]
        })),
        output_schema: Some(InputSchema::Inline(json!({"type": "object", "properties": {"value": {"type": "number"}}}))),
        requires_auth: Some(false), annotations: None, experimental: None,
    }).await;

    server.add_tool(Tool {
        name: "weather".to_string(),
        title: Some("Current Weather Fetcher".to_string()),
        description: "Fetches current weather information for a location. (Note: Execution not implemented for this tool)".to_string(),
        input_schema: InputSchema::Inline(json!({
            "type": "object", "properties": {"location": {"type": "string", "description": "City name or zip code"}}, "required": ["location"]
        })),
        output_schema: Some(InputSchema::Inline(json!({"type": "object", "properties": {"temperature": {"type": "number"}}}))),
        requires_auth: Some(false), annotations: None, experimental: None,
    }).await;

    server.register_tool_execution_handler(
        "calculator",
        Arc::new(move |params, _server_state| {
             // _server_state is fine here if not used
            Box::pin(async move {
                execute_calculator_tool(params, _server_state).await
            })
        })
    ).await;

    // --- OTHER HANDLERS (non-tool specific) ---

    // Register the custom/ping handler (for testing general requests)
    server.register_request_handler(
        "custom/ping",
        Arc::new(move |request, server_state| {
            Box::pin(async move {
                // --- Negotiation Check within the closure (as per earlier discussion) ---
                let negotiated_version_guard = server_state.negotiated_protocol_version.lock().await;
                if negotiated_version_guard.is_none() {
                    return Ok(rust_mcp_sdk::Response::new_error(
                        Some(request.id),
                        -32600,
                        &"Protocol handshake not complete. Please send 'initialize' request first.".to_string(),
                        None,
                    ));
                }
                // --- End Negotiation Check ---

                eprintln!("Server: Received custom/ping request: {:?}", request);
                let current_negotiated_version = server_state.negotiated_protocol_version.lock().await;
                eprintln!("Server: Negotiated protocol version (in custom handler): {:?}", *current_negotiated_version);

                Ok(rust_mcp_sdk::Response::new_success(
                    request.id,
                    Some(json!({"pong": true, "message": "Hello from custom handler!"}))
                ))
            })
        })
    ).await;

    eprintln!("Server initialized. Listening for client connections on stdin/stdout...");

    if let Err(e) = server.run().await {
        eprintln!("Server run loop terminated with error: {:?}", e);
    }

    eprintln!("Server example finished.");
    Ok(())
}