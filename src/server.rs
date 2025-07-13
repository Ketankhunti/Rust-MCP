use std::{collections::HashMap, sync::{Arc}};

use futures::future::BoxFuture;
use serde_json::Value;
use tokio::sync::Mutex;

use crate::{transport::StdioTransport, InitializeRequestParams, InitializeResult, McpError, McpMessage, Notification, Request, Response, ServerCapabilities, ServerInfo, Tool, ToolOutputContentBlock, ToolsCallRequestParams, ToolsCallResult, ToolsListRequestParams, ToolsListResult};

type RequestHandler = Arc<
    dyn Fn(Request, Arc<McpServerInternal>) -> BoxFuture<'static, Result<Response, McpError>>
    + Send
    + Sync,
>;

type NotificationHandler = Arc<
    dyn Fn(Notification, Arc<McpServerInternal>) -> BoxFuture<'static, Result<(), McpError>>
    + Send
    + Sync,
>;

type ToolExecutionHandler = Arc<
    dyn Fn(Value, Arc<McpServerInternal>) -> BoxFuture<'static, Result<Value, McpError>>
    + Send
    + Sync
>; 

pub struct McpServerInternal {
    pub transport: Mutex<StdioTransport>,
    pub request_handlers: Mutex<HashMap<String, RequestHandler>>,
    pub notification_handlers: Mutex<HashMap<String, NotificationHandler>>,
    pub current_protocol_version: String,
    pub negotiated_protocol_version: Mutex<Option<String>>,
    pub server_capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
    pub instructions: Option<String>,
    pub tools: Mutex<Vec<Tool>>,
    pub tool_execution_handlers: Mutex<HashMap<String, ToolExecutionHandler>>,
}

#[derive(Clone)]
pub struct McpServer {
    pub internal: Arc<McpServerInternal>,
}

impl McpServer {
    const LATEST_SUPPORTED_PROTOCOL_VERSION: &'static str = "2025-06-18";
    pub async fn new(
        server_name: String,
        server_version: Option<String>,
        mut server_capabilities: ServerCapabilities,
        instructions: Option<String>,
    ) -> Self {
        let server_info = ServerInfo {
            name: server_name,
            title: None, // Can be set later or via constructor arg
            version: server_version,
        };

        // Ensure tools capability is advertised if not already present
        if server_capabilities.tools.is_none() {
             server_capabilities.tools = Some(crate::ServerToolsCapability { list_changed: Some(true) });
        }

        let internal = Arc::new(McpServerInternal {
            transport: Mutex::new(StdioTransport::new()),
            request_handlers: Mutex::new(HashMap::new()),
            notification_handlers: Mutex::new(HashMap::new()),
            current_protocol_version: Self::LATEST_SUPPORTED_PROTOCOL_VERSION.to_string(),
            negotiated_protocol_version: Mutex::new(None),
            server_capabilities,
            server_info: server_info,
            instructions,
            tools: Mutex::new(Vec::new()), // Initialize with an empty vector
            tool_execution_handlers: Mutex::new(HashMap::new()), // Initialize with an empty map
        });
       let server = Self { internal };

       let internal_for_init = server.internal.clone();
       server.register_request_handler(
        "initialize", 
           Arc::new(move |request, _| {
               Box::pin(McpServer::handle_initialize(request, internal_for_init.clone()))
           })
       ).await;

       let internal_for_initialized: Arc<McpServerInternal> = server.internal.clone();
        server.register_notification_handler(
            "notifications/initialized",
            Arc::new(move |notification, _| {
                Box::pin(McpServer::handle_initialized_notification(notification, internal_for_initialized.clone()))
            })
        ).await;

        // Refister tools/list
        let  internal_for_tools_list = server.internal.clone();

        server.register_request_handler(
            "tools/list",
            Arc::new(move |request, _| {
            let internal = internal_for_tools_list.clone();
            Box::pin(async move {
                McpServer::handle_tools_list(request, internal).await
            })
        })
        )
        .await;

        // Register the `tools/call` request handler
        let internal_for_tools_call = server.internal.clone();
        server.register_request_handler(
            "tools/call",
            Arc::new(move |request, _| {
                let internal = internal_for_tools_call.clone();
                Box::pin(async move {
                    McpServer::handle_tools_call(request, internal).await
                })
            })
        ).await;

        server

    }

