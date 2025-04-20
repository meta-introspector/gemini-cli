use env_logger;
use gemini_ipc::daemon_messages::{
    BrokerCapabilities, DaemonRequest, DaemonResponse, DaemonResult, ToolDefinition,
};
use gemini_mcp::{load_mcp_servers, McpHost};
use log::{debug, error, info, warn};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

// Helper function to determine the socket path
fn get_socket_path() -> Result<PathBuf, String> {
    let base_dir = dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .ok_or_else(|| "Could not determine runtime or data local directory".to_string())?;
    let socket_dir = base_dir.join("gemini-cli");
    fs::create_dir_all(&socket_dir)
        .map_err(|e| format!("Failed to create socket directory: {}", e))?;
    Ok(socket_dir.join("mcp-hostd.sock"))
}

// Extension trait to add embedding and broker capabilities functions
trait McpHostExtensions {
    async fn generate_embedding(&self, text: &str, model_variant: &str)
        -> Result<Vec<f32>, String>;
    async fn get_broker_capabilities(&self) -> Result<BrokerCapabilities, String>;
}

// Implement extensions for McpHost
impl McpHostExtensions for Arc<McpHost> {
    async fn generate_embedding(
        &self,
        text: &str,
        model_variant: &str,
    ) -> Result<Vec<f32>, String> {
        // Forward to embedding server using execute_tool
        let params = json!({
            "text": text,
            "is_query": false,
            "variant": model_variant
        });

        match self.execute_tool("embedding", "embed", params).await {
            Ok(result) => {
                // Extract the embedding vector from the result
                let embedding = result
                    .get("embedding")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| "Missing or invalid embedding in response".to_string())?;

                // Convert to Vec<f32>
                let embedding_vec: Vec<f32> = embedding
                    .iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();

                Ok(embedding_vec)
            }
            Err(e) => Err(format!("Failed to generate embedding: {}", e)),
        }
    }

    async fn get_broker_capabilities(&self) -> Result<BrokerCapabilities, String> {
        // Get capabilities and filter for the ones needed by the broker
        let all_caps = self.get_all_capabilities().await;

        // Convert to broker capabilities format
        let tools = all_caps
            .tools
            .iter()
            .map(|tool| ToolDefinition {
                name: tool.name.clone(),
            })
            .collect();

        Ok(BrokerCapabilities { tools })
    }
}

