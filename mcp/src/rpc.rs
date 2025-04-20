// Defines the core JSON-RPC 2.0 structures and MCP specific types.

use serde::{Deserialize, Serialize};
use serde_json::{self, json, Value};
// Import shared types from gemini_core using the specific exports from lib.rs
use gemini_core::ServerCapabilities;

// --- Core JSON-RPC 2.0 Structures ---

// Removed Message enum and its Deserialize impl as they are unused dead code
/*
#[derive(Debug, Clone)]
pub(crate) enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

impl<'de> Deserialize<'de> for Message {
   // ... impl ...
}
*/

// Request, Response, JsonRpcError are now imported from gemini_core::{...}
// but not used directly in this file.

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct Notification {
    pub jsonrpc: String, // Should always be "2.0"
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    pub(crate) fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
        }
    }
}

// --- MCP Specific Types ---

// `initialize` request parameters
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeParams {
    pub client_info: ClientInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>, // Optional trace ID
                               // Add other fields as needed, e.g., capabilities from the client
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ClientInfo {
    pub name: String,
    pub version: String,
    // Add other relevant client info if needed
}

// `initialize` response result
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitializeResult {
    pub server_info: ServerInfo,
    pub capabilities: ServerCapabilities, // Needs ServerCapabilities import
                                          // Add other fields as needed
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ServerInfo {
    pub name: String,
    pub version: String,
    // Add other relevant server info if needed
}

// ServerCapabilities, Tool, Resource are defined in gemini_core::rpc_types

// --- MCP Tool Execution Types ---

// `mcp/tool/execute` request parameters
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecuteToolParams {
    // Workaround: Use "name" on the wire for backward compatibility
    #[serde(rename = "name")]
    pub tool_name: String,
    // Workaround: Use "args" on the wire for backward compatibility
    #[serde(rename = "args")]
    pub arguments: Value, // JSON object or value
}

// `mcp/tool/execute` response result
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExecuteToolResult {
    pub result: Value, // The output of the tool execution
}

// LSP/MCP LogMessage notification params
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct LogMessageParams {
    #[serde(rename = "type")]
    pub type_: i32, // 1=Error, 2=Warning, 3=Info, 4=Log
    pub message: String,
}

// LSP/MCP $/progress notification params
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct ProgressParams {
    pub token: Value, // The progress token (number or string)
    pub value: Value, // The progress data
}

// LSP/MCP $/cancelRequest notification params
#[derive(Serialize, Deserialize, Debug, Clone)]
pub(crate) struct CancelParams {
    pub id: Value, // The request id to cancel
}

// `resource/get` request parameters
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GetResourceParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>, // Optional parameters for resource retrieval
}

// Helper function to create a log message notification
pub(crate) fn create_log_notification(message: &str, level: i32) -> Notification {
    let params = Some(json!({
        "type": level,
        "message": message
    }));
    Notification::new("window/logMessage".to_string(), params)
}
