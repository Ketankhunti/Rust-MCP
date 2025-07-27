// 1. Import the procedural macros and necessary types
use rust_mcp_sdk::{
    server::{McpServer, RequestHandler},
    McpError, Notification, PromptMessage, PromptMessageContent, PromptMessageRole, Request,
    Resource, ResourceContents, ServerCapabilities, ServerResourcesCapability,
    Annotations, ResourcesUpdatedParams, Response,
};
use mcp_macros::{prompt, resource, tool};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::{sync::Mutex, time};
use std::{pin::Pin, sync::Arc, time::Duration};
use chrono::{ Utc};
use lazy_static::lazy_static;
use serde_json::{json, Value};

// --- Global State for the Dynamic Resource ---
lazy_static! {
    static ref VISIT_COUNTER: Mutex<i32> = Mutex::new(0);
}

// --- Tool Definition (using the macro) ---
#[tool(
    title = "Simple Calculator",
    description = "A tool for adding or subtracting two numbers."
)]
pub async fn calculator(operation: String, a: f64, b: f64) -> Result<f64, String> {
    eprintln!("Executing 'calculator' tool with: {} {} {}", operation, a, b);
    match operation.as_str() {
        "add" => Ok(a + b),
        "subtract" => Ok(a - b),
        _ => Err(format!("Invalid operation: '{}'", operation)),
    }
}

// --- Prompt Definition (using the macro) ---
#[prompt(
    name = "code_review",
    title = "Review Code Snippet",
    description = "Generates a prompt for an LLM to review a piece of code."
)]
pub async fn review_code(code: String) -> Result<Vec<PromptMessage>, String> {
    eprintln!("Executing 'review_code' prompt for code snippet.");
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

// --- Dynamic Resource Definition (using the macro) ---
#[resource(
    uri = "mcp://server/visitors",
    name = "visitor_count",
    title = "Visitor Count",
    description = "Provides the current number of visitors.",
    mime_type = "text/plain"
)]
pub async fn visitor_count(uri: String) -> Result<ResourceContents, String> {
    eprintln!("Executing dynamic resource handler for URI: {}", uri);
    let count = *VISIT_COUNTER.lock().await;
    let now = Utc::now();
    let content_text = format!("Current visitor count: {}", count);
    Ok(ResourceContents {
        resource: Resource {
            uri,
            name: "visitor_count".to_string(),
            title: Some("Visitor Count".to_string()),
            description: Some("Provides the current number of visitors.".to_string()),
            mime_type: Some("text/plain".to_string()),
            size: Some(content_text.len() as u64),
            annotations: Some(Annotations {
                last_modified: Some(now.to_rfc3339()),
                ..Default::default()
            }),
        },
        text: Some(content_text),
        blob: None,
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    eprintln!("Starting MCP Macro Tool Server example...");

    const ADDR: &str = "127.0.0.1:8080";

    let server_capabilities = ServerCapabilities {
        logging: None,
        prompts: Some(rust_mcp_sdk::ServerPromptsCapability { list_changed: Some(true) }),
        resources: Some(ServerResourcesCapability {
            subscribe: Some(true),
            list_changed: Some(true),
        }),
        tools: Some(rust_mcp_sdk::ServerToolsCapability { list_changed: Some(true) }),
        completions: None,
        experimental: None,
    };

    let app_server = Arc::new(McpServer::new(
        "RustMcpSdkMacroServer".to_string(),
        Some("My Macro Server".to_string()),
        Some(env!("CARGO_PKG_VERSION").to_string()),
        server_capabilities.clone(),
        Some("A server demonstrating automatic feature registration via macros.".to_string()),
    )
    .await
    );
    // --- Discover and register all macro-defined features ---
    app_server.register_discovered_tools().await;
    app_server.register_discovered_prompts().await;
    app_server.register_discovered_resources().await;


    // --- Spawn a background task to automatically update the resource ---
    let server_clone = app_server.clone();
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(5));
        loop {
            interval.tick().await; // Wait for the next 5-second interval

            let mut count = VISIT_COUNTER.lock().await;
            *count += 1;
            
            eprintln!("TIMER TASK: Incrementing visitor count to {}.", *count);

            let uri_to_update = "mcp://server/visitors";
            
            let updated_params = ResourcesUpdatedParams {
                uri: uri_to_update.to_string(),
                title: Some(format!("New Visitor Count: {}", *count)),
                ..Default::default()
            };
            
            // Call the notification function on the server instance
            server_clone.notify_resource_updated(uri_to_update, updated_params).await;
        }
    });

    // --- Start the server ---
    if let Err(e) = rust_mcp_sdk::server::McpHttpServer::start_listener(ADDR, app_server).await {
        eprintln!("HTTP Server: Listener terminated with error: {:?}", e);
    }

    eprintln!("HTTP Server example finished.");
    Ok(())
}
