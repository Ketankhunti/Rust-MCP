use std::{collections::HashMap, convert::Infallible, hash::Hash, net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, http::StatusCode, response::{sse::{Event, KeepAlive}, IntoResponse, Sse}, routing::{get, get_service, post, post_service}, Json, Router};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::{mpsc, oneshot, Mutex}, task};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use uuid::Uuid;
use axum::response::Response as AxumResponse;

use crate::{tcp_transport::TcpTransport, transport::StdioTransport, InitializeRequestParams, InitializeResult, McpError, McpMessage, Notification, Request, RequestId, Response, ServerCapabilities, ServerInfo, Tool, ToolOutputContentBlock, ToolsCallRequestParams, ToolsCallResult, ToolsListRequestParams, ToolsListResult};

pub type RequestHandler = Arc<
    dyn Fn(Request, Arc<McpSessionInternal>, Arc<McpServer>) -> BoxFuture<'static, Result<Response, McpError>>
    + Send
    + Sync,
>;

pub type NotificationHandler = Arc<
    dyn Fn(Notification, Arc<McpSessionInternal>, Arc<McpServer>) -> BoxFuture<'static, Result<(), McpError>>
    + Send
    + Sync,
>;

pub type ToolExecutionHandler = Arc<
    dyn Fn(Value, Arc<McpSessionInternal>, Arc<McpServer>) -> BoxFuture<'static, Result<Value, McpError>>
    + Send
    + Sync
>; 

pub enum ServerTransportConfig{
    Stdio,
    Tcp(String),
    Http(String),   // address to listen on
}

pub enum TransportHandle {
    Stdio(StdioTransport),
    Tcp(TcpTransport)
}


impl TransportHandle {
    pub async fn send(&mut self, message: McpMessage) -> Result<(), McpError> {
        match self {
            TransportHandle::Stdio(t) => t.send(message).await,
            TransportHandle::Tcp(t) => t.send(message).await,
        }
    }

    pub async fn recv(&mut self) -> Result<McpMessage, McpError> {
        match self {
            TransportHandle::Stdio(t) => t.recv().await,
            TransportHandle::Tcp(t) => t.recv().await,
        }
    }
}

pub struct McpServer {
    pub server_name: String,
    pub title: Option<String>,
    pub server_version: Option<String>,
    pub server_capabilities: ServerCapabilities,
    pub instructions: Option<String>,
    pub current_protocol_version: String, // Global protocol version for this server app

    // Global handler maps, defined by the SDK user
    pub tool_definitions: Mutex<Vec<Tool>>, // Stores metadata about tools (for tools/list)
    pub tool_execution_handler_registrations: Mutex<HashMap<String, ToolExecutionHandler>>, // Stores execution logic for tools
    pub custom_request_handlers: Mutex<HashMap<String, RequestHandler>>, // User-defined request handlers
    pub custom_notification_handlers: Mutex<HashMap<String, NotificationHandler>>, // User-defined notification handlers
}

impl McpServer {

    const LATEST_SUPPORTED_PROTOCOL_VERSION: &'static str = "2025-06-18";

    /// Creates a new McpServer application instance.
    /// This is the entry point for defining the server's capabilities and handlers.
    pub async fn new(
        server_name: String,
        title: Option<String>,
        server_version: Option<String>,
        mut server_capabilities: ServerCapabilities,
        instructions: Option<String>,
    ) -> Self {
        if server_capabilities.tools.is_none() {
             server_capabilities.tools = Some(crate::ServerToolsCapability { list_changed: Some(true) });
        }

        let mut server = McpServer { // Create the struct
            server_name,
            title,
            server_version,
            server_capabilities,
            instructions,
            current_protocol_version: Self::LATEST_SUPPORTED_PROTOCOL_VERSION.to_string(),
            tool_definitions: Mutex::new(Vec::new()),
            tool_execution_handler_registrations: Mutex::new(HashMap::new()),
            custom_request_handlers: Mutex::new(HashMap::new()),
            custom_notification_handlers: Mutex::new(HashMap::new()),
        };

        server.register_request_handler(
            "initialize",
            Arc::new(move |request, session_internal_arg, app_config_arg| { // app_config_arg is now available
                Box::pin(async move {
                    McpSessionHandler::handle_initialize(request, session_internal_arg, app_config_arg).await
                })
            })
        ).await;

        server.register_notification_handler(
            "notifications/initialized",
            Arc::new(move |notification, session_internal_arg, app_config_arg| {
                Box::pin(async move {
                    McpSessionHandler::handle_initialized_notification(notification, session_internal_arg, app_config_arg).await
                })
            })
        ).await;

        server.register_request_handler(
            "tools/list",
            Arc::new(move |request, session_internal_arg, app_config_arg| {
                Box::pin(async move {
                    McpSessionHandler::handle_tools_list(request, session_internal_arg, app_config_arg).await
                })
            })
        ).await;

        server.register_request_handler(
            "tools/call",
            Arc::new(move |request, session_internal_arg, app_config_arg| {
                Box::pin(async move {
                    McpSessionHandler::handle_tools_call(request, session_internal_arg, app_config_arg).await
                })
            })
        ).await;

        
        server

    }