    pub async fn register_request_handler(&self, method: &str, handler : RequestHandler ) {
        let mut handlers = self.internal.request_handlers.lock().await;
        if handlers.insert(method.to_string(), handler).is_some() {
            panic!("Request handler for method '{}' already registered.", method);
        }
    }
    
    pub async fn register_notification_handler(&self, method: &str, handler: NotificationHandler) {
        let mut handlers = self.internal.notification_handlers.lock().await;
        if handlers.insert(method.to_string(), handler).is_some() {
            panic!("Notification handler for method '{}' already registered.", method);
        }
    }

    pub async fn add_tool(&self, tool: Tool) {
        let mut tools_guard = self.internal.tools.lock().await;
        tools_guard.push(tool);
       eprintln!("Server: Added tool definition '{}'. Current tools: {}",
                  tools_guard.last().unwrap().name, tools_guard.len());
    }

    pub async fn register_tool_execution_handler(&self, tool_name: &str, handler: ToolExecutionHandler) {
        let mut handlers = self.internal.tool_execution_handlers.lock().await;
        if handlers.insert(tool_name.to_string(), handler).is_some() {
            panic!("Tool execution handler for '{}' already registered.", tool_name);
        }
        eprintln!("Server: Registered execution handler for tool '{}'.", tool_name);
    }
    
     pub async fn run(&self) -> Result<(), McpError> {
        eprintln!("Server: run loop started."); // ADD THIS
        loop {
            let msg_result = {
                let mut transport_guard = self.internal.transport.lock().await;
                eprintln!("Server: run loop acquiring transport lock for recv, waiting for message..."); // ADD THIS
                let res = transport_guard.recv().await;
                eprintln!("Server: run loop released transport lock for recv. Result: {:?}", res); // ADD THIS (Improved logging for Ok)
                res
            };

            match msg_result {
                Ok(message) => {
                    eprintln!("Server: run loop received message, handling..."); // ADD THIS
                    self.handle_message(message).await?;
                    eprintln!("Server: run loop finished handling message."); // ADD THIS
                },
                Err(McpError::Io(e)) if e.kind() == std::io::ErrorKind::ConnectionAborted => {
                    eprintln!("Server: Transport connection aborted (EOF), server exiting gracefully."); // CLARIFIED MESSAGE
                    break;
                },
                Err(e) => {
                    eprintln!("Server: Error receiving message: {:?}", e);
                    eprintln!("Server: run loop encountered critical error, breaking."); // ADD THIS
                    break;
                }
            }
        }
        eprintln!("Server: run loop finished."); // ADD THIS
        Ok(())
    }