#[tokio::main]
async fn main() {
    // Initialize logging
    env_logger::init();
    info!("Starting MCP Host Daemon...");

    // Determine and prepare socket path
    let socket_path = match get_socket_path() {
        Ok(path) => path,
        Err(e) => {
            error!("Failed to determine socket path: {}", e);
            std::process::exit(1);
        }
    };

    // Clean up existing socket file if it exists
    if socket_path.exists() {
        info!("Removing existing socket file: {}", socket_path.display());
        if let Err(e) = fs::remove_file(&socket_path) {
            error!("Failed to remove existing socket file: {}", e);
            std::process::exit(1);
        }
    }

    // Load server configurations
    let configs = match load_mcp_servers() {
        Ok(configs) => {
            info!("Loaded {} MCP server configurations", configs.len());
            configs
        }
        Err(e) => {
            error!("Failed to load MCP server configurations: {}", e);
            // Clean up the socket dir we might have created
            let _ = fs::remove_file(&socket_path);
            std::process::exit(1);
        }
    };

    // Create MCP Host
    let mcp_host = match McpHost::new(configs).await {
        Ok(host) => {
            info!("MCP Host initialized successfully");
            Arc::new(host)
        }
        Err(e) => {
            error!("Failed to initialize MCP Host: {}", e);
            // Clean up the socket dir we might have created
            let _ = fs::remove_file(&socket_path);
            std::process::exit(1);
        }
    };

    // Bind the Unix listener
    let listener = match UnixListener::bind(&socket_path) {
        Ok(listener) => {
            info!("IPC Listener bound to {}", socket_path.display());
            listener
        }
        Err(e) => {
            error!("Failed to bind IPC listener: {}", e);
            // Clean up the socket dir we might have created
            let _ = fs::remove_file(&socket_path);
            std::process::exit(1);
        }
    };

    // Set up shutdown channel
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

    // Set up signal handlers for graceful shutdown
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(signal) => signal,
        Err(e) => {
            error!("Failed to set up SIGINT handler: {}", e);
            let _ = fs::remove_file(&socket_path);
            std::process::exit(1);
        }
    };

    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(signal) => signal,
        Err(e) => {
            error!("Failed to set up SIGTERM handler: {}", e);
            let _ = fs::remove_file(&socket_path);
            std::process::exit(1);
        }
    };

    let shutdown_tx_int = shutdown_tx.clone();
    let shutdown_tx_term = shutdown_tx;

    // Handle SIGINT
    tokio::spawn(async move {
        sigint.recv().await;
        info!("Received SIGINT, initiating graceful shutdown...");
        let _ = shutdown_tx_int.send(()).await;
    });

    // Handle SIGTERM
    tokio::spawn(async move {
        sigterm.recv().await;
        info!("Received SIGTERM, initiating graceful shutdown...");
        let _ = shutdown_tx_term.send(()).await;
    });

    info!("MCP Host Daemon running. Accepting IPC connections...");

    // Main loop: Accept connections and listen for shutdown signal
    loop {
        tokio::select! {
            // Accept new IPC connection
            Ok((stream, _addr)) = listener.accept() => {
                info!("Accepted new IPC connection");
                let host_clone = Arc::clone(&mcp_host); // Clone Arc for the task

                // Spawn a task to handle this client connection
                tokio::spawn(async move {
                    handle_client(stream, host_clone).await;
                });
            }
            // Wait for shutdown signal
            _ = shutdown_rx.recv() => {
                info!("Shutdown signal received, stopping IPC listener.");
                break; // Exit the loop
            }
            // Handle listener errors (optional, depends on desired robustness)
            else => {
                 warn!("IPC listener error or closed unexpectedly.");
                 break; // Exit the loop
            }
        }
    }

    info!("Shutting down MCP Host...");

    // Shutdown the MCP Host
    mcp_host.shutdown().await;
    info!("MCP Host shutdown complete");

    // Clean up the socket file
    info!("Removing socket file: {}", socket_path.display());
    if let Err(e) = fs::remove_file(&socket_path) {
        error!("Failed to remove socket file during shutdown: {}", e);
        // Don't exit here, shutdown should proceed
    }

    // Give tasks a moment to complete their cleanup
    sleep(Duration::from_millis(100)).await;

    info!("MCP Host Daemon terminated");
}

/// Handles communication with a single connected client.
async fn handle_client(mut stream: UnixStream, host: Arc<McpHost>) {
    info!("Client handler started for connection.");
    let (mut reader, mut writer) = stream.split();

    loop {
        // 1. Read message length (u32)
        let msg_len = match reader.read_u32().await {
            Ok(len) => len,
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                info!("Client disconnected gracefully.");
                break;
            }
            Err(e) => {
                warn!("Failed to read message length from client: {}", e);
                break;
            }
        };

        if msg_len == 0 {
            warn!("Received message with length 0. Closing connection.");
            break;
        }

        debug!("Received message header with length: {} bytes", msg_len);

        // 2. Read the message payload
        let mut buffer = vec![0u8; msg_len as usize];
        if let Err(e) = reader.read_exact(&mut buffer).await {
            warn!("Failed to read message payload from client: {}", e);
            break;
        }

        // 3. Deserialize request
        let request: DaemonRequest = match serde_json::from_slice(&buffer) {
            Ok(req) => req,
            Err(e) => {
                warn!("Failed to deserialize request from client: {}", e);
                // Send an error response back
                let response = DaemonResponse::error(format!("Invalid request format: {}", e));
                if write_response(&mut writer, &response).await.is_err() {
                    warn!("Failed to send deserialization error response to client.");
                }
                continue; // Or break, depending on desired behavior
            }
        };

        debug!("Received request: {:?}", request);

        // 4. Process request and generate response
        let response = process_request(request, Arc::clone(&host)).await;

        // 5. Send response
        match write_response(&mut writer, &response).await {
            Ok(_) => debug!("Successfully sent response: {:?}", response),
            Err(e) => {
                warn!("Failed to send response to client: {}", e);
                break;
            }
        }
    }

    info!("Client handler finished for connection.");
}

