pub mod transport;
pub mod server;
pub mod client;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{from_str, json, Value};
use thiserror::Error;
// protocol version
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq,)]
pub struct JsonRpcBase {
    pub jsonrpc:String
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Request {
    #[serde(flatten)]
    pub protocol: JsonRpcBase,
    pub method: String,
    pub id : RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>
}


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Response {
    #[serde(flatten)]
    pub protocol: JsonRpcBase,
    pub id: Option<RequestId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>
}






#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    #[serde(flatten)] 
    pub base: JsonRpcBase,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Hash, Eq)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(u64)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>
}

#[derive(Debug, Error)]
pub enum McpError{
    #[error("Json serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Failed to parse JSON message: {0}")]
    ParseJson(String),
    #[error("Invalid JSON-RPC 2.0 message: {0}")]
    InvalidMessage(String),
    #[error("Received unexpected message type: {0}")]
    UnexpectedMessageType(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Protocol error: {0}")]
    ProtocolError(String),
    #[error("Authentication error: {0}")]
    AuthError(String),
    #[error("Unsupported protocol version: requested {requested}, supported versions: {supported:?}")]
    UnsupportedProtocolVersion {
        requested: String,
        supported: Vec<String>
    },
    #[error("Request timed out")]
    RequestTimeout,
    #[error("Oneshot channel send error: {0}")]
    OneshotSend(String),
    #[error("Oneshot channel receive error: {0}")]
    OneshotRecv(#[from] tokio::sync::oneshot::error::RecvError),
    
}

//--------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>, // Added 'title'
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>, // Added 'title'
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roots: Option<ClientRootsCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>, // Empty object {} -> use Value for now
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<Value>, // Empty object {} -> use Value for now
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>, // Empty object {} -> use Value for now
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRootsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>, // Empty object {} -> use Value for now
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<ServerPromptsCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<ServerResourcesCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<ServerToolsCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completions: Option<Value>, // Empty object {} -> use Value for now
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>, // Empty object {} -> use Value for now
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerResourcesCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerToolsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPromptsCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

// --- Initialize Request/Response Payloads ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeRequestParams {
    pub protocol_version: String, // MANDATORY
    pub capabilities: ClientCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_info: Option<ClientInfo>, // Now optional, but typically present
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String, // MANDATORY
    pub capabilities: ServerCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_info: Option<ServerInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}


// --- New constructors for initialize and initialized ---


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpMessage {
    /// A JSON-RPC Request. Must have a `method`, an `id`, and optionally `params`.
    Request(Request),
    /// A JSON-RPC Response. Must have an `id` and either `result` or `error`.
    Response(Response),
    /// A JSON-RPC Notification. Must have a `method` but NO `id`.
    Notification(Notification),
}


// ---------------------   Tool Call-related Strucutres ---------------


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputSchema {
    #[serde(rename = "$ref")] 
    Reference(String), // Reference to a schema definition
    Inline(Value), // Inline schema definition
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>, 
    pub description: String,
    pub input_schema: InputSchema, // JSON Schema defining expected parameters
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<InputSchema>, // Optional JSON Schema defining expected output structure
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_auth: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>, 
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsListRequestParams {
    pub cursor: Option<String>, // Optional cursor for pagination
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsListResult {
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>, // cursor for the next page of results
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCallRequestParams {
    #[serde(rename = "name")] // Renamed from tool_name
    pub tool_name: String,
    #[serde(rename = "arguments")] // Renamed from tool_parameters
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_parameters: Option<Value>,
}

/// Represents a single content block within a tool's output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")] // Use internal tagging for content blocks
pub enum ToolOutputContentBlock {
    #[serde(rename = "text")]
    Text {
        text: String,
    },
    #[serde(rename = "image")]
    Image {
        data: String, // Base64-encoded image data
        mime_type: String,
        #[serde(flatten)] // Allows for arbitrary other fields in the block
        extra_fields: HashMap<String, Value>, // For other image specific fields
    },
    #[serde(rename = "audio")]
    Audio {
        data: String, // Base64-encoded audio data
        mime_type: String,
        #[serde(flatten)] // Allows for arbitrary other fields in the block
        extra_fields: HashMap<String, Value>, // For other audio specific fields
    },
    #[serde(rename = "resource_link")]
    ResourceLink {
        uri: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(flatten)] // Allows for arbitrary other fields in the block
        extra_fields: HashMap<String, Value>, // For other link specific fields
    },
    #[serde(rename = "resource")]
    EmbeddedResource {
        resource: Resource, // Assuming a Resource struct exists or will be defined
        #[serde(flatten)] // Allows for arbitrary other fields in the block
        extra_fields: HashMap<String, Value>,
    },
    // Allows for future/custom content types not explicitly defined
    #[serde(other)]
    Other // Fallback for unknown "type" strings
}


// Assuming a basic Resource struct for `Embedded Resource` type.
// You might need to expand this when implementing the full Resource feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>, // For text-based embedded resources
    // ... other fields for a full resource ...
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCallResult {
    pub content: Vec<ToolOutputContentBlock>, // The output content blocks (unstructured)
    pub is_error: bool, // Indicates if the tool execution resulted in an error
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>, // Optional error message if is_error is true

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>, // NEW: For structured output
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>, // NEW: Optional metadata field
}



impl Request {
    pub fn new <I : Into<RequestId>, P:Into<Option<Value>>>(id: I, method: &str, params:P) -> Self {
        Request { protocol: JsonRpcBase { jsonrpc: "2.0".to_string() },
             method: method.to_string(), 
             id: id.into(),
             params: params.into()   
            }
    }

    pub fn new_initialize(id: RequestId, protocol_version:String, capabilities: ClientCapabilities, client_info:Option<ClientInfo>) -> Self {
        let params = InitializeRequestParams {
            protocol_version,
            capabilities,
            client_info: client_info,
        };

        Request::new(id, "initialize", params)

    }

    pub fn to_json(&self) -> Result<String, McpError> {
        serde_json::to_string(self).map_err(
            |e| McpError::Serialization(e)
        )
    }

    pub fn from_json(json: &str) -> Result<Self, McpError> {
        serde_json::from_str(json).map_err(
            |e| McpError::Serialization(e)
        )
    }
}

impl Response {
    pub fn new_success<I: Into<RequestId>, R: Into<Option<Value>>>(id: I, result: R) -> Self {
        Response {
            protocol: JsonRpcBase { jsonrpc: "2.0".to_string() },
            id: Some(id.into()),
            result: result.into(),
            error: None,
        }
    }

    pub fn new_error(id: Option<RequestId>, code: i32, message: &str, data: Option<Value>) -> Self {
        Response {
            protocol: JsonRpcBase { jsonrpc: "2.0".to_string() },
            id,
            result: None,
            error: Some(ResponseError {
                code,
                message: message.to_string(),
                data,
            }),
        }
    }
    /// Creates a successful `initialize` response.
    pub fn new_initialize_success(id: RequestId, result: InitializeResult) -> Self {
        Response::new_success(
            id,
            Some(serde_json::to_value(result).expect("Failed to serialize InitializeResult"))
        )
    }

    pub fn new_unsupported_protocol_error(
        id: Option<RequestId>,
        requested_version: String,
        supported_versions: Vec<String>,
    ) -> Self {
        Response::new_error(
            id,
            -32602, // Standard JSON-RPC InvalidParams error code, common for protocol issues
            "Unsupported protocol version",
            Some(json!({
                "supported": supported_versions,
                "requested": requested_version
            })),
        )
    }

    pub fn to_json(&self) -> Result<String, McpError> {
        serde_json::to_string(self).map_err(
            |e| McpError::Serialization(e)
        )
    }

    pub fn from_json(json_str: &str) -> Result<Self, McpError> {
        serde_json::from_str(json_str).map_err(
            |e| McpError::Serialization(e)
        )
    }
}

impl Notification {
    pub fn new<P: Into<Option<Value>>>(method: &str, params: P) -> Self {
        Notification {
            base: JsonRpcBase { jsonrpc: "2.0".to_string() },
            method: method.to_string(),
            params: params.into(),
        }
    }

    pub fn new_initialized() -> Self {
        Notification::new(
            "notifications/initialized",
            None // No params for this notification
        )
    }

    pub fn to_json(&self) -> Result<String, McpError> {
        serde_json::to_string(self).map_err(
            |e| McpError::Serialization(e)
        )
    }

    pub fn from_json(json_str: &str) -> Result<Self, McpError> {
        serde_json::from_str(json_str).map_err(
            |e| McpError::Serialization(e)
        )
    }
}

impl From<InitializeRequestParams> for Option<Value> {
    fn from(params: InitializeRequestParams) -> Self {
        Some(serde_json::to_value(params).unwrap())
    }
}

impl From<InitializeResult> for Option<Value> {
    fn from(result: InitializeResult) -> Self {
        Some(serde_json::to_value(result).unwrap())
    }
}

impl From<String> for RequestId {
    fn from(s:String) -> Self {
        RequestId::String(s)
    }
}

impl From<&str> for RequestId {
    fn from(s: &str) -> Self {
        RequestId::String(s.to_string())
    }
}

impl From<u64> for RequestId {
    fn from(n: u64) -> Self {
        RequestId::Number(n)
    }
}

// impl From<Option<RequestId>> for RequestId {
//     fn from(id: Option<RequestId>) -> Self {
//         match id {
//             Some(id) => id,
//             None => RequestId::Number(0)
//         }
//     }
// }
impl McpMessage {
    pub fn from_json(json_str: &str) -> Result<Self, McpError> {
        eprintln!("McpMessage::from_json: Attempting to parse: '{}'", json_str.trim());

        let raw_value: Value = serde_json::from_str(json_str)
            .map_err(|e| {
                eprintln!("McpMessage::from_json: Serialization error (invalid JSON string) for '{}': {}", json_str.trim(), e);
                McpError::Serialization(e) // This is still correct for the initial `from_str`
            })?;

        // ... (jsonrpc_version check) ...

        let id_field_present = raw_value.get("id").is_some();
        let method_field_present = raw_value.get("method").is_some();
        let result_field_present = raw_value.get("result").is_some();
        let error_field_present = raw_value.get("error").is_some();

        if id_field_present {
            if method_field_present {
                let request: Request = serde_json::from_value(raw_value.clone())
                    .map_err(|e| {
                        eprintln!("McpMessage::from_json: Failed to deserialize as Request, but looked like one: {}", e);
                        // Use the new ParseJson variant here
                        McpError::ParseJson(format!("Failed to deserialize as Request: {}", e))
                    })?;
                eprintln!("McpMessage::from_json: Successfully parsed as Request.");
                Ok(McpMessage::Request(request))
            } else if result_field_present || error_field_present {
                let response: Response = serde_json::from_value(raw_value)
                    .map_err(|e| {
                        eprintln!("McpMessage::from_json: Failed to deserialize as Response, but looked like one: {}", e);
                        // Use the new ParseJson variant here
                        McpError::ParseJson(format!("Failed to deserialize as Response: {}", e))
                    })?;

                if response.result.is_some() && response.error.is_some() {
                    return Err(McpError::InvalidMessage("Response: 'result' and 'error' cannot both be set".to_string()));
                }
                if response.result.is_none() && response.error.is_none() {
                    return Err(McpError::InvalidMessage("Response: Either 'result' or 'error' must be set".to_string()));
                }

                eprintln!("McpMessage::from_json: Successfully parsed as Response.");
                Ok(McpMessage::Response(response))
            } else {
                Err(McpError::InvalidMessage(format!("Message has 'id' but is neither a Request nor a Response (missing 'method' or 'result'/'error'): {}", json_str.trim())))
            }
        } else {
            if method_field_present {
                let notification: Notification = serde_json::from_value(raw_value)
                    .map_err(|e| {
                        eprintln!("McpMessage::from_json: Failed to deserialize as Notification, but looked like one: {}", e);
                        // Use the new ParseJson variant here
                        McpError::ParseJson(format!("Failed to deserialize as Notification: {}", e))
                    })?;
                eprintln!("McpMessage::from_json: Successfully parsed as Notification.");
                Ok(McpMessage::Notification(notification))
            } else {
                Err(McpError::InvalidMessage(format!("Message is neither a Request, Response, nor Notification (missing 'id' and 'method'): {}", json_str.trim())))
            }
        }
    }
    
     pub fn to_json(&self) -> Result<String, McpError> {
        serde_json::to_string(self).map_err( |e|McpError::Serialization(e))
    }
}