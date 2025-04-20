use anyhow::Result;
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use std::error::Error;
use std::sync::Arc;
use async_trait::async_trait;

// Import required types from memory crate
use gemini_memory::broker::{McpHostInterface, Capabilities, ToolDefinition};
use gemini_memory::schema::EmbeddingModelVariant;

// Import the daemon request/response types from gemini-mcp
use gemini_mcp::ipc::{DaemonRequest, DaemonResponse, ResponseStatus};
use gemini_core::rpc_types::{ServerCapabilities, Request, Response, JsonRpcError};

/// Represents a client for connecting to the MCP Host Daemon.
#[derive(Clone)]
pub struct McpDaemonClient {
    socket_path: PathBuf,
}

impl McpDaemonClient {
    /// Creates a new instance of the MCP Daemon Client.
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    /// Helper function to determine the default socket path.
    pub fn get_default_socket_path() -> Result<PathBuf> {
        let base_dir = dirs::runtime_dir()
            .or_else(dirs::data_local_dir)
            .ok_or_else(|| anyhow::anyhow!("Could not determine runtime or data local directory"))?;
        let socket_dir = base_dir.join("gemini-cli");
        // Ensure the directory exists before returning the path
        std::fs::create_dir_all(&socket_dir).map_err(|e| anyhow::anyhow!("Failed to create socket directory: {}", e))?;
        Ok(socket_dir.join("mcp-hostd.sock"))
    }

    /// Tests if a connection to the daemon can be established.
    pub async fn test_connection(&self) -> Result<bool> {
        match UnixStream::connect(&self.socket_path).await {
            Ok(_) => Ok(true),
            Err(e) => {
                debug!("Connection to daemon failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Requests all server capabilities from the daemon.
    pub async fn get_all_capabilities(&self) -> Result<ServerCapabilities> {
        let request = DaemonRequest::GetCapabilities;
        let response = self.send_request(request).await?;
        
        match response.status {
            ResponseStatus::Success => {
                // Extract the capabilities from the response
                // This assumes the DaemonResponse contains a DaemonResult::Capabilities variant
                match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Result(gemini_mcp::ipc::DaemonResult::Capabilities(caps)) => Ok(caps),
                    gemini_mcp::ipc::ResponsePayload::Error(err) => Err(anyhow::anyhow!("Daemon returned error for capabilities: {}", err.message)),
                    _ => Err(anyhow::anyhow!("Unexpected response payload format for capabilities")),
                }
            }
            ResponseStatus::Error => {
                // Extract the error message and return it
                let error_msg = match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Error(error) => error.message,
                    _ => "Unknown error occurred while getting capabilities".to_string(),
                };
                Err(anyhow::anyhow!("Daemon error: {}", error_msg))
            }
        }
    }

    /// Executes a tool on a specified server via the daemon.
    pub async fn execute_tool(&self, server: &str, tool: &str, args: Value) -> Result<Value> {
        debug!("Sending execute_tool request: server={}, tool={}, args={}", 
               server, tool, serde_json::to_string(&args).unwrap_or_else(|_| "unable to serialize".to_string()));
        
        let request = DaemonRequest::ExecuteTool {
            server: server.to_string(),
            tool: tool.to_string(),
            args: args.clone(),
        };
        
        let response = match self.send_request(request).await {
            Ok(resp) => resp,
            Err(e) => {
                error!("Failed to send tool execution request to daemon: {}", e);
                return Err(anyhow::anyhow!("Communication error with MCP daemon: {}", e));
            }
        };
        
        match response.status {
            ResponseStatus::Success => {
                // Extract the execution result from the response
                match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Result(gemini_mcp::ipc::DaemonResult::ExecutionOutput(output)) => {
                        debug!("Tool execution successful: {}.{}", server, tool);
                        Ok(output)
                    },
                    gemini_mcp::ipc::ResponsePayload::Error(err) => {
                        warn!("Daemon returned error for tool execution: {}", err.message);
                        Err(anyhow::anyhow!("Daemon returned error for tool execution: {}", err.message))
                    },
                    _ => {
                        error!("Unexpected response payload format for tool execution");
                        Err(anyhow::anyhow!("Unexpected response payload format for tool execution"))
                    }
                }
            }
            ResponseStatus::Error => {
                // Extract the error message and return it
                let error_msg = match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Error(error) => error.message,
                    _ => "Unknown error occurred during tool execution".to_string(),
                };
                error!("Tool execution error for {}.{}: {}", server, tool, error_msg);
                Err(anyhow::anyhow!("Tool execution error: {}", error_msg))
            }
        }
    }
    
