use std::{collections::HashMap, sync::{Arc}};

use futures::future::BoxFuture;
use tokio::sync::Mutex;

use crate::{transport::StdioTransport, InitializeRequestParams, InitializeResult, McpError, McpMessage, Notification, Request, Response, ServerCapabilities, ServerInfo};

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

pub struct McpServerInternal {
    pub transport: Mutex<StdioTransport>,
    pub request_handlers: Mutex<HashMap<String, RequestHandler>>,
    pub notification_handlers: Mutex<HashMap<String, NotificationHandler>>,
    pub current_protocol_version: String,
    pub negotiated_protocol_version: Mutex<Option<String>>,
    pub server_capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
    pub instructions: Option<String>,
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
        server_capabilities: ServerCapabilities,
        instructions: Option<String>,
    ) -> Self {
        let server_info = ServerInfo {
            name: server_name,
            title: None, // Can be set later or via constructor arg
            version: server_version,
        };
        let internal = Arc::new(McpServerInternal {
            transport: Mutex::new(StdioTransport::new()),
            request_handlers: Mutex::new(HashMap::new()),
            notification_handlers: Mutex::new(HashMap::new()),
            current_protocol_version: Self::LATEST_SUPPORTED_PROTOCOL_VERSION.to_string(),
            negotiated_protocol_version: Mutex::new(None),
            server_capabilities,
            server_info: server_info,
            instructions,
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

}
