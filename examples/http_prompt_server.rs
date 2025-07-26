
use rust_mcp_sdk::{prompts::{
    //...
    Prompt, PromptArgument, PromptMessage, PromptMessageContent, PromptMessageRole,
    PromptsGetRequestParams, PromptsGetResult, PromptsListResult,
}, ServerCapabilities, ServerPromptsCapability};

use rust_mcp_sdk::server::{McpServer, McpSessionInternal};
use rust_mcp_sdk::{
    InputSchema, McpError, Request, Response, Tool,
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
    // ...

    eprintln!("Starting MCP Simple HTTP Server example...");

    const ADDR: &str = "127.0.0.1:8080";

    let server_capabilities = ServerCapabilities {
        logging: None,//Some(json!({})),
        prompts: Some(ServerPromptsCapability { list_changed: Some(true) }),
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

    // --- Register Calculator Tool ---
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

    // 2. Define and add a sample prompt
    let code_review_prompt = Prompt {
        name: "code_review".to_string(),
        title: Some("Review Code".to_string()),
        description: Some("Asks an LLM to review a snippet of code for quality and improvements.".to_string()),
        arguments: Some(vec![PromptArgument {
            name: "code".to_string(),
            description: Some("The code snippet to be reviewed.".to_string()),
            required: Some(true),
        }]),
    };
    app_server.add_prompt(code_review_prompt).await;

    // 3. Register a handler for 'prompts/list'
    let prompts_list_handler = Arc::new(|request: Request, _session, server:Arc<McpServer>| {
        Box::pin(async move {
            let prompts = server.prompt_definitions.lock().await;
            let result = PromptsListResult {
                prompts: prompts.clone(),
                next_cursor: None,
            };
            eprintln!("Server: Responding to prompts/list with {} prompts.", result.prompts.len());
            Ok(Response::new_success(request.id, Some(serde_json::to_value(result).unwrap())))
        }) as Pin<Box<(dyn Future<Output = Result<rust_mcp_sdk::Response, McpError>> + Send + 'static)>>
    });
    app_server.register_request_handler("prompts/list", prompts_list_handler).await;

    // 4. Register a handler for 'prompts/get'
    let prompts_get_handler = Arc::new(|request: Request, _session, server:Arc<McpServer>| {
        Box::pin(async move {
            let params: PromptsGetRequestParams = serde_json::from_value(request.params.clone().unwrap_or_default())?;
            let prompts = server.prompt_definitions.lock().await;

            // Find the requested prompt
            if let Some(prompt_template) = prompts.iter().find(|p| p.name == params.name) {
                // In a real implementation, you would use `params.arguments` to render a dynamic message.
                // For now, we'll return a hardcoded message for our example prompt.
                let code_to_review = params.arguments.as_ref()
                    .and_then(|args| args.get("code"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("[No code provided]");

                let message_text = format!("Please review the following code for quality, bugs, and potential improvements:\n\n```\n{}\n```", code_to_review);

                let result = PromptsGetResult {
                    description: Some(format!("Executing prompt: {}", prompt_template.title.as_deref().unwrap_or_default())),
                    messages: vec![PromptMessage {
                        role: PromptMessageRole::User,
                        content: PromptMessageContent::Text { text: message_text },
                        annotations: None,
                    }],
                };
                eprintln!("Server: Responding to prompts/get for '{}'.", params.name);
                Ok(Response::new_success(request.id, Some(serde_json::to_value(result).unwrap())))
            } else {
                Err(McpError::ProtocolError(format!("Prompt '{}' not found.", params.name)))
            }
        }) as Pin<Box<(dyn Future<Output = Result<rust_mcp_sdk::Response, McpError>> + Send + 'static)>>
    });
    app_server.register_request_handler("prompts/get", prompts_get_handler).await;

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


    // ... (rest of main function, including sse_test_handler and start_listener)
}