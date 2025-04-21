use anyhow::{anyhow, Result};
use gemini_core::rpc_types::ServerCapabilities;
use gemini_core::types::{FunctionDeclaration, Tool};
use gemini_ipc::daemon_messages::{
    DaemonRequest, DaemonResponse, DaemonResult, ResponsePayload, ResponseStatus,
};
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Client to communicate with the MCP host daemon
#[derive(Clone)]
pub struct McpHostClient {
    socket_path: PathBuf,
}

impl McpHostClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }
    
    /// Helper function to determine the default socket path if none is provided
    pub fn get_default_socket_path() -> PathBuf {
        let base_dir = dirs::runtime_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let socket_dir = base_dir.join("gemini-cli");
        socket_dir.join("mcp-hostd.sock")
    }
    
    /// Connect to the MCP host daemon and send a request
    async fn send_request(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        // Connect to the Unix socket
        let mut stream = UnixStream::connect(&self.socket_path).await
            .map_err(|e| anyhow!("Failed to connect to MCP host daemon: {}", e))?;
        
        // Serialize the request
        let request_bytes = serde_json::to_vec(&request)
            .map_err(|e| anyhow!("Failed to serialize request: {}", e))?;
        
        // Send the length prefix followed by the data
        stream.write_u32(request_bytes.len() as u32).await
            .map_err(|e| anyhow!("Failed to write request length: {}", e))?;
        
        stream.write_all(&request_bytes).await
            .map_err(|e| anyhow!("Failed to write request data: {}", e))?;
        
        // Read the response length
        let response_len = stream.read_u32().await
            .map_err(|e| anyhow!("Failed to read response length: {}", e))?;
        
        // Read the response payload
        let mut buffer = vec![0u8; response_len as usize];
        stream.read_exact(&mut buffer).await
            .map_err(|e| anyhow!("Failed to read response data: {}", e))?;
        
        // Deserialize the response
        let response: DaemonResponse = serde_json::from_slice(&buffer)
            .map_err(|e| anyhow!("Failed to deserialize response: {}", e))?;
        
        Ok(response)
    }
    
    /// Get capabilities from the MCP host daemon
    pub async fn get_capabilities(&self) -> Result<ServerCapabilities> {
        let response = self.send_request(DaemonRequest::GetCapabilities).await?;
        
        match response {
            DaemonResponse { status: ResponseStatus::Success, payload: ResponsePayload::Result(DaemonResult::Capabilities(caps)) } => {
                Ok(caps)
            }
            DaemonResponse { status: ResponseStatus::Error, payload: ResponsePayload::Error(error) } => {
                Err(anyhow!("MCP host daemon error: {}", error.message))
            }
            _ => {
                Err(anyhow!("Unexpected response from MCP host daemon"))
            }
        }
    }
    
    /// Execute a tool via the MCP host daemon
    pub async fn execute_tool(&self, server_name: &str, tool_name: &str, args: Value) -> Result<Value> {
        let request = DaemonRequest::ExecuteTool { 
            server: server_name.to_owned(), 
            tool: tool_name.to_owned(), 
            args,
        };
        
        let response = self.send_request(request).await?;
        
        match response {
            DaemonResponse { status: ResponseStatus::Success, payload: ResponsePayload::Result(DaemonResult::ExecutionOutput(output)) } => {
                Ok(output)
            }
            DaemonResponse { status: ResponseStatus::Error, payload: ResponsePayload::Error(error) } => {
                Err(anyhow!("Tool execution failed: {}", error.message))
            }
            _ => {
                Err(anyhow!("Unexpected response from MCP host daemon"))
            }
        }
    }
}

/// Generate a Tool declaration based on server capabilities
pub fn generate_tool_declarations(tools: &[gemini_core::rpc_types::Tool]) -> Tool {
    let function_declarations: Vec<FunctionDeclaration> = tools
        .iter()
        .filter_map(|tool| {
            // Ensure name meets Gemini API requirements (starts with letter/underscore, alphanumeric with dots/underscores/dashes)
            let name = tool.name.clone();
            if !is_valid_function_name(&name) {
                tracing::warn!("Skipping tool with invalid name: {}", name);
                return None;
            }
            
            // Clone parameters and ensure the type field is set
            let mut parameters = tool.parameters.clone().unwrap_or_else(|| serde_json::json!({}));
            
            // Ensure parameters has a type field at the root level
            if let Some(obj) = parameters.as_object_mut() {
                if !obj.contains_key("type") {
                    // Add the object type if missing
                    obj.insert("type".to_string(), serde_json::json!("object"));
                }
            } else {
                // If parameters is not an object, create a minimal valid schema
                parameters = serde_json::json!({
                    "type": "object",
                    "properties": {}
                });
            }
            
            Some(FunctionDeclaration {
                name,
                description: tool.description.clone(),
                parameters,
            })
        })
        .collect();

    Tool {
        function_declarations,
    }
}

/// Check if a function name is valid according to Gemini API requirements
fn is_valid_function_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    
    // First character must be a letter or underscore
    let first_char = name.chars().next().unwrap();
    if !first_char.is_alphabetic() && first_char != '_' {
        return false;
    }
    
    // Rest of the characters must be alphanumeric, underscore, dash, or dot
    name.chars().all(|c| {
        c.is_alphanumeric() || c == '_' || c == '-' || c == '.'
    })
}

/// Sanitize JSON schema for Gemini API
fn sanitize_json_schema(schema: Value) -> Value {
    // Gemini has stricter requirements for JSON schema than some MCP servers might provide
    // This is a simple implementation that keeps the schema as-is
    // In a real implementation, you might want to:
    // - Remove unsupported keywords
    // - Ensure all required fields are present
    // - Convert formats to supported ones
    schema
} 