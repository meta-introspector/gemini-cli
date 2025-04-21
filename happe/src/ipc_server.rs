use crate::config::AppConfig;
use crate::coordinator;
use crate::mcp_client::McpHostClient;
use anyhow::Result;
use gemini_core::client::GeminiClient;
use gemini_ipc::happe_request::{HappeQueryRequest, HappeQueryResponse};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, warn};

/// Shared state for the IPC server
pub struct IpcServerState {
    config: AppConfig,
    gemini_client: Arc<GeminiClient>,
    mcp_client: Arc<McpHostClient>,
}

/// Run the IPC server
pub async fn run_server(
    socket_path: impl AsRef<Path>,
    config: AppConfig,
    gemini_client: GeminiClient,
    mcp_client: McpHostClient,
) -> Result<()> {
    let socket_path = socket_path.as_ref();

    // Clean up existing socket if it exists
    if socket_path.exists() {
        tokio::fs::remove_file(socket_path).await?;
    }

    // Create the Unix socket listener
    let listener = UnixListener::bind(socket_path)?;
    info!("Started IPC server on {}", socket_path.display());

    // Create shared state
    let state = Arc::new(IpcServerState {
        config,
        gemini_client: Arc::new(gemini_client),
        mcp_client: Arc::new(mcp_client),
    });

    // Accept and handle connections
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                debug!("Accepted new IPC connection");
                let state_clone = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state_clone).await {
                        error!(error = %e, "Error handling IPC connection");
                    }
                });
            }
            Err(e) => {
                warn!(error = %e, "Failed to accept IPC connection");
            }
        }
    }
}

/// Handle a single connection
async fn handle_connection(
    mut stream: UnixStream,
    state: Arc<IpcServerState>,
) -> Result<()> {
    // Read request size
    let mut size_buf = [0u8; 4];
    stream.read_exact(&mut size_buf).await?;
    let msg_size = u32::from_le_bytes(size_buf) as usize;

    // Read request data
    let mut msg_buf = vec![0u8; msg_size];
    stream.read_exact(&mut msg_buf).await?;

    // Parse request
    let request: HappeQueryRequest = serde_json::from_slice(&msg_buf)?;
    debug!(query = %request.query, "Received IPC query request");

    // Process the query
    let response = match coordinator::process_query(
        &state.config,
        &state.mcp_client,
        &state.gemini_client,
        request.query,
    )
    .await
    {
        Ok(response_text) => HappeQueryResponse {
            response: response_text,
            error: None,
        },
        Err(e) => {
            error!(error = %e, "Failed to process query");
            HappeQueryResponse {
                response: String::new(),
                error: Some(format!("Failed to process query: {}", e)),
            }
        }
    };

    // Serialize and send response
    let response_data = serde_json::to_vec(&response)?;
    let response_size = response_data.len() as u32;
    
    // Write response size
    stream.write_all(&response_size.to_le_bytes()).await?;
    
    // Write response data
    stream.write_all(&response_data).await?;
    
    debug!("Sent IPC response");
    Ok(())
} 