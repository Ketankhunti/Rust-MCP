
use rust_mcp_sdk::server::{McpServer, McpServerInternal};
use rust_mcp_sdk::{ServerCapabilities, ServerInfo, McpError, Tool, InputSchema, ToolsCallRequestParams, ToolsCallResult, ToolOutputContentBlock};
use serde_json::{json, Value};
use anyhow::Result;
use std::sync::Arc;

// Define a concrete implementation for the "calculator" tool
// This function performs the actual calculation logic.
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

    // Return the raw result as a JSON Value, which handle_tools_call will then wrap
    Ok(json!({"value": result}))
}


#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("Starting MCP Server example (with Tools Call functionality)...");

    // Define the server's capabilities.
    // Ensure `tools` capability is advertised.
    let server_capabilities = ServerCapabilities {
        logging: Some(json!({})),
        prompts: None,
        resources: None,
        tools: Some(rust_mcp_sdk::ServerToolsCapability { list_changed: Some(true) }), // Advertise tools support
        completions: None,
        experimental: None,
    };

    // Create a new server instance
    let server = McpServer::new(
        "RustMcpSdkExampleServer".to_string(),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities,
        Some("This is a simple Rust MCP server example. It handles initialize, custom/ping, tools/list, and tools/call.".to_string()),
    ).await; // new() is async, so await it

    // --- TOOL DEFINITION AND REGISTRATION ---

    // 1. Add Tool Metadata (for tools/list request)
    // This defines the tool's public API and description.
    server.add_tool(Tool {
        name: "calculator".to_string(),
        title: Some("Simple Arithmetic Calculator".to_string()),
        description: "A tool to perform basic addition and subtraction.".to_string(),
        input_schema: InputSchema::Inline(json!({
            "type": "object",
            "properties": {
                "operation": {"type": "string", "enum": ["add", "subtract"]},
                "a": {"type": "number"},
                "b": {"type": "number"}
            },
            "required": ["operation", "a", "b"]
        })),
        output_schema: Some(InputSchema::Inline(json!({"type": "object", "properties": {"value": {"type": "number"}}}))), // Output schema for 'value'
        requires_auth: Some(false),
        annotations: None,
        experimental: None,
    }).await;

    server.add_tool(Tool {
        name: "weather".to_string(),
        title: Some("Current Weather Fetcher".to_string()),
        description: "Fetches current weather information for a location. (Note: Execution not implemented for this tool)".to_string(),
        input_schema: InputSchema::Inline(json!({
            "type": "object",
            "properties": {
                "location": {"type": "string", "description": "City name or zip code"}
            },
            "required": ["location"]
        })),
        output_schema: Some(InputSchema::Inline(json!({"type": "object", "properties": {"temperature": {"type": "number"}}}))),
        requires_auth: Some(false),
        annotations: None,
        experimental: None,
    }).await;


    // 2. Register Tool Execution Handlers (for tools/call request)
    // This provides the actual Rust function that runs the tool's logic.
    let internal_for_calculator = server.internal.clone();
    server.register_tool_execution_handler(
        "calculator",
        Arc::new(move |params, _| {
            let internal = internal_for_calculator.clone();
            Box::pin(async move {
                execute_calculator_tool(params, internal).await // Pass params to tool func
            })
        })
    ).await;

    // --- OTHER HANDLERS (non-tool specific) ---

    // Register the custom/ping handler (for testing general requests)
    let internal_for_custom = server.internal.clone();
    server.register_request_handler(
        "custom/ping",
        Arc::new(move |request, _| {
            let internal = internal_for_custom.clone();
            Box::pin(async move {
                eprintln!("Server: Received custom/ping request: {:?}", request);
                let current_negotiated_version = internal.negotiated_protocol_version.lock().await;
                eprintln!("Server: Negotiated protocol version (in custom handler): {:?}", *current_negotiated_version);

                Ok(rust_mcp_sdk::Response::new_success(
                    request.id,
                    Some(json!({"pong": true, "message": "Hello from custom handler!"}))
                ))
            })
        })
    ).await;


    eprintln!("Server initialized. Listening for client connections on stdin/stdout...");

    // Run the server loop. This keeps the server alive and processing messages.
    if let Err(e) = server.run().await {
        eprintln!("Server run loop terminated with error: {:?}", e);
    }

    eprintln!("Server example finished.");
    Ok(())
}