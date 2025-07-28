use std::{collections::HashMap, convert::Infallible, fmt::format, hash::Hash, net::SocketAddr, sync::Arc, time::Duration};

use axum::{extract::State, http::StatusCode, response::{sse::{Event, KeepAlive}, IntoResponse, Sse}, routing::{get, get_service, post, post_service}, Json, Router};
use dashmap::DashMap;
use futures::future::{BoxFuture};
use serde_json::{json, Value};
use tokio::{net::TcpListener, sync::{mpsc, oneshot, Mutex, RwLock}, task};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};
use uuid::Uuid;
use axum::response::Response as AxumResponse;

use crate::{pagination::PaginationCursor, resources::*, ToolsListRequestParams, LISTABLE_RESOURCE_REGISTRY};
// use mcp_sdk_types::{Prompt, PromptsGetRequestParams, PromptsListResult};

use crate::{prompts::*, tcp_transport::TcpTransport, transport::StdioTransport, InitializeRequestParams, InitializeResult, McpError, McpMessage, Notification, Request, RequestId, Response, ServerCapabilities, ServerInfo, Tool, ToolOutputContentBlock, ToolsCallRequestParams, ToolsCallResult, ToolsListResult, PROMPT_REGISTRY, TOOL_REGISTRY};

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

pub type PromptHandler = Arc<
    dyn Fn(serde_json::Value) -> BoxFuture<'static, Result<Vec<PromptMessage>, String>>
    + Send
    + Sync,
>;

pub type ResourceHandler = Arc<
    // Takes the URI and maybe some parsed parameters
    dyn Fn(String) -> BoxFuture<'static, Result<ResourceContents, String>> 
    + Send 
    + Sync,
>;

