use rust_mcp_sdk::{ClientCapabilities, ClientInfo, ClientRootsCapability, InitializeResult, Notification, Request, Response, ServerCapabilities, ServerInfo, ServerPromptsCapability, ServerResourcesCapability, ServerToolsCapability};
use serde_json::json;
use serde_json::Value;

    // ... (Your existing tests like test_request_serialization, etc.) ...

    #[test]
    fn test_initialize_request_serialization_full() {
        let capabilities = ClientCapabilities {
            roots: Some(ClientRootsCapability { list_changed: Some(true) }),
            sampling: Some(json!({})),
            elicitation: None,
            experimental: Some(json!({})),
        };
        let client_info = Some(ClientInfo {
            name: "MyClient".to_string(),
            title: Some("My Client Display".to_string()),
            version: Some("1.0.0".to_string()),
        });
        let request = Request::new_initialize(
            1u64.into(),
            "2025-06-18".to_string(),
            capabilities,
            client_info,
        );
        let json_str = request.to_json().unwrap();
        // println!("Full Initialize Request: {}", json_str); // For debugging
        let expected_json = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {
                    "roots": { "listChanged": true },
                    "sampling": {},
                    "experimental": {}
                },
                "clientInfo": {
                    "name": "MyClient",
                    "title": "My Client Display",
                    "version": "1.0.0"
                }
            }
        });
        assert_eq!(serde_json::from_str::<Value>(&json_str).unwrap(), expected_json);
    }

    #[test]
    fn test_initialize_response_serialization_full() {
        let capabilities = ServerCapabilities {
            logging: Some(json!({})),
            prompts: Some(ServerPromptsCapability { list_changed: Some(true) }),
            resources: Some(ServerResourcesCapability { subscribe: Some(true), list_changed: Some(true) }),
            tools: Some(ServerToolsCapability { list_changed: Some(true) }),
            completions: Some(json!({})),
            experimental: None,
        };
        let server_info = Some(ServerInfo {
            name: "MyServer".to_string(),
            title: Some("My Server Display".to_string()),
            version: Some("2.0.0".to_string()),
        });
        let result = InitializeResult {
            protocol_version: "2025-06-18".to_string(),
            capabilities,
            server_info,
            instructions: Some("Welcome!".to_string()),
        };
        let response = Response::new_initialize_success(1u64.into(), result);
        let json_str = response.to_json().unwrap();
        // println!("Full Initialize Response: {}", json_str); // For debugging
        let expected_json = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2025-06-18",
                "capabilities": {
                    "logging": {},
                    "prompts": { "listChanged": true },
                    "resources": { "subscribe": true, "listChanged": true },
                    "tools": { "listChanged": true },
                    "completions": {}
                },
                "serverInfo": {
                    "name": "MyServer",
                    "title": "My Server Display",
                    "version": "2.0.0"
                },
                "instructions": "Welcome!"
            }
        });
        assert_eq!(serde_json::from_str::<Value>(&json_str).unwrap(), expected_json);
    }

#[test]
fn test_initialized_notification_serialization() {
    let notification = Notification::new_initialized();
    let json_str = notification.to_json().unwrap();
    // println!("Initialized Notification: {}", json_str);
    let expected_json = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    assert_eq!(serde_json::from_str::<Value>(&json_str).unwrap(), expected_json);
}

#[test]
fn test_unsupported_protocol_error_serialization() {
    let error_resp = Response::new_unsupported_protocol_error(
        "my_req_id".into(),
        "1.0.0".to_string(),
        vec!["2025-06-18".to_string(), "2024-11-05".to_string()],
    );
    let json_str = error_resp.to_json().unwrap();
    // println!("Unsupported Protocol Error: {}", json_str);
    let expected_json = json!({
        "jsonrpc": "2.0",
        "id": "my_req_id",
        "error": {
            "code": -32602,
            "message": "Unsupported protocol version",
            "data": {
                "supported": ["2025-06-18", "2024-11-05"],
                "requested": "1.0.0"
            }
        }
    });
    assert_eq!(serde_json::from_str::<Value>(&json_str).unwrap(), expected_json);
}