    pub async fn register_request_handler(&self, method: &str, handler: RequestHandler) {
        let mut handlers = self.custom_request_handlers.lock().await;
        if handlers.contains_key(method) {
            eprintln!("Request handler for method '{}' already registered.", method);
        }
        handlers.insert(method.to_string(), handler);
    }

    pub async fn register_notification_handler(&self, method: &str, handler: NotificationHandler) {
        let mut handlers = self.custom_notification_handlers.lock().await;
        if handlers.contains_key(method) {
            eprintln!("Notification handler for method '{}' already registered.", method);
        }
        handlers.insert(method.to_string(), handler);
    }

    pub async fn add_tool(&self, tool: Tool) { // `&mut self`
        let mut tools_guard = self.tool_definitions.lock().await;
        tools_guard.push(tool);
        eprintln!("Server: Added tool definition '{}'. Current tools: {}",
                  tools_guard.last().unwrap().name, tools_guard.len());
    }

    pub async fn register_tool_execution_handler(&self, tool_name: &str, handler: ToolExecutionHandler) { // `&mut self`
        let mut handlers = self.tool_execution_handler_registrations.lock().await;
        if handlers.contains_key(tool_name) {
            eprintln!("Tool execution handler for '{}' already registered.", tool_name);
        }
        handlers.insert(tool_name.to_string(), handler);
        eprintln!("Server: Registered execution handler for tool '{}'.", tool_name);
    }