    async fn handle_message(&self, message: McpMessage) -> Result<(), McpError> {
        match message {
            McpMessage::Request(request) => {
                // ... (existing Request handling logic - no changes here) ...
                let method = request.method.clone();
                let id = request.id.clone();
                let handlers = self.internal.request_handlers.lock().await;

                if let Some(handler) = handlers.get(&method) {
                    let response_result = handler(request, self.internal.clone()).await;
                    let response = match response_result {
                        Ok(res) => res,
                        Err(err) => {
                            eprintln!("Server: Handler for '{}' returned an error: {:?}", method, err);
                            Response::new_error(
                                Some(id), // ID is present for requests
                                -32000,
                                &format!("Internal server error: {}", err),
                                None,
                            )
                        }
                    };
                    self.internal.transport.lock().await.send(McpMessage::Response(response)).await?;
                } else {
                    eprintln!("Server: Received unhandled request method: {}", method);
                    let error_response = Response::new_error(
                        Some(id), // ID is present for requests
                        -32601, // JSON-RPC standard "Method not found" error code
                        &format!("Method '{}' not found", method),
                        None,
                    );
                    self.internal.transport.lock().await.send(McpMessage::Response(error_response)).await?;
                }
            }
            McpMessage::Notification(notification) => {
                let method = notification.method.clone();
                let notification_handlers = self.internal.notification_handlers.lock().await;

                if let Some(handler) = notification_handlers.get(&method) {
                    // This is a known notification, execute its handler
                    if let Err(e) = handler(notification, self.internal.clone()).await {
                        eprintln!("Server: Handler for notification '{}' returned an error: {:?}", method, e);
                    }
                } else {
                    // This is an UNHANDLED notification.
                    // Now, check if a REQUEST handler exists for this method name.
                    let request_handlers = self.internal.request_handlers.lock().await; // Acquire lock for request_handlers

                    if request_handlers.contains_key(&method) {
                        // This method is known as a REQUEST, but it was received as a NOTIFICATION (missing ID).
                        // According to MCP's strict "Request MUST have ID", this is an Invalid Request from a practical standpoint.
                        eprintln!("Server: Received method '{}' as a Notification, but it's configured as a Request handler. Informing client.", method);

                        // Send an "Invalid Request" error response with id: null
                        let error_response = Response::new_error(
                            None, // id: null, as per JSON-RPC spec for errors when original ID is missing/unidentifiable
                            -32600, // JSON-RPC standard "Invalid Request" error code
                            &format!("Method '{}' expects an ID and a response, but was received as a Notification (missing 'id' field).", method),
                            None,
                        );
                        self.internal.transport.lock().await.send(McpMessage::Response(error_response)).await?;
                    } else {
                        // Method is not known at all, neither as a request nor a notification
                        eprintln!("Server: Received completely unknown notification method: {}", method);
                    }
                }
            }
            McpMessage::Response(response) => {
                // ... (existing Response handling logic - no changes here) ...
                eprintln!("Server: Received unexpected response from client: {:?}", response);
            }
        }
        Ok(())
    }

    pub async fn handle_initialize(request: Request, server_internal: Arc<McpServerInternal>) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: InitializeRequestParams = serde_json::from_value(request.params.unwrap_or_default())
            .map_err(|e| McpError::InvalidMessage(format!("Invalid initialize params: {}", e)))?;

        // Protocol Version Negotiation (Server-side)
        // If server supports client's requested version, respond with that.
        // Otherwise, respond with server's latest supported version.
        let server_current_version = server_internal.current_protocol_version.clone();
        let response_protocol_version = if params.protocol_version == server_current_version {
            params.protocol_version
        } else {
            eprintln!("Server: Client requested protocol version '{}', but server supports '{}'. Responding with server's version.",
                      params.protocol_version, server_current_version);
            // TODO: Implement proper version compatibility check and potentially return UnsupportedProtocolVersion error.
            // If the client's requested version is too old or unsupported at all,
            // you would send an error response here instead of a success.
            // Example error: return Ok(Response::new_unsupported_protocol_error(request_id, params.protocol_version, vec![server_current_version.clone()]));
            server_current_version
        };

        // Store the negotiated version in the server's mutable state
        *server_internal.negotiated_protocol_version.lock().await = Some(response_protocol_version.clone());


        let initialize_result = InitializeResult {
            protocol_version: response_protocol_version,
            capabilities: server_internal.server_capabilities.clone(),
            server_info: Some(server_internal.server_info.clone()),
            instructions: server_internal.instructions.clone(),
        };

