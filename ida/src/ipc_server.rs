use crate::llm_clients::LLMClient;
use crate::{memory_mcp_client, storage};
use gemini_core::config::IdaConfig;
use gemini_ipc::internal_messages::InternalMessage;
use gemini_memory::broker::McpHostInterface;
use gemini_memory::MemoryStore;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, instrument, warn};
use anyhow::anyhow;

/// Configuration for the IDA daemon
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub ipc_path: String,
    /// Path to the memory database directory
    pub memory_db_path: Option<PathBuf>,
    /// Maximum number of memory results to return per query
    pub max_memory_results: usize,
}

/// Error type for server operations
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Memory error: {0}")]
    Memory(#[from] crate::memory_mcp_client::MemoryError),
    #[error("Anyhow error: {0}")]
    Anyhow(#[from] anyhow::Error),
}

/// Holds shared state for the IDA daemon and connection handlers
#[derive(Clone)]
struct ServerState {
    config: Arc<IdaConfig>,
    memory_store: Arc<MemoryStore>,
    mcp_host: Option<Arc<dyn McpHostInterface + Send + Sync>>,
    llm_client: Option<Arc<dyn LLMClient + Send + Sync>>,
}

// Manual Debug implementation because LLMClient and McpHostInterface might not impl Debug
impl std::fmt::Debug for ServerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerState")
            .field("config", &self.config)
            .field("memory_store", &self.memory_store)
            .field("mcp_host_present", &self.mcp_host.is_some())
            .field("llm_client_present", &self.llm_client.is_some())
            .finish()
    }
}

#[instrument(skip(memory_store, mcp_host, llm_client))]
pub async fn run_server(
    config: IdaConfig,
    memory_store: Arc<MemoryStore>,
    mcp_host: Option<Arc<dyn McpHostInterface + Send + Sync>>,
    llm_client: Option<Arc<dyn LLMClient + Send + Sync>>,
) -> Result<(), ServerError> {
    let ipc_path_str = config
        .ida_socket_path
        .clone()
        .ok_or_else(|| ServerError::Anyhow(anyhow!("IDA socket path not configured")))?;
    let ipc_path = Path::new(&ipc_path_str);

    // Clean up existing socket file if it exists
    if ipc_path.exists() {
        warn!("IPC socket file already exists, removing: {:?}", ipc_path);
        tokio::fs::remove_file(ipc_path).await?;
    }

    let listener = UnixListener::bind(ipc_path)?;
    info!("IDA Daemon listening on IPC path: {:?}", ipc_path);

    // Create a cloneable state containing all necessary components
    let server_state = Arc::new(ServerState {
        config: Arc::new(config),
        memory_store,
        mcp_host,
        llm_client,
    });

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                info!("Accepted new IPC connection");
                // Clone server state for the new connection handler
                let connection_state = server_state.clone();
                // Spawn a task to handle this connection independently
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, connection_state).await {
                        error!("Error handling connection: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept IPC connection: {}", e);
                // Depending on the error, might need more sophisticated handling
                // For now, just log and continue
            }
        }
    }
    // Note: The loop runs indefinitely unless an unrecoverable error occurs in accept()
    // or the process is terminated externally.
    // Ok(()) // Unreachable in the current loop structure
}

#[instrument(skip(stream, state), name = "ipc_connection_handler")]
async fn handle_connection(
    mut stream: UnixStream,
    state: Arc<ServerState>,
) -> Result<(), ServerError> {
    let mut buffer = Vec::with_capacity(4096); // Start with 4KB, might need adjustment

    loop {
        // Read data from the stream
        buffer.clear(); // Clear buffer for new message
                        // Simple length-prefixing: Read u32 length first
        let length = match stream.read_u32().await {
            Ok(len) => len,
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                info!("Connection closed by peer.");
                break; // Clean exit
            }
            Err(e) => {
                error!("Failed to read message length: {}", e);
                return Err(ServerError::Io(e));
            }
        };

        if length == 0 {
            warn!("Received zero-length message, ignoring.");
            continue;
        }

        // Read the actual message bytes
        buffer.resize(length as usize, 0);
        if let Err(e) = stream.read_exact(&mut buffer).await {
            error!("Failed to read message body: {}", e);
            return Err(ServerError::Io(e));
        }

        debug!("Received {} bytes from IPC stream.", buffer.len());

        // Deserialize the message
        let message: InternalMessage = match serde_json::from_slice(&buffer) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to deserialize IPC message: {}", e);
                // Optionally send an error back to the client?
                return Err(ServerError::Serialization(e));
            }
        };

        debug!("Deserialized message: {:?}", message);

        // Process the message based on its type
        match message {
            InternalMessage::GetMemoriesRequest {
                query,
                conversation_context,
            } => {
                info!("Processing GetMemoriesRequest for query: {}", query);

                // Use max_memory_results from config, providing a default
                let max_results = state.config.max_memory_results.unwrap_or(5);

                // Use the real memory retrieval function
                let memories = memory_mcp_client::retrieve_memories(
                    &query,
                    state.memory_store.clone(),
                    max_results, // Use the resolved value
                    &state.llm_client,
                    conversation_context,
                )
                .await?;

                let response = InternalMessage::GetMemoriesResponse { memories };
                send_message(&mut stream, &response).await?;
            }
            InternalMessage::StoreTurnRequest { turn_data } => {
                info!("Processing StoreTurnRequest, spawning background task.");

                // Clone the memory store for the background task
                let memory_store_clone = state.memory_store.clone();

                // Spawn a background task for storage and continue handling connection
                tokio::spawn(async move {
                    if let Err(e) = storage::handle_storage(turn_data, memory_store_clone).await {
                        error!("Error in background storage task: {}", e);
                    }
                });

                // No response is sent for this message type
            }
            // Handle other message types if added later
            _ => {
                warn!("Received unhandled message type");
                // Optionally send an error or ignore
            }
        }
    }

    Ok(())
}

// Helper function to send a message with length prefix
async fn send_message(
    stream: &mut UnixStream,
    message: &InternalMessage,
) -> Result<(), ServerError> {
    let serialized = serde_json::to_vec(message)?;
    let len = serialized.len() as u32;

    debug!("Sending message length: {}", len);
    stream.write_u32(len).await?;
    debug!("Sending message body ({} bytes)", serialized.len());
    stream.write_all(&serialized).await?;
    stream.flush().await?; // Ensure data is sent
    info!("Message sent successfully.");
    Ok(())
}

// TODO: Add thiserror to Cargo.toml if not already present