    pub async fn start(
        self: Arc<Self>, // McpServer is passed as Arc<Self> (the global application config)
        config: ServerTransportConfig
    ) -> Result<(), McpError> {

        match  config {
            ServerTransportConfig::Stdio => {
                eprintln!("Starting Stdio Server...");

                let (incoming_tx, incoming_rx) = mpsc::channel(100);
                let (outgoing_tx, mut outgoing_rx) = mpsc::channel(100);

                // spawn bridge task
                let app_config = self.clone();
                task::spawn(async move {
                    let mut stdio_transport  = StdioTransport::new();
                    loop {
                        tokio::select! {
                            incoming_msg = stdio_transport.recv() => {
                                match incoming_msg {
                                    Ok(msg) => {
                                        if let Err(e) = incoming_tx.send(msg).await {
                                            eprintln!("Stdio bridge: Failed to send incoming msg to session: {:?}", e); 
                                            break;
                                        }
                                    },
                                    Err(e) => {
                                        eprintln!("Stdio bridge: Recv from transport error: {:?}", e); 
                                        break;
                                    }
                                }
                            },
                            outgoing_msg = outgoing_rx.recv() => {
                                match outgoing_msg {
                                    Some(msg) => {
                                        if let Err(e) = stdio_transport.send(msg).await {
                                            eprintln!("Stdio bridge: Failed to send outgoing msg from session: {:?}", e); 
                                            break;
                                        }
                                    },
                                    None => {
                                        eprintln!("Stdio bridge: Outgoing channel closed."); 
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    eprintln!("Stdio bridge task finished.");
                });

                let session_handler = McpSessionHandler::new(
                    self.server_name.clone(),
                    self.title.clone(),
                    self.server_version.clone(),
                    self.server_capabilities.clone(),
                    self.instructions.clone(),
                    incoming_rx,
                    outgoing_tx,
                    app_config, // Pass Arc<McpServer> (global app config)
                    
                ).await;

                session_handler.process_incoming_messages().await?

            }

            ServerTransportConfig::Tcp(addr) => {
                eprintln!("Starting TCP server on {}", addr);
                let listener = tokio::net::TcpListener::bind(&addr).await
                .map_err(|e| McpError::NetworkError(format!("Failed to bind TCP listener to {}: {}", addr, e)))?;

                eprintln!("TCP server started on {}", addr);

                loop {
                    let (stream, peer_addr) = listener.accept().await
                    .map_err(|e| McpError::NetworkError(format!("Failed to accept TCP connection: {}", e)))?;
                    eprintln!("Server: Accepted connection from {}", peer_addr);

                    let (incoming_tx, incoming_rx) = mpsc::channel(100);
                    let (outgoing_tx, mut outgoing_rx) = mpsc::channel(100);

                    let session_handler = McpSessionHandler::new(
                        self.server_name.clone(),
                        self.title.clone(),
                        self.server_version.clone(),
                        self.server_capabilities.clone(),
                        self.instructions.clone(),
                        incoming_rx,
                        outgoing_tx,
                        self.clone(), // Pass Arc<McpServer> (global app config)
                       
                    ).await;

                    // spawn task to run core MCP protocol logic for this session
                    task::spawn(async move {
                        eprintln!("Server: Session logic task spawned for {}", peer_addr);
                        if let Err(e) = session_handler.process_incoming_messages().await {
                            eprintln!("Server: Session logic for {} terminated with error: {:?}", peer_addr, e);
                        }
                        eprintln!("Server: Session logic for {} finished.", peer_addr);
                    });

                    // spawn task to handler TCP I/O bridging for this session
                    task::spawn(async move {
                        eprintln!("Server: I/O task spawned for {}", peer_addr);
                        let mut tcp_transport = Mutex::new(TcpTransport::new(stream));

                        loop {
                            tokio::select! {
                                incoming_msg = async {
                                    let mut guard = tcp_transport.lock().await;
                                    guard.recv().await
                                } => {
                                    match incoming_msg {
                                        Ok(msg) => {
                                            if let Err(e) = incoming_tx.send(msg).await {
                                                eprintln!("HTTP Server: Failed to send TCP msg to session ({}): {:?}", peer_addr, e); 
                                                break;
                                            }
                                        },
                                        Err(e) => { 
                                            eprintln!("HTTP Server: TCP recv error ({}): {:?}", peer_addr, e); 
                                            break; 
                                        }
                                    }
                                },
                                outgoing_msg = outgoing_rx.recv() => {
                                    match outgoing_msg {
                                        Some(msg) => {
                                            if let Err(e) = tcp_transport.lock().await.send(msg).await {
                                                eprintln!("HTTP Server: Failed to send outgoing msg from session to TCP ({}): {:?}", peer_addr, e); break;
                                            }
                                        },
                                        None => { eprintln!("HTTP Server: Session outgoing channel closed ({}).", peer_addr); break; }
                                    }
                                }
                            }
                        }
                    });

                }
            },
            ServerTransportConfig::Http(addr) => {
                eprintln!("Server: Streamable HTTP listener on {}. (Starting Axum server)", addr);
            // Call the dedicated HTTP listener function
            McpHttpServer::start_listener(
                &addr,
                self.clone(), // Pass Arc<McpServer> (global app config)
            ).await?;

            }
        }

        Ok(())
    }
}

pub struct McpSessionInternal {
   
    // pub request_handlers: Mutex<HashMap<String, RequestHandler>>,
    // pub notification_handlers: Mutex<HashMap<String, NotificationHandler>>,
    pub current_protocol_version: String,
    pub negotiated_protocol_version: Mutex<Option<String>>,
    pub server_capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
    pub instructions: Option<String>,
    // pub tools: Mutex<Vec<Tool>>,
    // pub tool_execution_handlers: Mutex<HashMap<String, ToolExecutionHandler>>,
    pub incoming_rx: Mutex<mpsc::Receiver<McpMessage>>,
    pub outgoing_tx: mpsc::Sender<McpMessage>,

    pending_outgoing_server_requests: Mutex<HashMap<RequestId,oneshot::Sender<Result<Response,McpError>>>>,
    next_outgoing_server_request_id: Mutex<u64>,
    pub http_response_map: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Result<Response, McpError>>>>>,

}

impl McpSessionInternal {
    /// Creates and sends a server-initiated request to the client.
    /// It waits for and returns the client's response.
    pub async fn send_request_from_server(&self, method: &str, params: Option<Value>) -> Result<Response, McpError> {
        // Create a unique ID for this new server-initiated request
        let id = {
            let mut next_id = self.next_outgoing_server_request_id.lock().await;
            let current_id = *next_id;
            *next_id += 1;
            RequestId::Number(current_id)
        };

        let request = Request::new(id.clone(), method, params);
        
        // Create a one-shot channel to receive the response for this specific request
        let (tx, rx) = oneshot::channel();

        // Store the sender half so the response handler can find it
        {
            let mut pending_requests = self.pending_outgoing_server_requests.lock().await;
            pending_requests.insert(id, tx);
        }

        // Send the request into the outgoing message queue
        self.outgoing_tx
            .send(McpMessage::Request(request))
            .await
            .map_err(|e| McpError::NetworkError(format!("Failed to send server-initiated request: {}", e)))?;

        // Wait for the corresponding response to arrive on the one-shot channel
        rx.await.map_err(McpError::OneshotRecv)?
    }

    /// Creates and sends a server-initiated notification to the client.
    /// This is a "fire-and-forget" message; it does not wait for a response.
    pub async fn send_notification_from_server(&self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        let notification = Notification::new(method, params);

        // Send the notification into the outgoing message queue
        self.outgoing_tx
            .send(McpMessage::Notification(notification))
            .await
            .map_err(|e| McpError::NetworkError(format!("Failed to send server-initiated notification: {}", e)))?;
            
        Ok(())
    }
}


pub struct McpSessionHandler{
    pub internal: Arc<McpSessionInternal>,
    pub session_id: Uuid,
    pub app_config: Arc<McpServer>,
}

impl McpSessionHandler {
    const LATEST_SUPPORTED_PROTOCOL_VERSION: &'static str = "2025-06-18";

    pub async fn new(
        server_name: String,
        title: Option<String>,
        server_version: Option<String>,
        mut server_capabilities: ServerCapabilities,
        instructions: Option<String>,
        incoming_rx: mpsc::Receiver<McpMessage>,
        outgoing_tx: mpsc::Sender<McpMessage>,
        app_config: Arc<McpServer>,
    ) -> Self {

        let server_info = ServerInfo {
            name: server_name,
            title: title.clone(),
            version: server_version
        };

        if server_capabilities.tools.is_none() {
            server_capabilities.tools = Some(crate::ServerToolsCapability { list_changed: Some(true) });
        }

        let internal = Arc::new(McpSessionInternal {
            current_protocol_version: Self::LATEST_SUPPORTED_PROTOCOL_VERSION.to_string(),
            negotiated_protocol_version: Mutex::new(None),
            server_capabilities,
            server_info,
            instructions,
            incoming_rx: Mutex::new(incoming_rx),
            outgoing_tx: outgoing_tx,
            pending_outgoing_server_requests: Mutex::new(HashMap::new()),
            next_outgoing_server_request_id: Mutex::new(0),
            http_response_map: Arc::new(Mutex::new(HashMap::new())),
        });

        let session_handler = Self {
            internal,
            session_id: Uuid::new_v4(),
            app_config, // store global app config revference
        
        };

        session_handler

    }

    pub async fn send_request_from_server(&self, method: &str, params: Option<Value>) -> Result<Response, McpError> {
        let id = {
            let mut next_id = self.internal.next_outgoing_server_request_id.lock().await;
            let current_id = *next_id;
            *next_id += 1;
            RequestId::Number(current_id)
        };

        let request = Request::new(id.clone(), method,params);
        let (tx, rx) = oneshot::channel();

        {
            let mut pending_requests = self.internal.pending_outgoing_server_requests.lock().await;
            pending_requests.insert(id,tx);
        }
        self.internal.outgoing_tx.send(McpMessage::Request(request)).await
        .map_err(|e| McpError::NetworkError(format!("Failed to send server-initiated request: {}", e)))?;
         
         rx.await.map_err(McpError::OneshotRecv)?
    }

    pub async fn send_notification_from_server(&self, method: &str, params: Option<Value>) -> Result<(), McpError> {
        let notification = Notification::new(method, params);
        self.internal.outgoing_tx.send(McpMessage::Notification(notification)).await
            .map_err(|e| McpError::NetworkError(format!("Failed to send server-initiated notification: {}", e)))?;
        Ok(())
    }

    pub async fn process_incoming_messages(&self) -> Result<(),McpError> {
        eprintln!("Session handler run loop started for session {:#?}", self.session_id);
        loop{
            let received_msg =  {
                let mut incoming_rx_guard = self.internal.incoming_rx.lock().await;
                eprintln!("Server: Session handler acquiring incoming_rx lock, waiting for message...");
                let res = incoming_rx_guard.recv().await;
                eprintln!(
                    "Server: Session handler released incoming_rx lock. Result: {:?}",
                    res.as_ref().map(|_| "Ok(McpMessage)").unwrap_or_else(|| "None")
                );
                res
            };
            match received_msg {
                Some(message) => {
                    self.handle_message(message).await?;
                },
                None => {
                    eprintln!("Server: Incoming message channel closed, session handler exiting.");
                    break;
                }
            }
        }
        eprintln!("Server: Session handler run loop finished for session {:#?}", self.session_id);
        Ok(())
    }

    
    async fn handle_message(&self, message: McpMessage) -> Result<(), McpError> {
        match message {
            McpMessage::Request(request) => {
                let method = request.method.clone();
                let id = request.id.clone();
                let request_handlers = self.app_config.custom_request_handlers.lock().await; // Access global handlers

                let response_to_send = if let Some(handler) = request_handlers.get(&method) {
                    let response_result = handler(request, self.internal.clone(), self.app_config.clone()).await; // Pass app_config
                    match response_result {
                        Ok(res) => res,
                        Err(err) => {
                            eprintln!("Server: Handler for '{}' returned an error: {:?}", method, err);
                            let (error_code, error_message, error_data) = match err {
                                McpError::ProtocolError(msg) => {
                                    (-32600, msg, None)
                                },
                                McpError::ParseJson(msg) => {
                                    (-32602, msg, None)
                                },
                                McpError::ToolNotFound(msg) => {
                                    (-32601, msg, None)
                                },
                                _ => {
                                    (-32000, format!("Internal server error: {}", err), None)
                                }
                            };
                            Response::new_error(Some(id.clone()), error_code, &error_message, error_data)
                        }
                    }
                } else {
                    eprintln!("Server: Received unhandled request method: {}", method);
                    Response::new_error(
                        Some(id.clone()),
                        -32601,
                        &format!("Method '{}' not found", method),
                        None,
                    )
                };
                let mut http_response_map = self.internal.http_response_map.lock().await;
                if let Some(oneshot_tx) = http_response_map.remove(&id) {
                    eprintln!("Server: Routing response for {:?} to HTTP oneshot channel.", id);
                    if let Err(_) = oneshot_tx.send(Ok(response_to_send)) {
                        eprintln!("Server: Oneshot sender for HTTP response ID {:?} was dropped.", &id);
                    }
                } else {
                  
                    eprintln!("Server: Routing response for {:?} to general outgoing channel.", id);
                    self.internal.outgoing_tx.send(McpMessage::Response(response_to_send)).await
                        .map_err(|e| McpError::NetworkError(format!("Failed to send response via outgoing channel: {}", e)))?;
                }
            }
            McpMessage::Notification(notification) => {
                let method = notification.method.clone();
                let notification_handlers = self.app_config.custom_notification_handlers.lock().await;

                if let Some(handler) = notification_handlers.get(&method) {
                    if let Err(e) = handler(notification, self.internal.clone(), self.app_config.clone()).await {
                        eprintln!("Server: Handler for notification '{}' returned an error: {:?}", method, e);
                    }
                } else {
                    let request_handlers = self.app_config.custom_request_handlers.lock().await;

                    if request_handlers.contains_key(&method) {
                        eprintln!("Server: Received method '{}' as a Notification, but it's configured as a Request handler. Informing client.", method);
                        let error_response = Response::new_error(
                            None,
                            -32600,
                            &format!("Method '{}' expects an ID and a response, but was received as a Notification (missing 'id' field).", method),
                            None,
                        );
                        self.internal.outgoing_tx.send(McpMessage::Response(error_response)).await
                            .map_err(|e| McpError::NetworkError(format!("Failed to send error response via outgoing channel: {}", e)))?;
                    } else {
                        eprintln!("Server: Received completely unknown notification method: {}", method);
                    }
                }
            }
            McpMessage::Response(response) => {
                let response_id = response.id.clone();
                if let Some(id_val) = response_id {
                    let mut pending_requests = self.internal.pending_outgoing_server_requests.lock().await;
                    if let Some(oneshot_tx) = pending_requests.remove(&id_val) {
                        if let Err(_) = oneshot_tx.send(Ok(response)) {
                            eprintln!("Server: Oneshot sender for server-initiated request ID {:?} was dropped. Response not delivered.", id_val);
                        }
                    } else {
                        eprintln!("Server: Received unhandled response for server-initiated request ID {:?}.", id_val);
                    }
                } else {
                    eprintln!("Server: Received server-initiated error response without ID: {:?}", response);
                }
            }
        }
        Ok(())
    }

    async fn handle_initialize(request: Request, session_internal: Arc<McpSessionInternal>, app_config: Arc<McpServer>) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: InitializeRequestParams = serde_json::from_value(request.params.unwrap_or_default())
            .map_err(|e| McpError::ParseJson(format!("Invalid initialize params: {}", e)))?;

        let server_current_version = app_config.current_protocol_version.clone(); // Access from app_config
        let response_protocol_version = if params.protocol_version == server_current_version {
            params.protocol_version
        } else {
            eprintln!("Server: Client requested protocol version '{}', but server supports '{}'. Responding with server's version.",
                      params.protocol_version, server_current_version);
            server_current_version
        };

        *session_internal.negotiated_protocol_version.lock().await = Some(response_protocol_version.clone());

        let server_info = ServerInfo {
            name: app_config.server_name.clone(),
            title: None,
            version: app_config.server_version.clone()
        };

        let initialize_result = InitializeResult {
            protocol_version: response_protocol_version,
            capabilities: app_config.server_capabilities.clone(), // Access from app_config
            server_info: Some(server_info), // Access from app_config
            instructions: app_config.instructions.clone(), // Access from app_config
        };

        Ok(Response::new_success(request_id, Some(serde_json::to_value(initialize_result).unwrap())))
    }

    async fn handle_initialized_notification(notification: Notification, session_internal: Arc<McpSessionInternal>, app_config: Arc<McpServer>) -> Result<(), McpError> {
        eprintln!("Server: Received 'notifications/initialized' from client. Client is ready.");
        Ok(())
    }

    async fn handle_tools_list(request: Request, session_internal: Arc<McpSessionInternal>, app_config: Arc<McpServer>) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: ToolsListRequestParams = serde_json::from_value(request.params.unwrap_or_default())
            .map_err(|e| McpError::ParseJson(format!("Invalid tools/list params: {}", e)))?;

        let tools_guard = app_config.tool_definitions.lock().await; // Access global tool definitions
        let tools_list_result = ToolsListResult {
            tools: tools_guard.clone(),
            next_cursor: None,
        };

        eprintln!("Server: Responding to tools/list request with {} tools.", tools_list_result.tools.len());

        Ok(Response::new_success(request_id, Some(serde_json::to_value(tools_list_result).unwrap())))
    }
    
    async fn handle_tools_call(request: Request, session_internal: Arc<McpSessionInternal>, app_config: Arc<McpServer>) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: ToolsCallRequestParams = serde_json::from_value(request.params.unwrap_or_default())
            .map_err(|e| McpError::ParseJson(format!("Invalid tools/call parameters: {}", e)))?;

        let tool_name = params.tool_name;
        let tool_arguments = params.tool_parameters.unwrap_or_default();

        let tool_execution_handlers = app_config.tool_execution_handler_registrations.lock().await; // Access global execution handlers

        if let Some(handler) = tool_execution_handlers.get(&tool_name) {
            eprintln!("Server: Executing tool '{}' with arguments: {:?}", tool_name, tool_arguments);
            let tool_output_result = handler(tool_arguments, session_internal.clone(), app_config.clone()).await; // Pass app_config

            let call_result = match tool_output_result {
                Ok(output_value) => {
                    eprintln!("Server: Tool '{}' raw output: {:?}", tool_name, output_value);
                    ToolsCallResult {
                        content: vec![ToolOutputContentBlock::Text { text: output_value.to_string() }],
                        is_error: false,
                        error_message: None,
                        structured_content: None,
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

            Ok(Response::new_success(request_id, Some(serde_json::to_value(call_result).unwrap())))

        } else {
            eprintln!("Server: Tool '{}' not found or no execution handler registered.", tool_name);
            return Err(McpError::ToolNotFound(
                format!("Tool '{}' not found or execution handler not registered.", tool_name)
            ));
        }
    }
}

pub struct McpSessionClient {
    // This is the sender for messages *into* the McpSessionHandler's incoming_rx_internal.
    pub incoming_tx_to_session_handler: mpsc::Sender<McpMessage>,
    
    // This map is used to send responses from the McpSessionHandler back to the HTTP POST handler.
    pub http_response_map: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Result<Response, McpError>>>>>,

    // This is the receiver for messages *from* the McpSessionHandler's outgoing_tx_internal.
    // Used for future SSE GET streams. This needs to be the OTHER HALF of the outgoing_tx
    pub outgoing_rx_from_session_handler: Mutex<Option<mpsc::Receiver<McpMessage>>>, // NEW: Store outgoing_rx

    pub session_id: Uuid, // Store session ID for context/logging
}

impl McpSessionClient {
    pub fn new(
        incoming_tx: mpsc::Sender<McpMessage>,
        outgoing_rx: mpsc::Receiver<McpMessage>, // Added this parameter
        http_response_map: Arc<Mutex<HashMap<RequestId, oneshot::Sender<Result<Response, McpError>>>>>,
        session_id: Uuid,
    ) -> Self {
        McpSessionClient {
            incoming_tx_to_session_handler: incoming_tx,
            outgoing_rx_from_session_handler: Mutex::new(Some(outgoing_rx)),
            http_response_map,
            session_id,
        }
    }

    // Sends a Request message to the session handler and waits for its specific response via oneshot.
    pub async fn send_request_to_session_handler(&self, request: Request, timeout_duration: Duration) -> Result<Response, McpError> {
        let id_to_correlate = request.id.clone();
        
        let (oneshot_tx, oneshot_rx) = oneshot::channel();
        
        { // Store the oneshot sender in the http_response_map
            let mut map_guard = self.http_response_map.lock().await;
            if let Some(_) = map_guard.insert(id_to_correlate.clone(), oneshot_tx) {
                eprintln!("McpSessionClient: WARN: Overwriting http_response_map entry for ID {:?}.", id_to_correlate);
            }
        }

        self.incoming_tx_to_session_handler.send(McpMessage::Request(request)).await
            .map_err(|e| McpError::NetworkError(format!("Failed to send request to session handler: {}", e)))?;

        // Wait for response via oneshot (with timeout)
        tokio::time::timeout(timeout_duration, oneshot_rx).await
            .map_err(|_| McpError::RequestTimeout)? // Timeout error
            .map_err(McpError::OneshotRecv)? // oneshot channel error
    }

    // Sends a Notification message to the session handler.
    pub async fn send_notification_to_session_handler(&self, notification: Notification) -> Result<(), McpError> {
        self.incoming_tx_to_session_handler.send(McpMessage::Notification(notification)).await
            .map_err(|e| McpError::NetworkError(format!("Failed to send notification to session handler: {}", e)))?;
        Ok(())
    }

    // TODO: Add send_response_to_session_handler for client-initiated responses (from client->server Response messages)
    // This is less common, but possible per spec.
}
// --- NEW: Main HTTP Server Entry Point (The actual HTTP Listener and Router) ---
pub struct McpHttpServer;

pub struct HttpGlobalAppState {
    pub sessions: Mutex<HashMap<Uuid, Arc<McpSessionClient>>>,
    pub app_config:  Arc<McpServer>
}

impl McpHttpServer {

    #[allow(clippy::too_many_arguments)]
    pub async fn start_listener(
        addr: &str,
        // The rest of the parameters are now passed via `app_config`
        app_config: Arc<McpServer>, // Pass the entire application configuration
    ) -> Result<(), McpError> {
        let listener = tokio::net::TcpListener::bind(addr).await
            .map_err(|e| McpError::NetworkError(format!("Failed to bind HTTP listener to {}: {}", addr, e)))?;

        eprintln!("HTTP Server: Listening on http://{}", addr);

        // Create global HTTP state
        let http_global_state = Arc::new(HttpGlobalAppState {
            sessions: Mutex::new(HashMap::new()),
            app_config, // Store the application config
        });

        // Build Axum Router
        let app = Router::new()
            .route("/mcp", post(McpHttpServer::handle_mcp_post)) // POST for requests/notifications/responses
            .route("/mcp", get(McpHttpServer::handle_mcp_get_sse)) // GET for SSE
            .with_state(http_global_state); // Pass the global HTTP state to handlers

        axum::serve(listener, app).await.unwrap();
        Ok(()) // Should run indefinitely
    }

    // Handler for HTTP POST requests to /mcp
    // This is where incoming HTTP requests are processed into McpMessages

    pub async fn handle_mcp_post(
        State(state): State<Arc<HttpGlobalAppState>>, // Get global HTTP state
        headers: axum::http::HeaderMap,
        Json(raw_json_value): Json<Value>
    ) -> Result<AxumResponse, McpError>  {
        
        eprintln!("HTTP Server: Received POST request body: {:#?}", raw_json_value);

        let mcp_message = McpMessage::from_json(&raw_json_value.to_string())?;

        let is_request = matches!(mcp_message, McpMessage::Request(_));
        
        let session_id_from_header: Option<Uuid> = headers // Renamed for clarity in this function
            .get("Mcp-Session-Id")
            .and_then(|h_val| h_val.to_str().ok())
            .and_then(|s| Uuid::parse_str(s).ok());

        let session_client_arc: Arc<McpSessionClient>; // This will hold the client interface to the session
        let mut http_response_headers = axum::http::HeaderMap::new();

        let mut sessions_guard = state.sessions.lock().await;

        if let Some(session_id) = session_id_from_header {
            if let Some(existing_client) = sessions_guard.get(&session_id) {
                session_client_arc = existing_client.clone();
                eprintln!("HTTP Server: Reusing existing session ({:#?}) for POST request.", session_id);

                if is_request && matches!(mcp_message, McpMessage::Request(ref req) if req.method == "initialize") {
                    eprintln!("HTTP Server: WARN: Initialize request received for existing session ({:#?}). Returning 400 Bad Request.", session_id);
                    return Ok(StatusCode::BAD_REQUEST.into_response());
                }

            } else {
                eprintln!("HTTP Server: Session ID '{:#?}' not found for POST request. Returning 404.", session_id);
                return Ok(StatusCode::NOT_FOUND.into_response());
            }
        } else {
            if is_request && matches!(mcp_message, McpMessage::Request(ref req) if req.method == "initialize") {
                eprintln!("HTTP Server: Creating new MCP session for InitializeRequest.");
                // Create channels that will bridge HTTP input/output to the new session handler
                let (incoming_tx_to_session_handler, incoming_rx_from_http) = mpsc::channel(100);
                let (outgoing_tx_from_session_handler, outgoing_rx_from_session_handler_for_sse) = mpsc::channel(100); // For future SSE GET

                // Create the McpSessionHandler
                let new_session_handler = McpSessionHandler::new(
                    state.app_config.server_name.clone(),
                    state.app_config.title.clone(),
                    state.app_config.server_version.clone(),
                    state.app_config.server_capabilities.clone(),
                    state.app_config.instructions.clone(),
                    incoming_rx_from_http, // Receiver for messages from HTTP POST
                    outgoing_tx_from_session_handler, // Sender for responses from session
                    state.app_config.clone(),
                ).await;

                let new_session_id = new_session_handler.session_id;

                // Create the McpSessionClient for the new session
                let new_session_client = Arc::new(McpSessionClient::new(
                    incoming_tx_to_session_handler, // This sender is given to McpSessionClient
                    outgoing_rx_from_session_handler_for_sse, // This receiver is given to McpSessionClient for SSE
                    new_session_handler.internal.http_response_map.clone(), // Share the map
                    new_session_id,
                ));

                sessions_guard.insert(new_session_id, new_session_client.clone()); // Store the McpSessionClient

                let state_clone = state.clone();

                // Spawn the persistent session logic task
                task::spawn(async move {
                    eprintln!("HTTP Server: Session logic task spawned for session ({:#?}).", new_session_id);
                    if let Err(e) = new_session_handler.process_incoming_messages().await {
                         eprintln!("HTTP Server: Session logic for session ({:#?}) terminated with error: {:?}", new_session_id, e);
                    }
                    eprintln!("HTTP Server: Session logic for session ({:#?}) finished.", new_session_id);
                    let mut sessions_map = state_clone.sessions.lock().await;
                    sessions_map.remove(&new_session_id);
                    eprintln!("HTTP Server: Session ({:#?}) removed from active sessions map.", new_session_id);
                });

                session_client_arc = new_session_client.clone(); // Use the created client for this request
                http_response_headers.insert("Mcp-Session-Id", new_session_id.to_string().parse().unwrap());

            } else {
                eprintln!("HTTP Server: Non-initialize request without Session ID header. Returning 400 Bad Request.");
                return Ok(StatusCode::BAD_REQUEST.into_response());
            }
        }
        drop(sessions_guard); // Drop guard early

        let response_from_session_result: Result<Option<McpMessage>, McpError> = match mcp_message {
            McpMessage::Request(request_obj) => {
                session_client_arc
                    .send_request_to_session_handler(request_obj, Duration::from_secs(5))
                    .await
                    .map(|response| Some(McpMessage::Response(response)))
            }
            McpMessage::Notification(notification_obj) => {
                session_client_arc
                    .send_notification_to_session_handler(notification_obj)
                    .await
                    .map(|_| None)
            }
            McpMessage::Response(client_response_obj) => {
                session_client_arc
                    .send_notification_to_session_handler(Notification::new(
                        "client/response",
                        Some(serde_json::to_value(client_response_obj).unwrap()),
                    ))
                    .await
                    .map(|_| None)
            }
        };

        let final_axum_response = match response_from_session_result {
            Ok(Some(McpMessage::Response(response))) => {
                let mut res = Json(serde_json::to_value(response).unwrap()).into_response();
                res.headers_mut().extend(http_response_headers);
                res
            }
            Ok(None) => {
                let mut res = if is_request {
                    eprintln!("No response for request — timeout.");
                    StatusCode::GATEWAY_TIMEOUT.into_response()
                } else {
                    eprintln!("Notification processed. 202 Accepted.");
                    StatusCode::ACCEPTED.into_response()
                };
                res.headers_mut().extend(http_response_headers);
                res
            }
            Err(e) => {
                eprintln!("Session handler error: {:?}", e);
                e.into_response() // Because McpError implements IntoResponse
            }
            _ => {
                // Technically unreachable
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        };
        Ok(final_axum_response)
        
        
    }

 
    // Handler for HTTP GET requests to /mcp (for SSE)
    // For now, return Method Not Allowed. This will be expanded for SSE later.
    pub async fn handle_mcp_get_sse(
        State(state): State<Arc<HttpGlobalAppState>>,
        headers: axum::http::HeaderMap
    ) -> AxumResponse {
        
        eprintln!("HTTP Server: Received GET request for SSE stream.");

        let session_id_str = match headers.get("Mcp-Session-Id").and_then(|v| v.to_str().ok()) {
            Some(s) => s,
            None => return (StatusCode::BAD_REQUEST, "Mcp-Session-Id header is required for SSE stream").into_response(),
        };

        let session_id = match Uuid::parse_str(session_id_str) {
            Ok(id) => id,
            Err(_) => return (StatusCode::BAD_REQUEST, "Invalid Mcp-Session-Id format").into_response(),
        };

        eprintln!("HTTP Server: SSE request for session ID: {}", session_id);

        let session_client = {
            let session_guard = state.sessions.lock().await;
            session_guard.get(&session_id).cloned()
        };

        let session_client = match session_client {
            Some(client) => client,
            None => return (StatusCode::NOT_FOUND, "Session not found").into_response(),
        };

        let receiver = match session_client.outgoing_rx_from_session_handler.lock().await.take() {
            Some(rx) => rx,
            None => return (StatusCode::CONFLICT, "SSE stream already established for this session").into_response(),
        };

        eprintln!("HTTP Server: SSE receiver taken for session {}. Starting stream.", session_id);

        let stream = ReceiverStream::new(receiver)
        .map(move |msg: McpMessage| -> Result<Event, Infallible> {
            match msg.to_json() {
                Ok(json_str) => {
                    eprintln!("SSE Stream [{}]: Sending message: {}", session_id, json_str);
                    Ok(Event::default().data(json_str))
                }
                Err(e) => {
                    eprintln!("SSE Stream [{}]: Failed to serialize McpMessage: {:?}", session_id, e);
                    Ok(Event::default().event("error").data("serialization error"))
                }
            }
        });

        Sse::new(stream)
            .keep_alive(KeepAlive::new().text("keep-alive"))
            .into_response()

    }
}