/// Processes a deserialized DaemonRequest and returns a DaemonResponse.
async fn process_request(request: DaemonRequest, host: Arc<McpHost>) -> DaemonResponse {
    match request {
        DaemonRequest::GetCapabilities => {
            info!("Processing GetCapabilities request");
            match host.get_all_capabilities().await {
                caps => {
                    info!(
                        "Retrieved capabilities for {} tools and {} resources",
                        caps.tools.len(),
                        caps.resources.len()
                    );
                    DaemonResponse::success(DaemonResult::Capabilities(caps))
                }
            }
        }
        DaemonRequest::ExecuteTool { server, tool, args } => {
            info!(
                "Executing tool '{}' on server '{}' with args: {}",
                tool,
                server,
                serde_json::to_string(&args).unwrap_or_else(|_| "unable to serialize".to_string())
            );

            match host.execute_tool(&server, &tool, args).await {
                Ok(result_value) => {
                    debug!(
                        "Tool execution succeeded with result: {}",
                        serde_json::to_string(&result_value)
                            .unwrap_or_else(|_| "unable to serialize".to_string())
                    );
                    DaemonResponse::success(DaemonResult::ExecutionOutput(result_value))
                }
                Err(e) => {
                    error!(
                        "Tool execution failed: {} on server {} - Error: {}",
                        tool, server, e
                    );
                    DaemonResponse::error(format!("Tool execution error: {}", e))
                }
            }
        }
        DaemonRequest::GenerateEmbedding {
            text,
            model_variant,
        } => {
            info!("Generating embedding with model variant: {}", model_variant);
            // Forward to memory broker if available, otherwise return error
            match host.generate_embedding(&text, &model_variant).await {
                Ok(embedding) => {
                    debug!("Generated embedding with {} dimensions", embedding.len());
                    DaemonResponse::success(DaemonResult::Embedding(embedding))
                }
                Err(e) => {
                    error!("Embedding generation failed: {}", e);
                    DaemonResponse::error(format!("Error generating embedding: {}", e))
                }
            }
        }
        DaemonRequest::GetBrokerCapabilities => {
            info!("Getting broker capabilities");
            // Check if memory broker is available and get its capabilities
            match host.get_broker_capabilities().await {
                Ok(caps) => {
                    info!(
                        "Retrieved broker capabilities with {} tools",
                        caps.tools.len()
                    );
                    DaemonResponse::success(DaemonResult::BrokerCapabilities(caps))
                }
                Err(e) => {
                    error!("Failed to get broker capabilities: {}", e);
                    DaemonResponse::error(format!("Error getting broker capabilities: {}", e))
                }
            }
        } // Handle other request types here in the future
    }
}

/// Serializes and writes a DaemonResponse to the client stream with a length prefix.
async fn write_response<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    response: &DaemonResponse,
) -> Result<(), std::io::Error> {
    let response_bytes = match serde_json::to_vec(response) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to serialize response: {}", e);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize response: {}", e),
            ));
        }
    };

    let response_len = response_bytes.len() as u32;
    debug!("Sending response with length: {} bytes", response_len);

    // Write the length prefix first
    if let Err(e) = writer.write_u32(response_len).await {
        error!("Failed to write response length: {}", e);
        return Err(e);
    }

    // Then write the response data
    if let Err(e) = writer.write_all(&response_bytes).await {
        error!("Failed to write response data: {}", e);
        return Err(e);
    }

    // Flush to ensure all data is sent
    if let Err(e) = writer.flush().await {
        error!("Failed to flush response: {}", e);
        return Err(e);
    }

    debug!("Response successfully written and flushed");
    Ok(())
}
