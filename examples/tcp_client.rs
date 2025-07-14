use rust_mcp_sdk::tcp_transport::TcpTransport;
use rust_mcp_sdk::{Request, Notification, ToolsCallRequestParams};
use serde_json::json;
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:8080";

#[tokio::main]
async fn main() {
    println!("TCP Client: Connecting to server at {}", SERVER_ADDR);
    let stream = TcpStream::connect(SERVER_ADDR)
        .await
        .expect("Failed to connect to server");
    let mut transport = TcpTransport::new(stream);

    // 1. Initialize handshake
    let initialize_request_id = 0;
    let initialize_params = json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {
            "roots": { "listChanged": true },
            "sampling": {},
            "clientInfo": { "name": "TcpClient", "version": "1.0" }
        }
    });
    transport.send(rust_mcp_sdk::McpMessage::Request(Request::new(
        initialize_request_id,
        "initialize",
        Some(initialize_params),
    )))
    .await
    .expect("Failed to send initialize request");
    println!("TCP Client: Sent initialize request.");

    let response = transport.recv().await.expect("Failed to receive initialize response");
    println!("TCP Client: Received initialize response: {:#?}", response);

    // 2. Send initialized notification
    transport
        .send(rust_mcp_sdk::McpMessage::Notification(Notification::new(
            "notifications/initialized",
            None,
        )))
        .await
        .expect("Failed to send initialized notification");
    println!("TCP Client: Sent initialized notification.");

    // 3. List tools
    let tools_list_id = 1;
    transport
        .send(rust_mcp_sdk::McpMessage::Request(Request::new(
            tools_list_id,
            "tools/list",
            Some(json!({})),
        )))
        .await
        .expect("Failed to send tools/list request");
    println!("TCP Client: Sent tools/list request.");

    let tools_list_response = transport.recv().await.expect("Failed to receive tools/list response");
    println!("TCP Client: Received tools/list response: {:#?}", tools_list_response);

    // 4. Call calculator add
    let calc_add_id = 2;
    let calc_add_params = ToolsCallRequestParams {
        tool_name: "calculator".to_string(),
        tool_parameters: Some(json!({ "operation": "add", "a": 5.0, "b": 3.0 })),
    };
    transport
        .send(rust_mcp_sdk::McpMessage::Request(Request::new(
            calc_add_id,
            "tools/call",
            Some(serde_json::to_value(calc_add_params).unwrap()),
        )))
        .await
        .expect("Failed to send calculator add request");
    println!("TCP Client: Sent tools/call (add) request.");

    let calc_add_response = transport.recv().await.expect("Failed to receive calculator add response");
    println!("TCP Client: Received calculator add response: {:#?}", calc_add_response);

    // 5. Call calculator subtract
    let calc_sub_id = 3;
    let calc_sub_params = ToolsCallRequestParams {
        tool_name: "calculator".to_string(),
        tool_parameters: Some(json!({ "operation": "subtract", "a": 10.0, "b": 2.0 })),
    };
    transport
        .send(rust_mcp_sdk::McpMessage::Request(Request::new(
            calc_sub_id,
            "tools/call",
            Some(serde_json::to_value(calc_sub_params).unwrap()),
        )))
        .await
        .expect("Failed to send calculator subtract request");
    println!("TCP Client: Sent tools/call (subtract) request.");

    let calc_sub_response = transport.recv().await.expect("Failed to receive calculator subtract response");
    println!("TCP Client: Received calculator subtract response: {:#?}", calc_sub_response);

    // 6. Custom ping
    let ping_id = 4;
    transport
        .send(rust_mcp_sdk::McpMessage::Request(Request::new(
            ping_id,
            "custom/ping",
            Some(json!({ "message": "hello server" })),
        )))
        .await
        .expect("Failed to send custom/ping request");
    println!("TCP Client: Sent custom/ping request.");

    let ping_response = transport.recv().await.expect("Failed to receive custom/ping response");
    println!("TCP Client: Received custom/ping response: {:#?}", ping_response);

    println!("TCP Client: Done. You can Ctrl+C to exit.");
}