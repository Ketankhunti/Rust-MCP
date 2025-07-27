use std::sync::Arc;

// Bring the procedural macro into scope
use mcp_macros::{tool,prompt};
use rust_mcp_sdk::{prompts::{PromptMessage, PromptMessageContent}, ServerCapabilities, ServerPromptsCapability};
use mcp_sdk_types::PromptMessageRole;

#[tool(
    title = "Simple Calculator",
    description = "A tool for adding or subtracting two numbers."
)]
pub async fn calculator(operation: Option<String>, a: f64, b: f64) -> Result<f64, String> {
    eprintln!("Executing 'calculator' tool with: {:#?} {} {}", operation.as_ref(), a, b);
    match operation.as_ref().unwrap().as_str() {
        "add" => Ok(a + b),
        "subtract" => Ok(a - b),
        _ => Err(format!("Invahlid operation: '{:#?}'", operation)),
    }
}

#[prompt(
    name = "code_review",
    title = "Review Code Snippet",
    description = "Generates a prompt for an LLM to review a piece of code."
)]
pub async fn review_code(code: String) -> Result<Vec<PromptMessage>, String> {
    eprintln!("Executing 'review_code' prompt for code snippet.");

    // The function's job is to construct the list of messages.
    let message_text = format!(
        "Please act as an expert code reviewer. Analyze the following code snippet for quality, \
        potential bugs, style issues, and suggest improvements. Provide your feedback clearly. \
        \n\n```\n{}\n```",
        code
    );

    let messages = vec![PromptMessage {
        role: PromptMessageRole::User,
        content: PromptMessageContent::Text { text: message_text },
        annotations: None,
    }];

    Ok(messages)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("Starting MCP Simple HTTP Server example...");

    const ADDR: &str = "127.0.0.1:8080";

    let server_capabilities = ServerCapabilities {
        logging: None,//Some(json!({})),
        prompts: Some(ServerPromptsCapability{ list_changed : Some(true)}),
        resources: None,
        tools: Some(rust_mcp_sdk::ServerToolsCapability {
            list_changed: Some(true),
        }),
        completions: None,
        experimental: None,
    };

    let app_server = rust_mcp_sdk::server::McpServer::new(
        "RustMcpSdkSimpleServer".to_string(),
        Some("MyServer".to_string()),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities.clone(),
        Some("This is a simple Rust MCP server example. It handles initialize and calculator.".to_string()),
    )
    .await;

    // --- Discover and register all tools from the macro ---
    app_server.register_discovered_tools().await;
    app_server.register_discovered_prompts().await;

    // ... (You can add prompts and other handlers here as before) ...

    if let Err(e) = rust_mcp_sdk::server::McpHttpServer::start_listener(ADDR, Arc::new(app_server)).await {
        eprintln!("HTTP Server: Listener terminated with error: {:?}", e);
    }

    eprintln!("HTTP Server example finished.");
    Ok(())
}
