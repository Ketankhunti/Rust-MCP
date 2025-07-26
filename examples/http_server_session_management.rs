use rust_mcp_sdk::server::{McpServer, McpSessionInternal};
use rust_mcp_sdk::{
    InputSchema, McpError, ServerCapabilities, Tool,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::future::Future;
use std::pin::Pin;
use rust_mcp_sdk::server::ToolExecutionHandler;

async fn execute_calculator_tool(
    params: Value,
    _session_internal: Arc<McpSessionInternal>,
    _app_config: Arc<McpServer>,
) -> Result<Value, McpError> {
    eprintln!("Server: Executing calculator tool with params: {:?}", params);

    let operation = params
        .get("operation")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::ProtocolError("Calculator: 'operation' missing or invalid".to_string()))?;
    let a = params
        .get("a")
        .and_then(Value::as_f64)
        .ok_or_else(|| McpError::ProtocolError("Calculator: 'a' missing or invalid".to_string()))?;
    let b = params
        .get("b")
        .and_then(Value::as_f64)
        .ok_or_else(|| McpError::ProtocolError("Calculator: 'b' missing or invalid".to_string()))?;

    let result = match operation {
        "add" => a + b,
        "subtract" => a - b,
        _ => return Err(McpError::ProtocolError(format!(
            "Calculator: Unknown operation '{}'",
            operation
        ))),
    };

    eprintln!("Server: Calculator raw result: {}", result);

    Ok(json!({ "value": result }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("Starting MCP Simple HTTP Server example...");

    const ADDR: &str = "127.0.0.1:8080";

    let server_capabilities = ServerCapabilities {
        logging: Some(json!({})),
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

    app_server
        .add_tool(Tool {
            name: "calculator".to_string(),
            title: Some("Simple Arithmetic Calculator".to_string()),
            description: "A tool to perform basic addition and subtraction.".to_string(),
            input_schema: InputSchema::Inline(json!({
                "type": "object",
                "properties": {
                    "operation": { "type": "string", "enum": ["add", "subtract"] },
                    "a": { "type": "number" },
                    "b": { "type": "number" }
                },
                "required": ["operation", "a", "b"]
            })),
            output_schema: Some(InputSchema::Inline(
                json!({ "type": "object", "properties": { "value": { "type": "number" } } }),
            )),
            requires_auth: Some(false),
            annotations: None,
            experimental: None,
        })
        .await;

    let handler_for_calculator =
        Arc::new(move |params, session_internal_arg: Arc<McpSessionInternal>, app_config_arg: Arc<McpServer>| {
            Box::pin(async move {
                execute_calculator_tool(params, session_internal_arg, app_config_arg).await
            }) as Pin<Box<dyn Future<Output = Result<Value, McpError>> + Send>>
        }) as ToolExecutionHandler;
    app_server
        .register_tool_execution_handler("calculator", handler_for_calculator)
        .await;

    if let Err(e) = rust_mcp_sdk::server::McpHttpServer::start_listener(
        ADDR,
        Arc::new(app_server),
    )
    .await
    {
        eprintln!("HTTP Server: Listener terminated with error: {:?}", e);
    }

    eprintln!("HTTP Server example finished.");
    Ok(())
}