// Defines the core JSON-RPC 2.0 structures and MCP specific types.

use serde::{Deserialize, Serialize};
use serde_json::{self, Value, json};

// --- Core JSON-RPC 2.0 Structures ---

// Union type for all message types
#[derive(Debug, Clone)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

// Implement deserialization for Message
impl<'de> Deserialize<'de> for Message {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        
        // Check for id + method = Request
        // Check for id without method = Response
        // Check for method without id = Notification
        
        if let Some(_id) = value.get("id") {
            if value.get("method").is_some() {
                // Request
                Ok(Message::Request(serde_json::from_value(value).map_err(|e| {
                    serde::de::Error::custom(format!("Invalid Request: {}", e))
                })?))
            } else {
                // Response
                Ok(Message::Response(serde_json::from_value(value).map_err(|e| {
                    serde::de::Error::custom(format!("Invalid Response: {}", e))
                })?))
            }
        } else if value.get("method").is_some() {
            // Notification
            Ok(Message::Notification(serde_json::from_value(value).map_err(|e| {
                serde::de::Error::custom(format!("Invalid Notification: {}", e))
            })?))
        } else {
            Err(serde::de::Error::custom("Invalid message format"))
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Request {
    pub jsonrpc: String, // Should always be "2.0"
    pub id: Option<Value>, // Request ID (number or string), null if notification
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>, // Structured value or array
}

impl Request {
    pub fn new(id: Option<Value>, method: String, params: Option<Value>) -> Self {
        Request {
            jsonrpc: "2.0".to_string(),
            id,
            method,
            params,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    pub jsonrpc: String, // Should always be "2.0"
    pub id: Value, // Must match the request ID (can be null for special cases)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl Response {
    // Helper to get result, converting the Option<Value> to a Result
    pub fn result(&self) -> Result<Value, JsonRpcError> {
        if let Some(error) = &self.error {
            Err(error.clone())
        } else if let Some(result) = &self.result {
            Ok(result.clone())
        } else {
            // This shouldn't happen in a well-formed response
            Err(JsonRpcError {
                code: -32603, // Internal error
                message: "Response missing both result and error".to_string(),
                data: None,
            })
        }
    }
    
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Notification {
    pub jsonrpc: String, // Should always be "2.0"
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl Notification {
    pub fn new(method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method,
            params,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

// --- MCP Specific Types ---

// `initialize` request parameters
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub client_info: ClientInfo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>, // Optional trace ID
    // Add other fields as needed, e.g., capabilities from the client
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
    // Add other relevant client info if needed
}

// `initialize` response result
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub server_info: ServerInfo,
    pub capabilities: ServerCapabilities,
    // Add other fields as needed
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    // Add other relevant server info if needed
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>, // JSON Schema for parameters
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub name: String,
    pub description: Option<String>,
}

// --- MCP Tool Execution Types ---

// `mcp/tool/execute` request parameters
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteToolParams {
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
pub struct ExecuteToolResult {
    pub result: Value, // The output of the tool execution
}

// LSP/MCP LogMessage notification params
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LogMessageParams {
    #[serde(rename = "type")]
    pub type_: i32, // 1=Error, 2=Warning, 3=Info, 4=Log
    pub message: String,
}

// LSP/MCP $/progress notification params
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProgressParams {
    pub token: Value, // The progress token (number or string)
    pub value: Value, // The progress data
}

// LSP/MCP $/cancelRequest notification params
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CancelParams {
    pub id: Value, // The request id to cancel
}

// `resource/get` request parameters
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetResourceParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>, // Optional parameters for resource retrieval
}

// Helper function to create a log message notification
pub fn create_log_notification(message: &str, level: i32) -> Notification {
    let params = Some(json!({
        "type": level,
        "message": message
    }));
    Notification::new("window/logMessage".to_string(), params)
}