const PAGE_SIZE: usize = 1; 

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
    pub tool_definitions: RwLock<Vec<Tool>>, // Stores metadata about tools (for tools/list)
    pub tool_execution_handler_registrations: DashMap<String, ToolExecutionHandler>, // Stores execution logic for tools
    pub custom_request_handlers: DashMap<String, RequestHandler>, // User-defined request handlers
    pub custom_notification_handlers: DashMap<String, NotificationHandler>, // User-defined notification handlers
    pub prompt_definitions: RwLock<Vec<Prompt>>,
    pub prompt_handler_registrations: DashMap<String, PromptHandler>,
    pub resource_handlers: DashMap<String, ResourceHandler>,
    // A list of metadata for resources that should be returned by `resources/list`.
    pub resources_definitions: RwLock<Vec<Resource>>,
    pub resource_templates: RwLock<Vec<ResourceTemplate>>,

    pub resource_subscriptions: DashMap<String,Vec<Uuid>>,
    http_global_state: RwLock<Option<Arc<HttpGlobalAppState>>>,

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

        let server = McpServer { // Create the struct
            server_name,
            title,
            server_version,
            server_capabilities,
            instructions,
            current_protocol_version: Self::LATEST_SUPPORTED_PROTOCOL_VERSION.to_string(),
            tool_definitions: RwLock::new(Vec::new()),
            tool_execution_handler_registrations: DashMap::new(),
            custom_request_handlers: DashMap::new(),
            custom_notification_handlers: DashMap::new(),
            prompt_definitions: RwLock::new(Vec::new()),
            prompt_handler_registrations: DashMap::new(),
            resource_handlers: DashMap::new(),
            resources_definitions: RwLock::new(Vec::new()),
            resource_templates: RwLock::new(Vec::new()),
            resource_subscriptions: DashMap::new(),
            http_global_state: RwLock::new(None),
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

        server.register_request_handler(
            "prompts/list",
            Arc::new(|req, session, server| Box::pin(McpSessionHandler::handle_prompts_list(req, session, server)))
        ).await;

        server.register_request_handler(
            "prompts/get",
            Arc::new(|req, session, server| Box::pin(McpSessionHandler::handle_prompts_get(req, session, server)))
        ).await;

        server.register_request_handler(
            "resources/list",
            Arc::new(|req, session, server| Box::pin(McpSessionHandler::handle_resources_list(req, session, server)))
        ).await;

        server.register_request_handler(
            "resources/read",
            Arc::new(|req, session, server| Box::pin(McpSessionHandler::handle_resources_read(req, session, server)))
        ).await;

        server.register_request_handler(
            "resources/templates/list",
            Arc::new(|req, session, server| Box::pin(McpSessionHandler::handle_resources_templates_list(req, session, server)))
        ).await;

        server.register_request_handler(
            "resources/subscribe",
            Arc::new(|req, session, server| Box::pin(McpSessionHandler::handle_resources_subscribe(req, session, server)))
        ).await;
        
        server

    }

    pub async fn register_request_handler(&self, method: &str, handler: RequestHandler) {
        if self.custom_request_handlers.contains_key(method) {
            eprintln!("Request handler for method '{}' already registered.", method);
        }
        self.custom_request_handlers.insert(method.to_string(), handler);
    }

    pub async fn register_notification_handler(&self, method: &str, handler: NotificationHandler) {
        if self.custom_notification_handlers.contains_key(method) {
            panic!("Notification handler for method '{}' already registered.", method);
        }
        self.custom_notification_handlers.insert(method.to_string(), handler);
    }

    pub async fn add_tool(&self, tool: Tool) { // `&mut self`
        self.tool_definitions.write().await.push(tool);
    }

    pub async fn register_tool_execution_handler(&self, tool_name: &str, handler: ToolExecutionHandler) { // `&mut self`
        if self.tool_execution_handler_registrations.contains_key(tool_name) {
            panic!("Tool execution handler for '{}' already registered.", tool_name);
        }
        self.tool_execution_handler_registrations.insert(tool_name.to_string(), handler);
    }

    pub async fn add_prompt(&self, prompt: Prompt) {
        self.prompt_definitions.write().await.push(prompt);
    }

    pub async fn register_prompt_handler(&self, name: &str, handler: PromptHandler) {
        self.prompt_handler_registrations.insert(name.to_string(), handler);
    }

    /// Discovers and registers all prompts defined with the `#[prompt]` macro.
    pub async fn register_discovered_prompts(&self) {
        let mut registry = PROMPT_REGISTRY.lock().unwrap();
        for (prompt, handler) in registry.drain(..) {
            let name = prompt.name.clone();
            self.add_prompt(prompt).await;
            self.register_prompt_handler(&name, handler).await;
        }
    }

    pub async fn add_resource(&self, resource: ResourceContents) {
        let uri = resource.resource.uri.clone();
        
        // 1. Add the metadata to the list of discoverable resources.
        self.resources_definitions.write().await.push(resource.resource.clone());

        // 2. Create a simple handler that just returns the static content.
        let handler: ResourceHandler = Arc::new(move |_uri: String| {
            let resource_clone = resource.clone();
            Box::pin(async move { Ok(resource_clone) })
        });

        // 3. Add the handler to the unified map.
        self.resource_handlers.insert(uri.clone(), handler);
        eprintln!("Server: Added static resource with URI '{}'.", uri);
    }

    pub async fn register_resource_handler(&self, uri: &str, handler: ResourceHandler) {
        self.resource_handlers.insert(uri.to_string(), handler);
        eprintln!("Server: Registered dynamic resource handler for URI '{}'.", uri);
    }

    /// Adds a resource template to the server's collection.
    pub async fn add_resource_template(&self, template: ResourceTemplate) {
        self.resource_templates.write().await.push(template);
    }

    pub async fn register_discovered_resources(&self) {
        // 1. Register the handlers for reading content.
        let mut handler_registry = crate::RESOURCE_HANDLER_REGISTRY.lock().unwrap();
        for (uri, handler) in handler_registry.drain() {
            self.register_resource_handler(&uri, handler).await;
        }
        drop(handler_registry); // Release lock
    
        // 2. Register the metadata for resources that should be listable.
        let mut listable_registry = LISTABLE_RESOURCE_REGISTRY.lock().unwrap();
        let mut server_listable = self.resources_definitions.write().await;
        for resource in listable_registry.drain(..) {
            server_listable.push(resource);
        }
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
                        let tcp_transport = Mutex::new(TcpTransport::new(stream));

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

    pub async fn register_discovered_tools(&self) {
        let tools: Vec<(Tool, ToolExecutionHandler)> = {
            // Lock the registry and drain the tools
            let mut registry = crate::server::TOOL_REGISTRY
                .lock()
                .expect("TOOL_REGISTRY mutex poisoned");
    
            registry.drain(..).collect()
        };
    
        // Register each tool and its handler asynchronously
        for (tool, handler) in tools {
            let name = tool.name.clone();
            self.add_tool(tool).await;
            self.register_tool_execution_handler(&name, handler).await;
        }
    }

    pub async fn set_http_global_state(&self, state: Arc<HttpGlobalAppState>) {
        *self.http_global_state.write().await = Some(state);
    }

    pub async fn notify_resource_updated(&self, uri: &str, updated_params: ResourcesUpdatedParams) {
        if let Some(http_state) = self.http_global_state.read().await.as_ref() {
            if let Some(session_ids_guard) = self.resource_subscriptions.get(uri) {
                let session_ids: Vec<Uuid> = session_ids_guard.iter().cloned().collect();
                drop(session_ids_guard); // optional, auto-dropped
            
                eprintln!("Server: Notifying {} sessions about update to resource '{}'", session_ids.len(), uri);
            
                let notification = Notification::new(
                    "notifications/resources/updated",
                    Some(serde_json::to_value(updated_params).unwrap())
                );
            
                for session_id in session_ids {
                    if let Some(session_tx) = http_state.session_outgoing_txs.get(&session_id) {
                        if let Err(e) = session_tx.send(McpMessage::Notification(notification.clone())).await {
                            eprintln!("  -> Failed to notify session {}: {}", session_id, e);
                        } else {
                            eprintln!("  -> Successfully notified session {}", session_id);
                        }
                    }
                }
            }
            
        } else {
            eprintln!("Server: Cannot notify resource updated because HTTP global state is not set.");
        }
    }
    

}

