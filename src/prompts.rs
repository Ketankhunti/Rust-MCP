use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use crate::Resource; // Re-use the existing Resource struct
use mcp_sdk_types::PromptMessageRole;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptsGetResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
}



#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptMessage {
    pub role: PromptMessageRole,
    pub content: PromptMessageContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum PromptMessageContent {
    Text {
        text: String,
    },
    Image {
        data: String, // Base64-encoded
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Audio {
        data: String, // Base64-encoded
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Resource {
        resource: Resource,
        #[serde(flatten)]
        extra_fields: HashMap<String, Value>,
    },
}