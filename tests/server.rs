// src/server.rs

use std::{collections::HashMap, sync::Arc};
use futures::future::BoxFuture;
use tokio::sync::Mutex; // Use tokio's Mutex for async contexts

use rust_mcp_sdk::{transport::StdioTransport, InitializeRequestParams, InitializeResult, McpError, McpMessage, Notification, Request, Response, ServerCapabilities, ServerInfo};
use serde_json::json;
use anyhow::Result; // Used in main functions, good to keep for consistency in tests if needed

// --- (Your existing RequestHandler, NotificationHandler, McpServerInternal, McpServer structs and impl blocks) ---
// (Copy paste the full content of your `src/server.rs` file here before adding the test block below)


#[cfg(test)]
mod tests {
    use super::*;
    use rust_mcp_sdk::{server::{McpServer, McpServerInternal}, ClientCapabilities, ClientInfo, ClientRootsCapability, RequestId}; // Import RequestId from crate root
    use std::time::Duration; // For tokio::time::sleep

    // Helper to create a minimal McpServerInternal instance for tests
    async fn create_test_server_internal() -> Arc<McpServerInternal> {
        Arc::new(McpServerInternal {
            transport: Mutex::new(StdioTransport::new()), // StdioTransport.new() spawns tasks, but within tokio::test it's fine.
            request_handlers: Mutex::new(HashMap::new()),
            notification_handlers: Mutex::new(HashMap::new()),
            current_protocol_version: "2025-06-18".to_string(),
            negotiated_protocol_version: Mutex::new(None),
            server_capabilities: ServerCapabilities {
                logging: Some(json!({})),
                prompts: None,
                resources: None,
                tools: None,
                completions: None,
                experimental: None,
            },
            server_info: ServerInfo {
                name: "TestServer".to_string(),
                title: None,
                version: Some("1.0.0".to_string()),
            },
            instructions: Some("Test instructions.".to_string()),
        })
    }

    #[tokio::test]
    async fn test_register_request_handler_success() {
        let server = McpServer::new(
            "TestServer".to_string(),
            None,
            ServerCapabilities { logging: None, prompts: None, resources: None, tools: None, completions: None, experimental: None },
            None,
        ).await;

        let internal_clone = server.internal.clone();
        server.register_request_handler(
            "test_method",
            Arc::new(move |_, _| Box::pin(async move { Ok(Response::new_success(RequestId::Number(0), None)) }))
        ).await;

        let handlers = server.internal.request_handlers.lock().await;
        assert!(handlers.contains_key("test_method"));
    }

    #[tokio::test]
    #[should_panic(expected = "Request handler for method 'test_method' already registered.")]
    async fn test_register_request_handler_panic_on_duplicate() {
        let server = McpServer::new(
            "TestServer".to_string(),
            None,
            ServerCapabilities { logging: None, prompts: None, resources: None, tools: None, completions: None, experimental: None },
            None,
        ).await;

        let internal_clone_1 = server.internal.clone();
        server.register_request_handler(
            "test_method",
            Arc::new(move |_, _| Box::pin(async move { Ok(Response::new_success(RequestId::Number(0), None)) }))
        ).await;

        let internal_clone_2 = server.internal.clone();
        // This should panic
        server.register_request_handler(
            "test_method",
            Arc::new(move |_, _| Box::pin(async move { Ok(Response::new_success(RequestId::Number(1), None)) }))
        ).await;
    }

    #[tokio::test]
    async fn test_register_notification_handler_success() {
        let server = McpServer::new(
            "TestServer".to_string(),
            None,
            ServerCapabilities { logging: None, prompts: None, resources: None, tools: None, completions: None, experimental: None },
            None,
        ).await;

        let internal_clone = server.internal.clone();
        server.register_notification_handler(
            "test_notification",
            Arc::new(move |_, _| Box::pin(async move { Ok(()) }))
        ).await;

        let handlers = server.internal.notification_handlers.lock().await;
        assert!(handlers.contains_key("test_notification"));
    }

