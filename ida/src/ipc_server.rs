use crate::{memory_mcp_client, storage};
use gemini_ipc::internal_messages::InternalMessage;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, instrument, warn};

// TODO: Define a proper config struct
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub ipc_path: String,
    // Add other config fields like MCP server address later
}

// TODO: Define a proper error type
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("MCP client error: {0}")]
    McpClient(#[from] crate::memory_mcp_client::McpClientError),
}

#[instrument(skip(config))]
pub async fn run_server(config: DaemonConfig) -> Result<(), ServerError> {
    let ipc_path = Path::new(&config.ipc_path);

    // Clean up existing socket file if it exists
    if ipc_path.exists() {
        warn!("IPC socket file already exists, removing: {:?}", ipc_path);
        tokio::fs::remove_file(ipc_path).await?;
    }

    let listener = UnixListener::bind(ipc_path)?;
    info!("IDA Daemon listening on IPC path: {:?}", ipc_path);

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                info!("Accepted new IPC connection");
                // Spawn a task to handle this connection independently
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream).await {
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

#[instrument(skip(stream), name = "ipc_connection_handler")]
async fn handle_connection(mut stream: UnixStream) -> Result<(), ServerError> {
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
            InternalMessage::GetMemoriesRequest { query } => {
                info!("Processing GetMemoriesRequest for query: {}", query);
                let memories = memory_mcp_client::retrieve_memories(&query).await?;
                let response = InternalMessage::GetMemoriesResponse { memories };
                send_message(&mut stream, &response).await?;
            }
            InternalMessage::StoreTurnRequest { turn_data } => {
                info!("Processing StoreTurnRequest, spawning background task.");
                // Spawn a background task for storage and continue handling connection
                tokio::spawn(storage::handle_storage(turn_data));
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
