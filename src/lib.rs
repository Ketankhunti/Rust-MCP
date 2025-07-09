use serde::{Deserialize, Serialize};
use serde_json::{from_str, Value};
use thiserror::Error;
// protocol version
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawRequest {
    #[serde(flatten)]
    pub protocol: JsonRpcBase,
    pub id: Option<RequestId>, // Allows missing or null ID for initial parsing
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Response {
    #[serde(flatten)]
    pub protocol: JsonRpcBase,
    pub id: RequestId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct RawResponse {
    #[serde(flatten)]
    pub protocol: JsonRpcBase,
    pub id: Option<RequestId>, // Allows missing or null ID for initial parsing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)] 
enum RawMcpMessage {
    Request(RawRequest),
    Response(RawResponse),
    Notification(Notification), // Notification ID is already correctly handled as absence
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    #[serde(flatten)] 
    pub base: JsonRpcBase,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

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

impl Request {
    pub fn new <I : Into<RequestId>, P:Into<Option<Value>>>(id: I, method: &str, params:P) -> Self {
        Request { protocol: JsonRpcBase { jsonrpc: "2.0".to_string() },
             method: method.to_string(), 
             id: id.into(),
             params: params.into()   
            }
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
            id: id.into(),
            result: result.into(),
            error: None,
        }
    }

    pub fn new_error<I: Into<RequestId>>(id: I, code: i32, message: &str, data: Option<Value>) -> Self {
        Response {
            protocol: JsonRpcBase { jsonrpc: "2.0".to_string() },
            id: id.into(),
            result: None,
            error: Some(ResponseError {
                code,
                message: message.to_string(),
                data,
            }),
        }
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

impl McpMessage {
    pub fn from_json(json: &str) -> Result<Self, McpError> {

        // check if json contains jsonrpc protocol version
        let raw_msg:RawMcpMessage = from_str(json)
            .map_err(McpError::Serialization)?;

        match raw_msg {
            RawMcpMessage::Request(raw_req) => {
                if raw_req.protocol.jsonrpc != "2.0" {
                    return Err(McpError::InvalidMessage(
                        "Request: 'jsonrpc' must be '2.0'".to_string(),
                    ));
                }

                let id = raw_req.id.ok_or_else(|| {
                    McpError::InvalidMessage("Request: 'id' field is required and must not be null".to_string())
                })?;
               
                Ok(McpMessage::Request(Request {
                    protocol: raw_req.protocol,
                    id,
                    method: raw_req.method,
                    params: raw_req.params,
                }))
            },
            RawMcpMessage::Response(raw_res) => {
                if raw_res.protocol.jsonrpc != "2.0" {
                    return Err(McpError::InvalidMessage(
                        "Response: 'jsonrpc' must be '2.0'".to_string(),
                    ));
                }
                // Response ID must be present (unless it's an error response for unknown ID, which is handled implicitly by `ResponseError`'s `id: Option<RequestId>` if we had it, but for now we expect it)
                let id = raw_res.id.ok_or_else(|| {
                    McpError::InvalidMessage("Response: 'id' field is required and must not be null".to_string())
                })?;

                // Either result OR error MUST be set, not both.
                if raw_res.result.is_some() && raw_res.error.is_some() {
                    return Err(McpError::InvalidMessage(
                        "Response: 'result' and 'error' cannot both be set".to_string(),
                    ));
                }
                if raw_res.result.is_none() && raw_res.error.is_none() {
                    return Err(McpError::InvalidMessage(
                        "Response: Either 'result' or 'error' must be set".to_string(),
                    ));
                }

                Ok(McpMessage::Response(Response {
                    protocol: raw_res.protocol,
                    id,
                    result: raw_res.result,
                    error: raw_res.error,
                }))
            }
            RawMcpMessage::Notification(notif) => {
                if notif.base.jsonrpc != "2.0" {
                    return Err(McpError::InvalidMessage(
                        "Notification: 'jsonrpc' must be '2.0'".to_string(),
                    ));
                }
                Ok(McpMessage::Notification(notif))
            }
        }
    }

    pub fn to_json(&self) -> Result<String, McpError> {
        serde_json::to_string(self).map_err( |e|McpError::Serialization(e))
    }
}