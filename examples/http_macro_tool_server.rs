use std::sync::Arc;

use chrono::Utc;
// Bring the procedural macro into scope
use mcp_macros::{tool,prompt,resource};
use rust_mcp_sdk::{prompts::{PromptMessage, PromptMessageContent}, Annotations, PromptMessageRole, Resource, ResourceContents, ServerCapabilities, ServerPromptsCapability, ServerResourcesCapability};


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

#[tool(
    title = "Simple Multiplier",
    description = "A tool for multiplying two numbers."
)]
pub async fn multiplier(operation: Option<String>, a: f64, b: f64) -> Result<f64, String> {
    eprintln!("Executing 'calculator' tool with: {:#?} {} {}", operation.as_ref(), a, b);
    match operation.as_ref().unwrap().as_str() {
        "multiply" => Ok(a * b),
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

#[resource(
    uri = "mcp://status/current",
    name = "current_status",
    title = "Current Server Status",
    description = "Provides the current server time and status.",
    mime_type = "text/plain"
)]
pub async fn get_current_status(uri: String) -> Result<ResourceContents, String> {
    eprintln!("Executing dynamic resource handler for URI: {}", uri);

    let now = Utc::now();
    let content_text = format!(
        "Server Status Report\nTimestamp: {}\nStatus: OK",
        now.to_rfc3339()
    );

    let contents = ResourceContents {
        resource: Resource {
            uri, // Use the URI passed into the handler
            name: "current_status".to_string(),
            title: Some("Current Server Status".to_string()),
            description: Some("Provides the current server time and status.".to_string()),
            mime_type: Some("text/plain".to_string()),
            size: Some(content_text.len() as u64),
            annotations: Some(Annotations {
                last_modified: Some(now.to_rfc3339()),
                audience: None,
                priority: None
            }),
        },
        text: Some(content_text),
        blob: None,
    };

    Ok(contents)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() -> anyhow::Result<()> {
    eprintln!("Starting MCP Simple HTTP Server example...");

    const ADDR: &str = "127.0.0.1:8080";

    let server_capabilities = ServerCapabilities {
        logging: None,//Some(json!({})),
        prompts: Some(ServerPromptsCapability{ list_changed : Some(true)}),
        resources: Some(ServerResourcesCapability {
            subscribe: Some(false),
            list_changed: Some(true),
        }),
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

   
    app_server.register_discovered_tools().await;
    app_server.register_discovered_prompts().await;
    app_server.register_discovered_resources().await;

    if let Err(e) = rust_mcp_sdk::server::McpHttpServer::start_listener(ADDR, Arc::new(app_server)).await {
        eprintln!("HTTP Server: Listener terminated with error: {:?}", e);
    }

    eprintln!("HTTP Server example finished.");
    Ok(())
}