        Ok(Response::new_initialize_success(request_id, initialize_result))
    }

     /// Handles the `notifications/initialized` notification from the client.
    /// This associated function logs the event and updates server state.
    pub async fn handle_initialized_notification(notification: Notification, server_internal: Arc<McpServerInternal>) -> Result<(), McpError> {
        // Here you would update the server's internal state to reflect that initialization is complete.
        // For example, setting a flag that allows subsequent operations.
        eprintln!("Server: Received 'notifications/initialized' from client. Client is ready.");

        // Example of updating shared state (if needed for this notification beyond logging)
        // let mut negotiated_version_guard = server_internal.negotiated_protocol_version.lock().unwrap();
        // if negotiated_version_guard.is_some() {
        //    eprintln!("Server: Client confirmed initialization on version: {:?}", negotiated_version_guard.as_ref().unwrap());
        // }
     
        Ok(())
    }

    pub async fn handle_tools_list(request: Request, server_internal: Arc<McpServerInternal>) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: ToolsListRequestParams = serde_json::from_value(request.params.unwrap_or_default())
            .map_err(|e| McpError::ParseJson(format!("Invalid tools/list params: {}", e)))?;
        
        let tools_guard = server_internal.tools.lock().await; // Acquire lock to read tools metadata
        let tools_list_result = ToolsListResult {
            tools: tools_guard.clone(), // Clone the vector to return it
            next_cursor: None, // No pagination implemented in this example
        };

        eprintln!("Server: Responding to tools/list request with {} tools.", tools_list_result.tools.len());

        Ok(Response::new_success(
            request_id,
            Some(serde_json::to_value(tools_list_result).unwrap())
        ))
    }

     /// Handles the `tools/call` request from the client.
    async fn handle_tools_call(request: Request, server_internal: Arc<McpServerInternal>) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: ToolsCallRequestParams = serde_json::from_value(request.params.unwrap_or_default())
            .map_err(|e| McpError::ParseJson(format!("Invalid tools/call parameters: {}", e)))?;

        let tool_name = params.tool_name;
        // The ToolExecutionHandler expects just the arguments Value, not the whole ToolsCallRequestParams
        let tool_arguments = params.tool_parameters.unwrap_or_default(); // Default to empty object if None

        let handlers = server_internal.tool_execution_handlers.lock().await;

        if let Some(handler) = handlers.get(&tool_name) {
            eprintln!("Server: Executing tool '{}' with arguments: {:?}", tool_name, tool_arguments);
            // Execute the specific tool handler and await its result
            let tool_output_result = handler(tool_arguments, server_internal.clone()).await;

            // ToolExecutionHandler returns Result<Value, McpError>
            // We need to convert this into the specific ToolsCallResult structure
            let call_result = match tool_output_result {
                Ok(output_value) => {
                    eprintln!("Server: Tool '{}' raw output: {:?}", tool_name, output_value);
                    
                    ToolsCallResult {
                        content: vec![ToolOutputContentBlock::Text { text: output_value.to_string() }], // Wrap in Text content for generic output
                        is_error: false,
                        error_message: None,
                        structured_content: None, // If tool specifically returns structured JSON
                        metadata: None,
                    }
                },
                Err(e) => {
                    eprintln!("Server: Tool '{}' execution failed: {:?}", tool_name, e);
                    ToolsCallResult {
                        content: vec![ToolOutputContentBlock::Text { text: format!("Tool execution failed: {}", e) }],
                        is_error: true,
                        error_message: Some(format!("Tool execution error: {}", e)),
                        structured_content: None,
                        metadata: None,
                    }
                }
            };

            Ok(Response::new_success(
                request_id,
                Some(serde_json::to_value(call_result).unwrap()) // Serialize the ToolsCallResult
            ))

        } else {
            eprintln!("Server: Tool '{}' not found or no execution handler registered.", tool_name);
            Ok(Response::new_error(
                Some(request_id),
                -32601, // Method not found (or Tool not found, similar semantic)
                &format!("Tool '{}' not found or execution handler not registered.", tool_name),
                None,
            ))
        }
    }

}