pub struct McpSessionInternal {
   
    
    pub current_protocol_version: String,
    pub negotiated_protocol_version: RwLock<Option<String>>,
    pub server_capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
    pub instructions: Option<String>,
    
    pub incoming_rx: RwLock<mpsc::Receiver<McpMessage>>,
    pub outgoing_tx: mpsc::Sender<McpMessage>,

    pub pending_outgoing_server_requests: DashMap<RequestId,oneshot::Sender<Result<Response,McpError>>>,
    pub next_outgoing_server_request_id: RwLock<u64>,
    pub http_response_map: Arc<DashMap<RequestId, oneshot::Sender<Result<Response, McpError>>>>,
    pub session_id: Uuid,

}

impl McpSessionInternal {
    /// Creates and sends a server-initiated request to the client.
    /// It waits for and returns the client's response.
    pub async fn send_request_from_server(&self, method: &str, params: Option<Value>) -> Result<Response, McpError> {
        // Create a unique ID for this new server-initiated request
        let id = {
            let mut next_id = self.next_outgoing_server_request_id.write().await;
            let current_id = *next_id;
            *next_id += 1;
            RequestId::Number(current_id)
        };

        let request = Request::new(id.clone(), method, params);
        
        // Create a one-shot channel to receive the response for this specific request
        let (tx, rx) = oneshot::channel();

       
        self.pending_outgoing_server_requests.insert(id, tx);
        
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

        let session_id = Uuid::new_v4();

        let internal = Arc::new(McpSessionInternal {
            current_protocol_version: Self::LATEST_SUPPORTED_PROTOCOL_VERSION.to_string(),
            negotiated_protocol_version: RwLock::new(None),
            server_capabilities,
            server_info,
            instructions,
            incoming_rx: RwLock::new(incoming_rx),
            outgoing_tx: outgoing_tx,
            pending_outgoing_server_requests: DashMap::new(),
            next_outgoing_server_request_id: RwLock::new(0),
            http_response_map: Arc::new(DashMap::new()),
            session_id,
        });

        let session_handler = Self {
            internal,
            session_id,
            app_config, // store global app config revference
        
        };

        session_handler

    }

