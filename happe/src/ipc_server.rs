use crate::config::AppConfig;
use crate::coordinator;
use crate::mcp_client::McpHostClient;
use crate::session::{InMemorySessionStore, Session, SessionStore, SessionStoreRef};
use anyhow::Result;
use gemini_core::client::GeminiClient;
use gemini_ipc::happe_request::{HappeQueryRequest, HappeQueryResponse};
use gemini_ipc::internal_messages::ConversationTurn;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::{Duration, Utc};

/// Shared state for the IPC server
pub struct IpcServerState {
    config: AppConfig,
    gemini_client: Arc<GeminiClient>,
    mcp_client: Arc<McpHostClient>,
    session_store: SessionStoreRef,
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

    // Create session store
    let session_store = Arc::new(InMemorySessionStore::new()) as SessionStoreRef;

    // Create shared state
    let state = Arc::new(IpcServerState {
        config,
        gemini_client: Arc::new(gemini_client),
        mcp_client: Arc::new(mcp_client),
        session_store,
    });

    // Start a periodic task to clean up expired sessions
    let store_clone = Arc::clone(&state.session_store);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600)); // Every hour
        loop {
            interval.tick().await;
            match store_clone.cleanup_expired_sessions().await {
                Ok(count) => {
                    if count > 0 {
                        info!("Cleaned up {} expired sessions", count);
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to clean up expired sessions");
                }
            }
        }
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
async fn handle_connection(mut stream: UnixStream, state: Arc<IpcServerState>) -> Result<()> {
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

    // Get or create a session
    let session_id = request.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    let mut session = get_or_create_session(&state.session_store, &session_id).await?;
    
    // Set session expiry to 24 hours from now
    session.set_expiry(Utc::now() + Duration::hours(24));

    // Process the query
    let response = match coordinator::process_query(
        &state.config,
        &state.mcp_client,
        &state.gemini_client,
        &session,
        request.query.clone(),
    )
    .await
    {
        Ok(response_text) => {
            // Create turn data and update session history
            let turn = ConversationTurn {
                user_query: request.query,
                llm_response: response_text.clone(),
                retrieved_memories: vec![],
            };
            
            coordinator::update_session_history(&mut session, turn);
            
            // Save the session
            if let Err(e) = state.session_store.save_session(session.clone()).await {
                error!(error = %e, "Failed to save session");
                // Continue despite error
            }
            
            HappeQueryResponse {
                response: response_text,
                session_id: Some(session_id),
                error: None,
            }
        },
        Err(e) => {
            error!(error = %e, "Failed to process query");
            
            // Save the session anyway to preserve any state changes
            if let Err(save_err) = state.session_store.save_session(session.clone()).await {
                error!(error = %save_err, "Failed to save session after query error");
            }
            
            HappeQueryResponse {
                response: String::new(),
                session_id: Some(session_id),
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

/// Get an existing session or create a new one
async fn get_or_create_session(
    session_store: &SessionStoreRef,
    session_id: &str,
) -> Result<Session> {
    // Try to get existing session
    match session_store.get_session(session_id).await {
        Ok(session) => {
            debug!(session_id = %session_id, "Using existing session");
            Ok(session)
        },
        Err(_) => {
            // Create a new session
            debug!(session_id = %session_id, "Creating new session");
            match session_store.create_session(session_id.to_string()).await {
                Ok(session) => Ok(session),
                Err(e) => Err(anyhow::anyhow!("Failed to create session: {}", e)),
            }
        }
    }
}