    #[tokio::test]
    #[should_panic(expected = "Notification handler for method 'test_notification' already registered.")]
    async fn test_register_notification_handler_panic_on_duplicate() {
        let server = McpServer::new(
            "TestServer".to_string(),
            None,
            ServerCapabilities { logging: None, prompts: None, resources: None, tools: None, completions: None, experimental: None },
            None,
        ).await;

        let internal_clone_1 = server.internal.clone();
        server.register_notification_handler(
            "test_notification",
            Arc::new(move |_, _| Box::pin(async move { Ok(()) }))
        ).await;

        let internal_clone_2 = server.internal.clone();
        // This should panic
        server.register_notification_handler(
            "test_notification",
            Arc::new(move |_, _| Box::pin(async move { Ok(()) }))
        ).await;
    }

    #[tokio::test]
    async fn test_handle_initialize_success() {
        let server_internal = create_test_server_internal().await;
        let request_id = RequestId::Number(123);
        let client_info = ClientInfo {
            name: "TestClient".to_string(),
            title: Some("ClientTitle".to_string()),
            version: Some("1.0.0".to_string()),
        };
        let client_capabilities = ClientCapabilities {
            roots: Some(ClientRootsCapability { list_changed: Some(true) }),
            sampling: Some(json!({})),
            elicitation: None,
            experimental: None,
        };

        let request_params = InitializeRequestParams {
            protocol_version: "2025-06-18".to_string(),
            capabilities: client_capabilities.clone(),
            client_info: Some(client_info.clone()),
        };

        let request = Request::new(
            request_id.clone(),
            "initialize",
            Some(serde_json::to_value(request_params).unwrap())
        );

        // Call the handler directly
        let response = McpServer::handle_initialize(request, server_internal.clone()).await.unwrap();

        // Assert response structure and content
        assert_eq!(response.id.unwrap(), request_id);
        assert!(response.result.is_some());
        assert!(response.error.is_none());

        let result: InitializeResult = serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.protocol_version, "2025-06-18");
        assert_eq!(result.server_info.unwrap().name, "TestServer");
        assert_eq!(result.capabilities.logging, Some(json!({}))); // From test server capabilities
        assert_eq!(result.instructions, Some("Test instructions.".to_string()));

        // Assert internal state update
        let negotiated_version = server_internal.negotiated_protocol_version.lock().await;
        assert_eq!(*negotiated_version, Some("2025-06-18".to_string()));
    }

    #[tokio::test]
    async fn test_handle_initialize_protocol_mismatch() {
        let server_internal = create_test_server_internal().await;
        // server_internal.current_protocol_version = "2024-11-05".to_string(); // Simulate older server version

        let request_id = RequestId::Number(124);
        let request_params = InitializeRequestParams {
            protocol_version: "2024-11-05".to_string(), // Client requests newer
            capabilities: ClientCapabilities { roots: None, sampling: None, elicitation: None, experimental: None },
            client_info: None,
        };
        let request = Request::new(
            request_id.clone(),
            "initialize",
            Some(serde_json::to_value(request_params).unwrap())
        );

        let response = McpServer::handle_initialize(request, server_internal.clone()).await.unwrap();

        assert_eq!(response.id.unwrap(), request_id);
        assert!(response.result.is_some());
        let result: InitializeResult = serde_json::from_value(response.result.unwrap()).unwrap();
        // Server should respond with its own version
        assert_eq!(result.protocol_version, "2025-06-18");

        // Internal state should reflect server's version
        let negotiated_version = server_internal.negotiated_protocol_version.lock().await;
        assert_eq!(*negotiated_version, Some("2025-06-18".to_string()));
    }

    #[tokio::test]
    async fn test_handle_initialized_notification() {
        let server_internal = create_test_server_internal().await;
        let notification = Notification::new_initialized();

        // Call the handler directly
        let result = McpServer::handle_initialized_notification(notification, server_internal.clone()).await;

        assert!(result.is_ok());
        // No direct state change to assert for this specific handler, but in a real app,
        // you might assert an `is_initialized` flag changed on `server_internal`.
    }
}