    pub async fn send_request_from_server(&self, method: &str, params: Option<Value>) -> Result<Response, McpError> {
        let id = {
            let mut next_id = self.internal.next_outgoing_server_request_id.write().await;
            let current_id = *next_id;
            *next_id += 1;
            RequestId::Number(current_id)
        };

        let request = Request::new(id.clone(), method,params);
        let (tx, rx) = oneshot::channel();

        self.internal.pending_outgoing_server_requests.insert(id,tx);
        
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
                let mut incoming_rx_guard = self.internal.incoming_rx.write().await;
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
               
                let response_to_send = if let Some(handler) = self.app_config.custom_request_handlers.get(&method) {
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
                
                if let Some(tuple) = self.internal.http_response_map.remove(&id) {
                    eprintln!("Server: Routing response for {:?} to HTTP oneshot channel.", id);
                    if let Err(_) = tuple.1.send(Ok(response_to_send)) {
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
                
                if let Some(handler) = self.app_config.custom_notification_handlers.get(&method) {
                    if let Err(e) = handler(notification, self.internal.clone(), self.app_config.clone()).await {
                        eprintln!("Server: Handler for notification '{}' returned an error: {:?}", method, e);
                    }
                } else {
                    if self.app_config.custom_request_handlers.contains_key(&method) {
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
                   
                    if let Some(tuple) = self.internal.pending_outgoing_server_requests.remove(&id_val) {
                        if let Err(_) = tuple.1.send(Ok(response)) {
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

        *session_internal.negotiated_protocol_version.write().await = Some(response_protocol_version.clone());

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
        let params:ToolsListRequestParams = serde_json::from_value(request.params.unwrap_or_default())?;
                
        let start_offset = params.cursor
        .and_then(|c| PaginationCursor::decode(&c).ok())
        .map_or(0, |cursor| cursor.offset);


        let tools = app_config.tool_definitions.read().await;
        let end_offset = (start_offset + PAGE_SIZE).min(tools.len());

        let page_of_tools = tools[start_offset..end_offset].to_vec();
        

        let next_cursor = if end_offset < tools.len() {
            Some(PaginationCursor{offset : end_offset}.encode())
        }else{
            None
        };
     // Access global tool definitions
        let tools_list_result = ToolsListResult {
            tools: page_of_tools,
            next_cursor: next_cursor
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

      
        if let Some(handler) = app_config.tool_execution_handler_registrations.get(&tool_name) {
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

    async fn handle_prompts_list(
        request: Request,
        _session_internal: Arc<McpSessionInternal>,
        app_config: Arc<McpServer>,
    ) -> Result<Response, McpError> {
        let request_id = request.id.clone();

        let params: PromptsListRequestParams = serde_json::from_value(request.params.unwrap_or_default())?;
                
        let start_offset = params.cursor
        .and_then(|c| PaginationCursor::decode(&c).ok())
        .map_or(0, |cursor| cursor.offset);


        let prompts = app_config.prompt_definitions.read().await;
        let end_offset = (start_offset + PAGE_SIZE).min(prompts.len());

        let page_of_prompts = prompts[start_offset..end_offset].to_vec();
        

        let next_cursor = if end_offset < prompts.len() {
            Some(PaginationCursor{offset : end_offset}.encode())
        }else{
            None
        };

        let result = PromptsListResult {
            prompts: page_of_prompts,
            next_cursor
        };

        eprintln!("Server: Responding to prompts/list with {} prompts.", result.prompts.len());
        Ok(Response::new_success(request_id, Some(serde_json::to_value(result).unwrap())))
    }

    async fn handle_prompts_get(
        request: Request,
        _session_internal: Arc<McpSessionInternal>,
        app_config: Arc<McpServer>,
    ) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: PromptsGetRequestParams = serde_json::from_value(request.params.unwrap_or_default())
            .map_err(|e| McpError::ParseJson(format!("Invalid prompts/get parameters: {}", e)))?;
    
        let prompt_name = params.name;
        let arguments = params.arguments.unwrap_or_default();
    
        if let Some(handler) = app_config.prompt_handler_registrations.get(&prompt_name) {
            eprintln!("Server: Executing prompt '{}' with arguments: {:?}", prompt_name, arguments);
            
            // Call the registered handler function.
            match handler(arguments).await {
                Ok(messages) => {
                    // On success, construct the result payload.
                    let result = PromptsGetResult {
                        description: Some(format!("Result of prompt: {}", prompt_name)),
                        messages,
                    };
                    Ok(Response::new_success(request_id, Some(serde_json::to_value(result).unwrap())))
                }
                Err(e) => {
                    // If the handler returns an error, wrap it in a protocol error.
                    eprintln!("Server: Prompt '{}' execution failed: {}", prompt_name, e);
                    Err(McpError::ProtocolError(format!("Prompt execution failed: {}", e)))
                }
            }
        } else {
            eprintln!("Server: Prompt '{}' not found.", prompt_name);
            Err(McpError::ProtocolError(format!("Prompt '{}' not found.", prompt_name)))
        }
    }

    async fn handle_resources_list(
        request: Request,
        _session_internal: Arc<McpSessionInternal>,
        app_config: Arc<McpServer>,
    ) -> Result<Response, McpError> {
        let request_id = request.id.clone();

        let params: ResourcesListRequestParams = serde_json::from_value(request.params.unwrap_or_default())?;
                
        let start_offset = params.cursor
        .and_then(|c| PaginationCursor::decode(&c).ok())
        .map_or(0, |cursor| cursor.offset);


        let resources = app_config.resources_definitions.read().await;
        let end_offset = (start_offset + PAGE_SIZE).min(resources.len());

        let page_of_resources = resources[start_offset..end_offset].to_vec();
        

        let next_cursor = if end_offset < resources.len() {
            Some(PaginationCursor{offset : end_offset}.encode())
        }else{
            None
        };
        
        let result = ResourcesListResult {
            resources: page_of_resources,
            next_cursor,
        };

        eprintln!("Server: Responding to resources/list with {} resources.", result.resources.len());
        Ok(Response::new_success(request_id, Some(serde_json::to_value(result).unwrap())))
    }

    /// Handles `resources/read` by looking up and executing a handler from the unified map.
    async fn handle_resources_read(
        request: Request,
        _session_internal: Arc<McpSessionInternal>,
        app_config: Arc<McpServer>,
    ) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: ResourcesReadParams = serde_json::from_value(request.params.unwrap_or_default())?;

        if let Some(handler) = app_config.resource_handlers.get(&params.uri) {
            eprintln!("Server: Found handler for URI '{}'. Executing...", &params.uri);
            
            match handler(params.uri.clone()).await {
                Ok(contents) => {
                    let result = ResourcesReadResult { contents: vec![contents] };
                    return Ok(Response::new_success(request_id, Some(serde_json::to_value(result).unwrap())));
                }
                Err(e) => {
                    return Err(McpError::ServerError {
                        code: -32003, // Custom server error for handler failure
                        message: "Resource handler failed".to_string(),
                        data: Some(json!({ "uri": &params.uri, "error": e })),
                    });
                }
            }
        }
        
        // If no handler is found for the URI, return an error.
        eprintln!("Server: Resource not found for URI '{}'.", &params.uri);
        Err(McpError::ServerError {
            code: -32002,
            message: "Resource not found".to_string(),
            data: Some(json!({ "uri": params.uri })),
        })
    }

    async fn handle_resources_templates_list(
        request: Request,
        _session_internal: Arc<McpSessionInternal>,
        app_config: Arc<McpServer>,
    ) -> Result<Response, McpError> {
        let request_id = request.id.clone();

        let params: ResourceTemplatesListRequestParams = serde_json::from_value(request.params.unwrap_or_default())?;
                
        let start_offset = params.cursor
        .and_then(|c| PaginationCursor::decode(&c).ok())
        .map_or(0, |cursor| cursor.offset);


        let resource_templates = app_config.resource_templates.read().await;
        let end_offset = (start_offset + PAGE_SIZE).min(resource_templates.len());

        let page_of_resource_templates = resource_templates[start_offset..end_offset].to_vec();
        

        let next_cursor = if end_offset < resource_templates.len() {
            Some(PaginationCursor{offset : end_offset}.encode())
        }else{
            None
        };
        
        let result = ResourceTemplatesListResult {
            resource_templates: page_of_resource_templates,
            next_cursor
        };

        eprintln!("Server: Responding to resources/templates/list with {} templates.", result.resource_templates.len());
        Ok(Response::new_success(request_id, Some(serde_json::to_value(result).unwrap())))
    }

    async fn handle_resources_subscribe(
        request: Request,
        _session_internal: Arc<McpSessionInternal>, // We will need this to get the session ID later
        app_config: Arc<McpServer>,
    ) -> Result<Response, McpError> {
        let request_id = request.id.clone();
        let params: ResourcesSubscribeParams = serde_json::from_value(request.params.unwrap_or_default())?;

        let uri_to_subscribe = &params.uri;
        
        let session_id = _session_internal.session_id;

        if !app_config.resource_handlers.contains_key(uri_to_subscribe) {
            return Err(McpError::ServerError {
                code: -32002, // Resource not found
                message: "Resource not found".to_string(),
                data: Some(json!({ "uri": uri_to_subscribe })),
            });
        }

        let mut subscribers = app_config.resource_subscriptions.entry(uri_to_subscribe.clone()).or_default();
        if !subscribers.contains(&session_id) {
            subscribers.push(session_id);
        }

        eprintln!("Server: Session {} subscribed to resource '{}'", session_id, uri_to_subscribe);

        // The spec says "Subscription confirmed", implying a simple success response.
        Ok(Response::new_success(request_id, Some(json!({ "status": "subscribed" }))))
    }

}

pub struct McpSessionClient {
    // This is the sender for messages *into* the McpSessionHandler's incoming_rx_internal.
    pub incoming_tx_to_session_handler: mpsc::Sender<McpMessage>,
    
    // This map is used to send responses from the McpSessionHandler back to the HTTP POST handler.
    pub http_response_map: Arc<DashMap<RequestId, oneshot::Sender<Result<Response, McpError>>>>,

    // This is the receiver for messages *from* the McpSessionHandler's outgoing_tx_internal.
    // Used for future SSE GET streams. This needs to be the OTHER HALF of the outgoing_tx
    pub outgoing_rx_from_session_handler: RwLock<Option<mpsc::Receiver<McpMessage>>>, // NEW: Store outgoing_rx

    pub session_id: Uuid, // Store session ID for context/logging
}

impl McpSessionClient {
    pub fn new(
        incoming_tx: mpsc::Sender<McpMessage>,
        outgoing_rx: mpsc::Receiver<McpMessage>, // Added this parameter
        http_response_map: Arc<DashMap<RequestId, oneshot::Sender<Result<Response, McpError>>>>,
        session_id: Uuid,
    ) -> Self {
        McpSessionClient {
            incoming_tx_to_session_handler: incoming_tx,
            outgoing_rx_from_session_handler: RwLock::new(Some(outgoing_rx)),
            http_response_map,
            session_id,
        }
    }

    // Sends a Request message to the session handler and waits for its specific response via oneshot.
    pub async fn send_request_to_session_handler(&self, request: Request, timeout_duration: Duration) -> Result<Response, McpError> {
        let id_to_correlate = request.id.clone();
        
        let (oneshot_tx, oneshot_rx) = oneshot::channel();
        
        { // Store the oneshot sender in the http_response_map
            if let Some(_) = self.http_response_map.insert(id_to_correlate.clone(), oneshot_tx) {
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
    pub sessions: DashMap<Uuid, Arc<McpSessionClient>>,
    pub session_outgoing_txs: DashMap<Uuid, mpsc::Sender<McpMessage>>,
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
            sessions: DashMap::new(),
            session_outgoing_txs: DashMap::new(),
            app_config: app_config.clone(), // Store the application config
        });

        app_config.set_http_global_state(http_global_state.clone()).await;

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


        if let Some(session_id) = session_id_from_header {
            if let Some(existing_client) = state.sessions.get(&session_id) {
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
                    outgoing_tx_from_session_handler.clone(), // Sender for responses from session
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

                state.sessions.insert(new_session_id, new_session_client.clone()); // Store the McpSessionClient
                state.session_outgoing_txs.insert(new_session_id, outgoing_tx_from_session_handler);
                
                let state_clone = state.clone();

                // Spawn the persistent session logic task
                task::spawn(async move {
                    eprintln!("HTTP Server: Session logic task spawned for session ({:#?}).", new_session_id);
                    if let Err(e) = new_session_handler.process_incoming_messages().await {
                         eprintln!("HTTP Server: Session logic for session ({:#?}) terminated with error: {:?}", new_session_id, e);
                    }
                    eprintln!("Server: Cleaning up resources for disconnected session {}", new_session_id);

                    // 1. Remove the session's channels from the global maps.
                    state_clone.sessions.remove(&new_session_id);
                    state_clone.session_outgoing_txs.remove(&new_session_id);

                    // 2. Remove the session from all resource subscription lists.
                    for mut subscribers in state_clone.app_config.resource_subscriptions.iter_mut() {
                        subscribers.retain(|&id| id != new_session_id);
                    }
                    eprintln!("Server: Cleanup complete for session {}", new_session_id);
                });

                session_client_arc = new_session_client.clone(); // Use the created client for this request
                http_response_headers.insert("Mcp-Session-Id", new_session_id.to_string().parse().unwrap());

            } else {
                eprintln!("HTTP Server: Non-initialize request without Session ID header. Returning 400 Bad Request.");
                return Ok(StatusCode::BAD_REQUEST.into_response());
            }
        }
   

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
            state.sessions.get(&session_id)
        };

        let session_client = match session_client {
            Some(client) => client,
            None => return (StatusCode::NOT_FOUND, "Session not found").into_response(),
        };

        let receiver = match session_client.outgoing_rx_from_session_handler.write().await.take() {
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
