use anyhow::{anyhow, Context, Result};
use gemini_core::config::UnifiedConfig;
use gemini_ipc::happe_request::{HappeQueryRequest, HappeQueryResponse};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tracing::{debug, error, info, instrument};
use uuid::Uuid;

// Function to get the default socket path from unified config
fn get_default_happe_socket_path() -> Result<PathBuf> {
    // Load the unified configuration
    let config = UnifiedConfig::load();

    // Check if HAPPE socket path is configured
    if let Some(path) = config.happe.happe_socket_path {
        return Ok(path);
    }

    // Fall back to environment variable (transitional support)
    if let Ok(config_dir) = std::env::var("GEMINI_CONFIG_DIR") {
        // Use runtime directory from unified config
        let config_path = PathBuf::from(config_dir);
        if config_path.exists() {
            // Try to read the HAPPE config to get the socket path
            let happe_config_path = config_path.join("happe/config.toml");
            if happe_config_path.exists() {
                if let Ok(content) = std::fs::read_to_string(&happe_config_path) {
                    // Very basic TOML parsing to find the socket path
                    for line in content.lines() {
                        if line.starts_with("happe_socket_path") {
                            let parts: Vec<&str> = line.split('=').collect();
                            if parts.len() >= 2 {
                                let socket_path =
                                    parts[1].trim().trim_matches('"').trim_matches('\'');
                                return Ok(PathBuf::from(socket_path));
                            }
                        }
                    }
                }
            }
        }
    }

    // Fall back to default path
    let socket_file = "gemini_suite_happe.sock";

    // Try in runtime dir first
    if let Some(runtime_dir) = dirs::runtime_dir() {
        let socket_path = runtime_dir.join(socket_file);
        if socket_path.exists() {
            return Ok(socket_path);
        }
    }

    // Then try /tmp
    let tmp_path = PathBuf::from("/tmp").join(socket_file);
    if tmp_path.exists() {
        return Ok(tmp_path);
    }

    // Finally fall back to default path in cache dir
    let cache_dir =
        dirs::cache_dir().ok_or_else(|| anyhow!("Could not determine cache directory"))?;
    Ok(cache_dir.join(socket_file))
}

#[derive(Debug)]
pub struct HappeClient {
    socket_path: PathBuf,
    session_id: String,
}

impl HappeClient {
    /// Creates a new HappeClient.
    /// If socket_path is None, it tries to use the default path from unified config.
    pub fn new(socket_path: Option<PathBuf>) -> Result<Self> {
        let path = match socket_path {
            Some(p) => p,
            None => get_default_happe_socket_path()?,
        };
        info!("Using HAPPE IPC socket path: {}", path.display());
        
        // Check if we have a session ID in the environment (set by the shell wrapper)
        let session_id = match std::env::var("GEMINI_SESSION_ID") {
            Ok(id) => {
                info!("Using session ID from environment: {}", id);
                id
            },
            Err(_) => {
                // No environment variable, generate a new session ID
                let id = Uuid::new_v4().to_string();
                info!("No session ID in environment, created new session with ID: {}", id);
                id
            }
        };
        
        Ok(Self { 
            socket_path: path,
            session_id,
        })
    }

