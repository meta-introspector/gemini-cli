use serde::{Deserialize, Serialize};
use serde_json::Value;
// Use the existing ServerCapabilities from core, assuming it's been moved to core::rpc_types
// If not, adjust the path accordingly.
use gemini_core::rpc_types::ServerCapabilities;

/// Represents a request sent from the CLI client to the MCP host daemon.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRequest {
    /// Request to get all capabilities from all connected MCP servers.
    GetCapabilities,
    /// Request to execute a specific tool on a specific server.
    ExecuteTool {
        server: String,
        tool: String,
        args: Value,
    },
    /// Request to generate an embedding for text using the embedding server.
    GenerateEmbedding { text: String, model_variant: String },
    /// Request to get broker capabilities for MemoryStore
    GetBrokerCapabilities,
    // Future requests can be added here, e.g., ShutdownDaemon, ListServers, etc.
}

/// Represents a response sent from the MCP host daemon back to the CLI client.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DaemonResponse {
    pub status: ResponseStatus,
    #[serde(flatten)] // Embed result or error directly based on status
    pub payload: ResponsePayload,
}

/// Indicates whether the daemon operation succeeded or failed.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Success,
    Error,
}

/// Contains either the successful result or the error details.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)] // Allows payload to be structurally different for Result vs Error
pub enum ResponsePayload {
    Result(DaemonResult),
    Error(DaemonError),
}

/// Represents the data returned upon successful execution of a daemon request.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)] // Allows the success result to be one of several types
pub enum DaemonResult {
    /// Contains the aggregated capabilities from all servers.
    Capabilities(ServerCapabilities),
    /// Contains the output value from a successful tool execution.
    ExecutionOutput(Value),
    /// Contains an embedding vector from a successful embedding generation.
    Embedding(Vec<f32>),
    /// Contains broker capabilities for MemoryStore.
    BrokerCapabilities(BrokerCapabilities),
    // Future success results can be added here
}

/// Contains details about an error that occurred during daemon request processing.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DaemonError {
    pub message: String,
    // Consider adding an optional error code or more details later
}

/// Simplified capabilities structure for the memory broker
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BrokerCapabilities {
    /// Available MCP tools
    pub tools: Vec<ToolDefinition>,
}

/// Simplified tool definition for the memory broker
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolDefinition {
    /// Tool name in format "server_name/tool_name"
    pub name: String,
}

// Helper constructors for creating responses easily.
impl DaemonResponse {
    /// Creates a success response with the given result payload.
    pub fn success(result: DaemonResult) -> Self {
        Self {
            status: ResponseStatus::Success,
            payload: ResponsePayload::Result(result),
        }
    }

    /// Creates an error response with the given error message.
    pub fn error(message: String) -> Self {
        Self {
            status: ResponseStatus::Error,
            payload: ResponsePayload::Error(DaemonError { message }),
        }
    }
}
