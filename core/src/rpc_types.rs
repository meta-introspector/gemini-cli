use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
// Added for Tool/Resource parameters if they use Value which might be Map

// Basic JSON-RPC Structures - Moved from mcp/src/rpc.rs

/// Represents a JSON-RPC Request object.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Request {
    pub jsonrpc: String,   // Should always be "2.0"
    pub id: Option<Value>, // Request ID (number or string), null if notification
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>, // Structured value or array
}

impl Request {
    pub fn new(id: Option<Value>, method: String, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method,
            params,
        }
    }
}

/// Represents a JSON-RPC Response object.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Response {
    pub jsonrpc: String, // Should always be "2.0"
    pub id: Value,       // Must match the request ID (can be null for special cases)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl Response {
    /// Convenience method to extract the result or return the error.
    pub fn result(&self) -> Result<Value, JsonRpcError> {
        if let Some(err) = &self.error {
            Err(err.clone())
        } else if let Some(res) = &self.result {
            Ok(res.clone())
        } else {
            // This case should ideally not happen for a valid response unless id is null?
            // But LSP spec allows null result for void methods. Let's return Value::Null.
            Ok(Value::Null)
        }
    }
}

/// Represents a JSON-RPC Error object.
#[derive(Error, Serialize, Deserialize, Debug, Clone)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JSON-RPC Error (code {}): {}", self.code, self.message)?;
        if let Some(data) = &self.data {
            write!(f, " (Data: {})", data)?;
        }
        Ok(())
    }
}

// MCP Specific Structures - Moved from mcp/src/rpc.rs

/// Server capabilities reported during initialization.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub resources: Vec<Resource>,
    // Add other capabilities like prompts, sampling support here if needed
}

/// Definition of a tool provided by an MCP server.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>, // JSON Schema for parameters
                                   // Add result schema if needed
}

/// Definition of a resource managed by an MCP server.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Resource {
    pub name: String,
    pub description: Option<String>,
    // Add schema/type information if needed
}
