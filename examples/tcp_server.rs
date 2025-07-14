// examples/tcp_server.rs
use rust_mcp_sdk::server::{McpServer, McpServerInternal};
use rust_mcp_sdk::{InputSchema, McpError, Request, Response, ServerCapabilities, Tool, ToolOutputContentBlock, ToolsCallRequestParams, ToolsCallResult};
use serde_json::map::Values;
use serde_json::{json, Value};
use anyhow::Result;
use tokio::sync::Mutex;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use rust_mcp_sdk::server::{RequestHandler, NotificationHandler, ToolExecutionHandler};


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

    Ok(json!({"value": result}))
}

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("Starting MCP Server example (TCP Mode)...");

    let addr = "127.0.0.1:8080"; // Define the address to listen on

    // --- Define Server Capabilities ---
    let server_capabilities = ServerCapabilities {
        logging: Some(json!({})),
        prompts: None,
        resources: None,
        tools: Some(rust_mcp_sdk::ServerToolsCapability { list_changed: Some(true) }),
        completions: None,
        experimental: None,
    };

    // --- Define Tool Metadata ---
    let mut tool_definitions = Vec::new();
    tool_definitions.push(Tool {
        name: "calculator".to_string(),
        title: Some("Simple Arithmetic Calculator".to_string()),
        description: "A tool to perform basic addition and subtraction.".to_string(),
        input_schema: InputSchema::Inline(json!({
            "type": "object", "properties": {"operation": {"type": "string", "enum": ["add", "subtract"]}, "a": {"type": "number"}, "b": {"type": "number"}}, "required": ["operation", "a", "b"]
        })),
        output_schema: Some(InputSchema::Inline(json!({"type": "object", "properties": {"value": {"type": "number"}}}))),
        requires_auth: Some(false), annotations: None, experimental: None,
    });
    tool_definitions.push(Tool {
        name: "weather".to_string(),
        title: Some("Current Weather Fetcher".to_string()),
        description: "Fetches current weather information for a location. (Note: Execution not implemented for this tool)".to_string(),
        input_schema: InputSchema::Inline(json!({
            "type": "object", "properties": {"location": {"type": "string", "description": "City name or zip code"}}, "required": ["location"]
        })),
        output_schema: Some(InputSchema::Inline(json!({"type": "object", "properties": {"temperature": {"type": "number"}}}))),
        requires_auth: Some(false), annotations: None, experimental: None,
    });

    // --- Define Tool Execution Handlers ---
    let mut tool_execution_handler_registrations: Vec<(String, ToolExecutionHandler)> = Vec::new();
    // For calculator
    let server_internal_for_calculator_tool_exec = Arc::new(McpServerInternal {
        // Dummy internal state for cloning into tool handler, as the real one is per-connection.
        // This is a workaround if the tool needs server_internal state during its execution.
        // A better long-term solution might be for tool handlers to return a closure
        // that takes the actual server_internal, or have a factory function.
        // For now, let's pass a basic, dummy internal state if not used.
        transport: Mutex::new(rust_mcp_sdk::server::TransportHandle::Stdio(rust_mcp_sdk::transport::StdioTransport::new())), // This is just for compilation, won't be used
        request_handlers: Mutex::new(HashMap::new()),
        notification_handlers: Mutex::new(HashMap::new()),
        current_protocol_version: "".to_string(),
        negotiated_protocol_version: Mutex::new(None),
        server_capabilities: rust_mcp_sdk::ServerCapabilities { logging: None, prompts: None, resources: None, tools: None, completions: None, experimental: None },
        server_info: rust_mcp_sdk::ServerInfo { name: "".to_string(), title: None, version: None },
        instructions: None,
        tools: Mutex::new(Vec::new()),
        tool_execution_handlers: Mutex::new(HashMap::new()),
    });
    // The `_server_state` in `execute_calculator_tool` is currently not used, so the clone doesn't need to be live.
    // If it *were* used, you'd need a more complex way to share state with the spawned tool handler.
    let handler_for_calculator = Arc::new(move |params, _server_state_arg:Arc<McpServerInternal>| { // _server_state_arg is from the handler signature
        Box::pin(async move {
            execute_calculator_tool(params, _server_state_arg).await // pass it
        }) as Pin<Box<dyn std::future::Future<Output = Result<Value, McpError>> + Send>>
    });
    tool_execution_handler_registrations.push(("calculator".to_string(), handler_for_calculator));


    // --- Define Custom Request Handlers (e.g., custom/ping) ---
    let mut custom_request_handlers: Vec<(String, RequestHandler)> = Vec::new();
    let server_internal_for_custom_ping_handler = Arc::new(rust_mcp_sdk::server::McpServerInternal {
        transport: Mutex::new(rust_mcp_sdk::server::TransportHandle::Stdio(rust_mcp_sdk::transport::StdioTransport::new())),
        request_handlers: Mutex::new(HashMap::new()),
        notification_handlers: Mutex::new(HashMap::new()),
        current_protocol_version: "".to_string(),
        negotiated_protocol_version: Mutex::new(None),
        server_capabilities: rust_mcp_sdk::ServerCapabilities { logging: None, prompts: None, resources: None, tools: None, completions: None, experimental: None },
        server_info: rust_mcp_sdk::ServerInfo { name: "".to_string(), title: None, version: None },
        instructions: None,
        tools: Mutex::new(Vec::new()),
        tool_execution_handlers: Mutex::new(HashMap::new()),
    });
    let handler_for_custom_ping = Arc::new(move |request: Request, _server_state_arg: Arc<rust_mcp_sdk::server::McpServerInternal>| { // _server_state_arg from handler sig
        Box::pin(async move {
            // --- Negotiation Check within the closure ---
            let negotiated_version_guard = _server_state_arg.negotiated_protocol_version.lock().await;
        

            if negotiated_version_guard.is_none() {
                return Ok(rust_mcp_sdk::Response::new_error(
                    Some(request.id),
                    -32600,
                    &"Protocol handshake not complete. Please send 'initialize' request first.".to_string(),
                    None,
                ));
            }
            // --- End Negotiation Check ---
            drop(negotiated_version_guard);
            eprintln!("Server: Received custom/ping request: {:?}", request);
            let current_negotiated_version = _server_state_arg.negotiated_protocol_version.lock().await; // Acquire lock here
            eprintln!("Server: Negotiated protocol version (in custom handler): {:?}", *current_negotiated_version);

            Ok(rust_mcp_sdk::Response::new_success(
                request.id,
                Some(json!({"pong": true, "message": "Hello from custom handler!"}))
            ))
        }) as Pin<Box<dyn std::future::Future<Output = Result<Response, McpError>> + Send>>
    });
    custom_request_handlers.push(("custom/ping".to_string(), handler_for_custom_ping));

    // --- Define Custom Notification Handlers (if any) ---
    let custom_notification_handlers: Vec<(String, NotificationHandler)> = Vec::new(); // Explicit type annotation

    // Start the TCP server
    if let Err(e) = McpServer::start_tcp_server(
        addr,
        "RustMcpSdkExampleServer".to_string(),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities,
        Some("This is a simple Rust MCP server example. It handles initialize, custom/ping, tools/list, and tools/call.".to_string()),
        tool_definitions,
        tool_execution_handler_registrations,
        custom_request_handlers,
        custom_notification_handlers,
    ).await {
        eprintln!("Server: TCP listener terminated with error: {:?}", e);
    }

    eprintln!("Server example finished.");
    Ok(())
}