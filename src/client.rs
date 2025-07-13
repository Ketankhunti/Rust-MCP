use crate::{McpError, McpMessage, Request, RequestId, Response, ClientCapabilities, ClientInfo, InitializeResult, ServerCapabilities, Notification};
use crate::transport::StdioTransport;
use tokio::sync::{mpsc, oneshot};
use tokio::task;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::u64;
use anyhow::Result;
use serde_json::Value;

struct PendingRequest {
    sender: oneshot::Sender<Result<Response, McpError>>,
}

pub struct McpClient {
    transport: StdioTransport,
    next_request_id: Arc<Mutex<u64>>,
    pending_requests: Arc<Mutex<HashMap<RequestId, PendingRequest>>>,
    current_protocol_version: String,
    negotiated_protocol_version: Arc<Mutex<Option<String>>>,
}

impl McpClient {
    const LATEST_SUPPORTED_PROTOCOL_VERSION: &'static str = "2025-06-18";

    pub fn new() -> Self {
        McpClient {
            transport: StdioTransport::new(),
            next_request_id: Arc::new(Mutex::new(0)),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            current_protocol_version: Self::LATEST_SUPPORTED_PROTOCOL_VERSION.to_string(),
            negotiated_protocol_version: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn run(&mut self) -> Result<(), McpError> {
        
        loop {
            tokio::select! {
                msg = self.transport.recv() => {
                    match msg {
                        Ok(message) => {
                            self.handle_message(message).await?;
                        },
                        Err(McpError::Io(e)) if e.kind() == std::io::ErrorKind::ConnectionAborted => {
                            eprintln!("Client: Transport connection aborted, client exiting.");
                            break;
                        },
                        Err(e) => {
                            eprintln!("Client: Error receiving message: {:?}", e);
                            break;
                        }
                    }
                }
              
            }
        
    }
    Ok(())
}

    fn generate_request_id(&self) -> RequestId {
        let mut id_guard = self.next_request_id.lock().unwrap();
        let id = *id_guard;
        *id_guard += 1;
        RequestId::Number(id)
    }

    async fn send_request_internal(&self, method: &str, params: Option<Value>) -> Result<Response, McpError> {
        let id = self.generate_request_id();
        let request = Request::new(id.clone(), method, params);
        let (tx, rx) = oneshot::channel();

        {
            let mut pending_requests = self.pending_requests.lock().unwrap();
            pending_requests.insert(id.clone(), PendingRequest { sender: tx });
        }

        self.transport.send(McpMessage::Request(request)).await?;

        // TODO: Implement timeout here using `tokio::time::timeout`.
        rx.await.map_err(McpError::OneshotRecv)?
    }

    async fn handle_message(&self, message: McpMessage) -> Result<(), McpError> {
        match message {
            McpMessage::Response(response) => {
                let id = response.id.clone();
                let mut pending_requests = self.pending_requests.lock().unwrap();
                if let Some(pending_req) = pending_requests.remove(&id.as_ref().unwrap()) {
                    pending_req.sender.send(Ok(response))
                        .map_err(|_| McpError::OneshotSend(
                            format!("Failed to send response back for ID: {:?}. Receiver was dropped.", id)
                        ))?;
                } else {
                    eprintln!("Received unhandled response for ID: {:?}", id);
                }
            }
            McpMessage::Request(request) => {
                eprintln!("Client received unhandled request: {:?}", request);
                if let RequestId::Number(id) = request.id {
                    let error_response = Response::new_error(
                        Some(id.into()),
                        -32601,
                        &format!("Method '{}' not supported by client", request.method),
                        None
                    );
                    self.transport.send(McpMessage::Response(error_response)).await?;
                } else {
                }
            }
            McpMessage::Notification(notification) => {
                eprintln!("Client received notification: {:?}", notification);
            }
        }
        Ok(())
    }

    pub async fn initialize(
        &self,
        client_capabilities: ClientCapabilities,
        client_info: Option<ClientInfo>,
    ) -> Result<InitializeResult, McpError> {
        let initialize_request_params = crate::InitializeRequestParams {
            protocol_version: self.current_protocol_version.clone(),
            capabilities: client_capabilities,
            client_info,
        };

        let response = self.send_request_internal(
            "initialize",
            Some(serde_json::to_value(initialize_request_params)
                .map_err(|e| McpError::Serialization(e))?
            )
        ).await?;

        let init_result: InitializeResult = if let Some(result_value) = response.result {
            serde_json::from_value(result_value)
                .map_err(|e| McpError::ProtocolError(format!("Failed to deserialize InitializeResult: {}", e)))?
        } else if let Some(error_obj) = response.error {
            if error_obj.code == -32602 && error_obj.message == "Unsupported protocol version" {
                let data: Value = error_obj.data.unwrap_or_default();
                let supported_versions: Vec<String> = serde_json::from_value(data["supported"].clone()).unwrap_or_default();
                let requested_version: String = serde_json::from_value(data["requested"].clone()).unwrap_or_default();
                return Err(McpError::UnsupportedProtocolVersion {
                    requested: requested_version,
                    supported: supported_versions,
                });
            }
            return Err(McpError::ProtocolError(format!(
                "Initialize request failed with server error: Code={}, Message={}", error_obj.code, error_obj.message
            )));
        } else {
            return Err(McpError::ProtocolError("Initialize response had neither result nor error".to_string()));
        };

        if init_result.protocol_version != self.current_protocol_version {
            eprintln!("WARNING: Server responded with protocol version '{}', client requested and supports '{}'.",
                init_result.protocol_version, self.current_protocol_version);
            eprintln!("         For a robust SDK, you would now check if the server's version is also supported by the client.");
            eprintln!("         If not, the client SHOULD disconnect. Continuing for demonstration purposes.");
        }

        *self.negotiated_protocol_version.lock().unwrap() = Some(init_result.protocol_version.clone());

        self.transport.send(McpMessage::Notification(Notification::new_initialized())).await?;
        println!("Sent initialized notification.");

        Ok(init_result)
    }

    pub async fn call_method(&self, method: &str, params: Option<Value>) -> Result<Response, McpError> {
        if self.negotiated_protocol_version.lock().unwrap().is_none() {
            return Err(McpError::ProtocolError(
                "Cannot send requests before initialization handshake is complete.".to_string()
            ));
        }
        self.send_request_internal(method, params).await
    }
}