    /// Returns the current session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Establishes a connection to the HAPPE daemon's IPC socket.
    #[instrument(skip(self))]
    async fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| {
                format!(
                    "Failed to connect to HAPPE socket at {}",
                    self.socket_path.display()
                )
            })
    }

    /// Sends a query to the HAPPE daemon and receives the response.
    #[instrument(skip(self, query))]
    pub async fn send_query(&self, query: String) -> Result<HappeQueryResponse> {
        debug!("Connecting to HAPPE daemon...");
        let mut stream = self.connect().await?;
        debug!("Connected. Sending query: {}", query);

        let request = HappeQueryRequest { 
            query,
            session_id: Some(self.session_id.clone()),
        };
        
        let serialized_request =
            serde_json::to_vec(&request).context("Failed to serialize HappeQueryRequest")?;
        let len = serialized_request.len() as u32;

        // Send length prefix in little-endian format
        stream
            .write_all(&len.to_le_bytes())
            .await
            .context("Failed to write request length to HAPPE socket")?;
        // Send request body
        stream
            .write_all(&serialized_request)
            .await
            .context("Failed to write request body to HAPPE socket")?;
        stream
            .flush()
            .await
            .context("Failed to flush HAPPE socket after writing request")?;
        debug!("Query sent successfully ({} bytes)", len);

        // Read response length in little-endian format
        debug!("Waiting for response length...");
        let mut size_buf = [0u8; 4];
        stream
            .read_exact(&mut size_buf)
            .await
            .context("Failed to read response length from HAPPE socket")?;
        let response_len = u32::from_le_bytes(size_buf);
        debug!("Received response length: {}", response_len);

        if response_len == 0 {
            return Err(anyhow!("Received zero-length response from HAPPE"));
        }

        // Read response body
        let mut response_buffer = vec![0; response_len as usize];
        stream
            .read_exact(&mut response_buffer)
            .await
            .context("Failed to read response body from HAPPE socket")?;
        debug!("Received response body ({} bytes)", response_len);

        // Deserialize response
        let response: HappeQueryResponse = serde_json::from_slice(&response_buffer)
            .context("Failed to deserialize HappeQueryResponse")?;

        // Update session_id if it was changed by the server
        if let Some(new_session_id) = &response.session_id {
            if new_session_id != &self.session_id {
                debug!("Session ID updated from {} to {}", self.session_id, new_session_id);
            }
        }

        debug!("Deserialized response successfully.");
        Ok(response)
    }

    /// Sends a simple ping request to test the connection.
    pub async fn test_connection(&self) -> Result<bool> {
        info!("Testing connection to HAPPE daemon...");
        match self.send_query("__PING__".to_string()).await {
            Ok(resp) => {
                // HAPPE should ideally respond with a specific PONG or similar
                // For now, just check if the response is successful
                if resp.error.is_none() {
                    info!("HAPPE connection test successful.");
                    Ok(true)
                } else {
                    error!(
                        "HAPPE connection test failed: received error response: {:?}",
                        resp.error
                    );
                    Ok(false)
                }
            }
            Err(e) => {
                error!("HAPPE connection test failed: {}", e);
                Ok(false)
            }
        }
    }

    /// Lists all active sessions from the HAPPE daemon.
    pub async fn list_sessions(&self) -> Result<Vec<String>> {
        debug!("Connecting to HAPPE daemon to list sessions...");
        let mut stream = self.connect().await?;
        debug!("Connected. Sending list_sessions command...");

        let request = HappeQueryRequest { 
            query: "__LIST_SESSIONS__".to_string(),
            session_id: Some(self.session_id.clone()),
        };
        
        let serialized_request =
            serde_json::to_vec(&request).context("Failed to serialize HappeQueryRequest")?;
        let len = serialized_request.len() as u32;

        // Send length prefix in little-endian format
        stream
            .write_all(&len.to_le_bytes())
            .await
            .context("Failed to write request length to HAPPE socket")?;
        // Send request body
        stream
            .write_all(&serialized_request)
            .await
            .context("Failed to write request body to HAPPE socket")?;
        stream
            .flush()
            .await
            .context("Failed to flush HAPPE socket after writing request")?;
        debug!("List sessions command sent successfully ({} bytes)", len);

        // Read response length in little-endian format
        debug!("Waiting for response length...");
        let mut size_buf = [0u8; 4];
        stream
            .read_exact(&mut size_buf)
            .await
            .context("Failed to read response length from HAPPE socket")?;
        let response_len = u32::from_le_bytes(size_buf);
        debug!("Received response length: {}", response_len);

        if response_len == 0 {
            return Err(anyhow!("Received zero-length response from HAPPE"));
        }

        // Read response body
        let mut response_buffer = vec![0; response_len as usize];
        stream
            .read_exact(&mut response_buffer)
            .await
            .context("Failed to read response body from HAPPE socket")?;
        debug!("Received response body ({} bytes)", response_len);

        // Deserialize response
        let response: HappeQueryResponse = serde_json::from_slice(&response_buffer)
            .context("Failed to deserialize HappeQueryResponse")?;

        if let Some(error) = response.error {
            return Err(anyhow!("Error listing sessions: {}", error));
        }

        // Parse the response which should be a JSON array of session IDs
        let sessions = if !response.response.is_empty() {
            serde_json::from_str::<Vec<String>>(&response.response)
                .context("Failed to parse session list response")?
        } else {
            Vec::new()
        };

        Ok(sessions)
    }
}