    /// Generates an embedding for the provided text.
    pub async fn generate_embedding(&self, text: &str, model_variant: EmbeddingModelVariant) -> Result<Vec<f32>> {
        let request = DaemonRequest::GenerateEmbedding {
            text: text.to_string(),
            model_variant: model_variant.as_str().to_string(),
        };
        
        let response = self.send_request(request).await?;
        
        match response.status {
            ResponseStatus::Success => {
                // Extract the embedding vector from the response
                match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Result(gemini_mcp::ipc::DaemonResult::Embedding(embedding)) => Ok(embedding),
                    gemini_mcp::ipc::ResponsePayload::Error(err) => Err(anyhow::anyhow!("Daemon returned error for embedding generation: {}", err.message)),
                    _ => Err(anyhow::anyhow!("Unexpected response payload format for embedding generation")),
                }
            }
            ResponseStatus::Error => {
                // Extract the error message and return it
                let error_msg = match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Error(error) => error.message,
                    _ => "Unknown error occurred during embedding generation".to_string(),
                };
                Err(anyhow::anyhow!("Embedding generation error: {}", error_msg))
            }
        }
    }
    
    /// Gets the broker capabilities for MemoryStore.
    pub async fn get_broker_capabilities(&self) -> Result<Capabilities> {
        let request = DaemonRequest::GetBrokerCapabilities;
        let response = self.send_request(request).await?;
        
        match response.status {
            ResponseStatus::Success => {
                // Extract the broker capabilities from the response
                match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Result(gemini_mcp::ipc::DaemonResult::BrokerCapabilities(broker_caps)) => {
                        // Convert from the daemon's BrokerCapabilities to gemini_memory's Capabilities
                        let tools = broker_caps.tools.iter().map(|tool| {
                            ToolDefinition {
                                name: tool.name.clone(),
                            }
                        }).collect();
                        
                        Ok(Capabilities { tools })
                    },
                    gemini_mcp::ipc::ResponsePayload::Error(err) => Err(anyhow::anyhow!("Daemon returned error for broker capabilities: {}", err.message)),
                    _ => Err(anyhow::anyhow!("Unexpected response payload format for broker capabilities")),
                }
            }
            ResponseStatus::Error => {
                // Extract the error message and return it
                let error_msg = match response.payload {
                    gemini_mcp::ipc::ResponsePayload::Error(error) => error.message,
                    _ => "Unknown error occurred while getting broker capabilities".to_string(),
                };
                Err(anyhow::anyhow!("Daemon error: {}", error_msg))
            }
        }
    }

    /// Sends a request to the daemon and returns the response.
    async fn send_request(&self, request: DaemonRequest) -> Result<DaemonResponse> {
        // Connect to the Unix socket
        let mut stream = UnixStream::connect(&self.socket_path).await
            .map_err(|e| anyhow::anyhow!("Failed to connect to MCP daemon at {}: {}", 
                self.socket_path.display(), e))?;
        
        // Serialize the request to JSON
        let request_json = serde_json::to_vec(&request)
            .map_err(|e| anyhow::anyhow!("Failed to serialize request: {}", e))?;
        
        // Send the length prefix
        stream.write_u32(request_json.len() as u32).await
            .map_err(|e| anyhow::anyhow!("Failed to write message length: {}", e))?;
        
        // Send the request payload
        stream.write_all(&request_json).await
            .map_err(|e| anyhow::anyhow!("Failed to write message payload: {}", e))?;
        
        // Read the response length
        let response_len = stream.read_u32().await
            .map_err(|e| anyhow::anyhow!("Failed to read response length: {}", e))?;
        
        // Read the response payload
        let mut buffer = vec![0u8; response_len as usize];
        stream.read_exact(&mut buffer).await
            .map_err(|e| anyhow::anyhow!("Failed to read response payload: {}", e))?;
        
        // Deserialize the response
        let response: DaemonResponse = serde_json::from_slice(&buffer)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize response: {}", e))?;
        
        Ok(response)
    }
}

// Implement McpHostInterface for McpDaemonClient
#[async_trait]
impl McpHostInterface for McpDaemonClient {
    /// Execute a tool on an MCP server through the daemon
    async fn execute_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        params: Value,
    ) -> Result<Value, Box<dyn Error>> {
        debug!("McpHostInterface::execute_tool called for {}/{}", server_name, tool_name);
        match self.execute_tool(server_name, tool_name, params).await {
            Ok(value) => Ok(value),
            Err(e) => {
                error!("McpHostInterface::execute_tool failed: {}", e);
                Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn Error>)
            }
        }
    }

    /// Get all capabilities from the daemon
    async fn get_all_capabilities(&self) -> Capabilities {
        debug!("McpHostInterface::get_all_capabilities called");
        match self.get_broker_capabilities().await {
            Ok(caps) => {
                debug!("Retrieved broker capabilities with {} tools", caps.tools.len());
                caps
            }
            Err(e) => {
                warn!("Failed to get broker capabilities: {}", e);
                Capabilities { tools: Vec::new() }
            }
        }
    }

    /// Send a generic request through the daemon
    async fn send_request(&self, request: Request) -> Result<Response, JsonRpcError> {
        warn!("send_request not fully implemented for McpDaemonClient");
        // For now, we don't support raw JSON-RPC requests through the daemon
        // If needed in the future, we could add a PassThroughRequest variant to DaemonRequest
        Err(JsonRpcError {
            code: -32601,
            message: "Method not available via IPC daemon".to_string(),
            data: None,
        })
    }

    /// Get capabilities from daemon (converts to ServerCapabilities)
    async fn get_capabilities(&self) -> Result<ServerCapabilities, String> {
        debug!("McpHostInterface::get_capabilities called");
        self.get_all_capabilities()
            .await
            .map_err(|e| {
                error!("Failed to get capabilities: {}", e);
                e.to_string()
            })
    }
}

// This extension trait adds embedding-specific functionality
// that works with the McpHostInterface trait but isn't part of it
pub trait McpEmbeddingExtension {
    /// Generate an embedding vector for text
    async fn generate_embedding(&self, text: &str, model_variant: EmbeddingModelVariant) -> Result<Vec<f32>>;
}

// Implement the extension trait for McpDaemonClient
impl McpEmbeddingExtension for McpDaemonClient {
    async fn generate_embedding(&self, text: &str, model_variant: EmbeddingModelVariant) -> Result<Vec<f32>> {
        self.generate_embedding(text, model_variant).await
    }
} 