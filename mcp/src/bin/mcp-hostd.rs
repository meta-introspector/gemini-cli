use gemini_ipc::daemon_messages::{
    BrokerCapabilities, DaemonRequest, DaemonResponse, DaemonResult, ToolDefinition,
};
use gemini_mcp::{load_mcp_servers, McpHost};
use gemini_core::config::{self, UnifiedConfig};
use gemini_memory::schema::{EmbeddingModelVariant, self};
use gemini_memory::MemoryStore;
use gemini_memory::broker::McpHostInterface;
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use std::str::FromStr;
use gemini_core::rpc_types::{ServerCapabilities, Tool as CoreTool};

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
    async fn get_broker_capabilities(&self, internal_memory_store: &Option<Arc<MemoryStore>>) -> Result<BrokerCapabilities, String>;
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

    async fn get_broker_capabilities(&self, internal_memory_store: &Option<Arc<MemoryStore>>) -> Result<BrokerCapabilities, String> {
        // Get capabilities from actual MCP servers
        let mut all_caps = self.get_all_capabilities().await;

        // Add internally handled memory tools if the store exists
        if internal_memory_store.is_some() {
            let internal_tools = get_internal_memory_tool_definitions();
            all_caps.tools.extend(internal_tools);
            info!("Added {} internal memory tools to broker capabilities", get_internal_memory_tool_definitions().len());
        }

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

#[derive(Clone)]
struct DaemonState {
    host: Arc<McpHost>,
    memory_store: Option<Arc<MemoryStore>>,
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

    // Load MCP server configurations for McpHost
    let mcp_server_configs = match load_mcp_servers() {
        Ok(configs) => configs,
        Err(e) => {
            error!("Failed to load MCP server configurations: {}", e);
            let _ = fs::remove_file(&socket_path);
            std::process::exit(1);
        }
    };

    // Create MCP Host (implements McpHostInterface)
    let mcp_host = match McpHost::new(mcp_server_configs).await {
        Ok(host) => Arc::new(host),
        Err(e) => {
            error!("Unexpected critical error during MCP Host setup: {}", e);
            let _ = fs::remove_file(&socket_path);
            std::process::exit(1);
        }
    };
    info!("MCP Host initialization completed.");

    // --- Initialize MemoryStore --- 
    let unified_config = UnifiedConfig::load();

    // Directly access fields, assuming unified_config.memory is not Option
    let db_path_opt: Option<PathBuf> = unified_config.memory.db_path.clone();
    
    // Convert Option<String> to Option<EmbeddingModelVariant>
    let embedding_variant_opt: Option<EmbeddingModelVariant> = 
        unified_config.memory.embedding_model_variant.clone().and_then(|s| {
            match EmbeddingModelVariant::from_str(&s) {
                Ok(variant) => Some(variant),
                Err(_) => {
                    warn!("Invalid embedding_model_variant '{}' in config, using default.", s);
                    None
                }
            }
        });

    // Pass the extracted options and the McpHost instance to MemoryStore::new
    let memory_store_instance = match MemoryStore::new(
        db_path_opt, 
        embedding_variant_opt, 
        Some(mcp_host.clone() as Arc<dyn McpHostInterface + Send + Sync>)
    ).await {
        Ok(store) => {
            info!("Initialized embedded MemoryStore successfully.");
            Some(Arc::new(store))
        }
        Err(e) => {
            error!(
                "Failed to initialize embedded MemoryStore: {}. Memory operations via MCP will fail.",
                e
            );
            None
        }
    };
    // --- End MemoryStore Init --- 

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
                let state = DaemonState {
                    host: Arc::clone(&mcp_host),
                    memory_store: memory_store_instance.clone(),
                };

                // Spawn a task to handle this client connection
                tokio::spawn(async move {
                    handle_client(stream, state).await;
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
async fn handle_client(mut stream: UnixStream, state: DaemonState) {
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
        let response = process_request(request, state.clone()).await;

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
async fn process_request(request: DaemonRequest, state: DaemonState) -> DaemonResponse {
    let host = state.host;
    let memory_store = state.memory_store;

    match request {
        DaemonRequest::GetCapabilities => {
            info!("Processing GetCapabilities request");
            // Get capabilities from actual MCP servers
            let mut caps = host.get_all_capabilities().await;
            info!(
                "Retrieved base capabilities for {} tools and {} resources",
                caps.tools.len(),
                caps.resources.len()
            );
            // Manually add internal memory tools if store exists
            if memory_store.is_some() {
                let internal_tools = get_internal_memory_tool_definitions();
                let count = internal_tools.len();
                caps.tools.extend(internal_tools);
                 info!("Added {} internal memory tools to reported capabilities", count);
            }
            DaemonResponse::success(DaemonResult::Capabilities(caps)) // Return the modified caps
        }
        DaemonRequest::ExecuteTool { server, tool, args } => {
            info!(
                "Executing tool '{}' on server '{}' with args: {}",
                tool,
                server,
                serde_json::to_string(&args).unwrap_or_else(|_| "unable to serialize".to_string())
            );

            // --- Intercept memory-store-mcp calls --- 
            if server == "memory-store-mcp" {
                if let Some(ref memory_store_arc) = memory_store { // Use ref memory_store_arc
                    // Handle memory operations internally
                    debug!("Intercepted call for internal memory store: {}", tool);
                    match handle_internal_memory_tool(&tool, args, &*memory_store_arc).await { // Dereference Arc
                        Ok(result_value) => {
                             debug!(
                                "Internal memory tool execution succeeded with result: {}",
                                serde_json::to_string(&result_value).unwrap_or_else(|_| "<unserializable>".to_string())
                            );
                             return DaemonResponse::success(DaemonResult::ExecutionOutput(result_value));
                        }
                        Err(e) => {
                            error!("Internal memory tool '{}' failed: {}", tool, e);
                            return DaemonResponse::error(format!("Internal memory tool error: {}", e));
                        }
                    }
                } else {
                     error!("Received request for memory-store-mcp, but internal store is not initialized.");
                     return DaemonResponse::error("Internal memory store not available".to_string());
                }
            }
            // --- End Intercept --- 

            // If not intercepted, proceed with standard MCP call
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
            match host.get_broker_capabilities(&memory_store).await {
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

// --- NEW HELPER FUNCTION --- 
// Defines the tools handled internally by the embedded MemoryStore
fn get_internal_memory_tool_definitions() -> Vec<CoreTool> {
    vec![
        CoreTool {
            name: "memory-store-mcp/store_memory".to_string(),
            description: Some("Stores a memory item (key, content, tags).".to_string()),
            parameters: None, // Define schema if needed
        },
         CoreTool {
            name: "memory-store-mcp/list_all_memories".to_string(),
            description: Some("Lists all stored memory items.".to_string()),
            parameters: None,
        },
        CoreTool {
            name: "memory-store-mcp/retrieve_memory_by_key".to_string(),
            description: Some("Retrieves a memory item by its key.".to_string()),
            parameters: None,
        },
        CoreTool {
            name: "memory-store-mcp/retrieve_memory_by_tag".to_string(),
            description: Some("Retrieves memory items matching a specific tag.".to_string()),
            parameters: None,
        },
        CoreTool {
            name: "memory-store-mcp/delete_memory_by_key".to_string(),
            description: Some("Deletes a memory item by its key.".to_string()),
            parameters: None,
        },
        CoreTool {
            name: "memory-store-mcp/semantic_search".to_string(),
            description: Some("Performs semantic search over memories.".to_string()),
            parameters: None, // TODO: Define input schema (query_embedding, k, filter)
        },
        // Add other internal tools like add_vector_memory if implemented
    ]
}

// --- NEW FUNCTION: Handle Internal Memory Tool Calls --- 
async fn handle_internal_memory_tool(
    tool_name: &str,
    args: Value,
    memory_store: &MemoryStore,
) -> Result<Value, String> {
    match tool_name {
        "store_memory" => {
            let key = args["key"].as_str().ok_or("Missing string arg: key")?.to_string();
            let content = args["content"].as_str().ok_or("Missing string arg: content")?.to_string();
            let tags: Vec<String> = args["tags"].as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            // Embeddings/metadata might be handled separately or passed if available
            // For now, basic implementation
            memory_store.add_memory(&key, &content, tags, None, None, None).await
                 .map_err(|e| format!("Failed to store memory: {}", e))?;
            Ok(json!({ "success": true, "message": format!("Memory stored with key: {}", key) }))
        }
        "list_all_memories" => {
             let memories = memory_store.get_all_memories().await
                 .map_err(|e| format!("Failed to list memories: {}", e))?;
             // Convert memories to JSON compatible format (consider timestamp formatting)
             let result_memories = memories.into_iter().map(|m| json!({
                 "key": m.key,
                 "content": m.value, // Assuming value is String
                 "tags": m.tags,
                 "timestamp": m.timestamp, // Keep as number or format?
                 // Add other fields as needed
             })).collect::<Vec<_>>();
             Ok(json!({ "memories": result_memories }))
        }
         "retrieve_memory_by_key" => {
            let key = args["key"].as_str().ok_or("Missing string arg: key")?;
            match memory_store.get_by_key(key).await {
                Ok(Some(mem)) => Ok(json!({
                     "memory": {
                         "key": mem.key,
                         "content": mem.value,
                         "tags": mem.tags,
                         "timestamp": mem.timestamp
                    }
                 })),
                Ok(None) => Err(format!("Memory with key '{}' not found", key)),
                Err(e) => Err(format!("Failed to retrieve memory by key: {}", e)),
            }
        }
        "retrieve_memory_by_tag" => {
             let tag = args["tag"].as_str().ok_or("Missing string arg: tag")?;
             let memories = memory_store.get_by_tag(tag).await
                 .map_err(|e| format!("Failed to retrieve memory by tag: {}", e))?;
             let result_memories = memories.into_iter().map(|m| json!({
                 "key": m.key,
                 "content": m.value,
                 "tags": m.tags,
                 "timestamp": m.timestamp
             })).collect::<Vec<_>>();
             Ok(json!({ "memories": result_memories }))
        }
        "delete_memory_by_key" => {
             let key = args["key"].as_str().ok_or("Missing string arg: key")?;
             let count = memory_store.delete_by_key(key).await
                 .map_err(|e| format!("Failed to delete memory: {}", e))?;
             Ok(json!({ "success": true, "message": format!("Memory with key '{}' deleted (count: {})", key, count) }))
        }
        "semantic_search" => {
            // TODO: Implement semantic search call
            // 1. Extract query embedding from args
            // 2. Extract k, filters etc from args
            // 3. Call memory_store.query_memories(...)
            // 4. Format results
            warn!("Internal call to unimplemented tool: semantic_search");
            Err("semantic_search not yet implemented internally".to_string())
        }
        // Add other memory tools here (e.g., add_vector_memory)
        _ => Err(format!("Unknown internal memory tool: {}", tool_name)),
    }
}
