use crate::mcp::config::McpServerConfig;
use crate::mcp::host::io;
use crate::mcp::host::message_handler;
use crate::mcp::host::types::{ActiveServer, CHANNEL_BUFFER_SIZE, PendingRequest, STDIO_BUFFER_SIZE, APP_NAME, APP_VERSION};
use crate::mcp::rpc::{self, ClientInfo, ExecuteToolParams, GetResourceParams, JsonRpcError, Request, Notification};
use log::{debug, error, info, warn};
use serde_json::{self, json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Command};
use tokio::sync::{mpsc, Mutex, oneshot};
use tokio::task;
use reqwest;
use futures_util::StreamExt;

// Implementation methods for ActiveServer
impl ActiveServer {
    // Launch a server using stdio transport and return the server instance and initialization future
    pub async fn launch_stdio(
        next_request_id_ref: &Arc<AtomicU64>,
        config: McpServerConfig,
    ) -> Result<
        (
            ActiveServer,
            tokio::time::Timeout<oneshot::Receiver<Result<Value, JsonRpcError>>>,
        ),
        String,
    > {
        let server_name = config.name.clone();
        info!("Launching MCP server (stdio): {}", server_name);

        if config.command.is_empty() {
            return Err(format!("Server '{}': Empty command", server_name));
        }

        let mut command_parts = config.command.iter();
        let executable = command_parts.next().unwrap(); // Safe due to is_empty check
        let mut cmd = Command::new(executable);
        cmd.args(command_parts);
        cmd.args(&config.args);
        cmd.envs(&config.env);

        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped()); // Capture stderr

        let mut process = cmd
            .spawn()
            .map_err(|e| format!("Server '{}': Failed to spawn: {}", server_name, e))?;

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stdin", server_name))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stdout", server_name))?;
        let stderr = process
            .stderr
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stderr", server_name))?;

        // Channel for sending messages to the stdin writer task
        let (stdin_tx, stdin_rx) = mpsc::channel::<String>(CHANNEL_BUFFER_SIZE);
        let capabilities = Arc::new(Mutex::new(None));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>(); // For signaling tasks to stop

        let process_arc = Arc::new(Mutex::new(process)); // Share process handle for waiting

        // --- Spawn Communication Tasks ---

        // 1. Stderr Handler Task
        let server_name_stderr = server_name.clone();
        let stderr_handle = task::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            loop {
                tokio::select! {
                    read_result = reader.read_line(&mut line) => {
                        match read_result {
                            Ok(0) => { // EOF
                                info!("MCP Server '{}' stderr closed.", server_name_stderr);
                                break;
                            }
                            Ok(_) => {
                                warn!("[MCP stderr - {}]: {}", server_name_stderr, line.trim_end());
                                line.clear();
                            }
                            Err(e) => {
                                error!("Error reading MCP stderr for '{}': {}", server_name_stderr, e);
                                break;
                            }
                        }
                    }
                    // Can add shutdown signal check here if needed
                }
            }
        });

        // 2. Stdin Writer Task
        let server_name_stdin = server_name.clone();
        let mut stdin_writer = BufWriter::with_capacity(STDIO_BUFFER_SIZE, stdin);
        let mut stdin_rx_local = stdin_rx; // Move receiver into task
        let shutdown_rx_stdin = shutdown_rx; // Receiver for shutdown signal
        let writer_handle = task::spawn(async move {
            tokio::select! {
                _ = io::stdin_writer_loop(&server_name_stdin, &mut stdin_writer, &mut stdin_rx_local) => {
                    info!("Stdin writer loop for '{}' finished.", server_name_stdin);
                }
                _ = shutdown_rx_stdin => {
                    info!("Stdin writer for '{}' received shutdown signal.", server_name_stdin);
                }
            }
            // Ensure buffer is flushed on exit
            if let Err(e) = stdin_writer.flush().await {
                error!("Error flushing stdin for '{}': {}. Stopping writer.", server_name_stdin, e);
            }
            info!("Stdin writer task for '{}' exited.", server_name_stdin);
        });


        // 3. Stdout Reader / Dispatcher Task
        let server_name_stdout = server_name.clone();
        let capabilities_clone = capabilities.clone();
        let pending_requests_clone = pending_requests.clone();
        let process_arc_clone = process_arc.clone();
        // Use a separate shutdown channel for the reader, triggered by the main shutdown signal
        let (_reader_shutdown_tx, _reader_shutdown_rx) = oneshot::channel::<()>();
        let reader_handle = task::spawn(async move {
            let mut reader = BufReader::with_capacity(STDIO_BUFFER_SIZE, stdout);
            message_handler::stdout_reader_loop(
                &server_name_stdout,
                &mut reader,
                capabilities_clone,
                pending_requests_clone
            ).await;
            info!("Stdout reader task for '{}' exited.", server_name_stdout);

            // Attempt to reap the process if reader exits (might indicate process termination)
            // This might be too early if only stdout closed. Consider a dedicated process monitor task.
            let mut process_guard = process_arc_clone.lock().await;
            match process_guard.try_wait() {
                Ok(Some(status)) => info!("MCP process '{}' exited with status: {}", server_name_stdout, status),
                Ok(None) => { /* Still running */ },
                Err(e) => error!("Error waiting for MCP process '{}': {}", server_name_stdout, e),
            }
        });


        // --- Send Initialize Request ---
        let init_request_id = next_request_id_ref.fetch_add(1, Ordering::Relaxed);
        let (init_responder_tx, init_responder_rx) = oneshot::channel();

        pending_requests.lock().await.insert(
            init_request_id,
            PendingRequest {
                responder: init_responder_tx,
                method: "initialize".to_string(),
            },
        );

        let client_info = ClientInfo {
            name: APP_NAME.to_string(),
            version: APP_VERSION.to_string(),
        };
        let init_params = rpc::InitializeParams {
            client_info,
            trace: None,
        };
        let init_request = Request::new(
            Some(json!(init_request_id)),
            "initialize".to_string(),
            Some(serde_json::to_value(init_params).map_err(|e| {
                format!(
                    "Server '{}': Failed to serialize init params: {}",
                    server_name, e
                )
            })?),
        );

        let request_json = serde_json::to_string(&init_request).map_err(|e| {
            format!(
                "Server '{}': Failed to serialize init request: {}",
                server_name, e
            )
        })?;

        // Send via channel to stdin writer task
        if let Err(e) = stdin_tx.send(request_json).await {
            let err_msg = format!(
                "Server '{}': Failed to send initialize request to stdin channel: {}",
                server_name, e
            );
            error!("{}", err_msg);
            // Clean up pending request if send fails
            pending_requests.lock().await.remove(&init_request_id);
            return Err(err_msg);
        }
        debug!("Sent initialize request (id={}) to {}", init_request_id, server_name);

        // Return server handle and future to await initialization result
        let init_timeout = Duration::from_secs(10); // Timeout for initialize response
        Ok((
            ActiveServer {
                config: config.clone(),
                process: process_arc.clone(),
                stdin_tx: stdin_tx.clone(),
                capabilities: capabilities.clone(),
                pending_requests: pending_requests.clone(),
                reader_task: Arc::new(Mutex::new(Some(reader_handle))),
                writer_task: Arc::new(Mutex::new(Some(writer_handle))),
                stderr_task: Arc::new(Mutex::new(Some(stderr_handle))),
                shutdown_signal: Arc::new(Mutex::new(Some(shutdown_tx))),
                should_stop: Arc::new(Mutex::new(false)),
            },
            tokio::time::timeout(init_timeout, init_responder_rx),
        ))
    }

    // Launch a server using SSE (Server-Sent Events) transport
    pub async fn launch_sse(
        next_request_id_ref: &Arc<AtomicU64>,
        config: McpServerConfig,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> Result<
        (
            ActiveServer,
            tokio::time::Timeout<oneshot::Receiver<Result<Value, JsonRpcError>>>,
        ),
        String,
    > {
        let server_name = config.name.clone();
        info!("Connecting to MCP server (SSE): {} at {}", server_name, url);
        
        // Create shared state
        let capabilities = Arc::new(Mutex::new(None));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(CHANNEL_BUFFER_SIZE);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        
        // Create a flag to indicate when writer should stop
        let should_stop = Arc::new(Mutex::new(false));
        
        // Configure HTTP client for SSE connection
        let mut client_builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(30));
        
        // Add headers if provided
        if let Some(header_map) = headers {
            let mut headers = reqwest::header::HeaderMap::new();
            for (key, value) in header_map {
                match reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                    Ok(header_name) => {
                        match reqwest::header::HeaderValue::from_str(&value) {
                            Ok(header_value) => { headers.insert(header_name, header_value); }
                            Err(e) => return Err(format!("Invalid header value for '{}': {}", key, e))
                        }
                    }
                    Err(e) => return Err(format!("Invalid header name '{}': {}", key, e))
                }
            }
            client_builder = client_builder.default_headers(headers);
        }
        
        let client = client_builder.build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
        
        // Process for SSE is created as a dummy process that's never used
        // This is to maintain compatibility with the stdio API
        let process = Arc::new(Mutex::new(
            tokio::process::Command::new("echo")
                .arg("dummy")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Failed to create dummy process: {}", e))?
        ));
        
        // SSE reader task
        let server_name_clone = server_name.clone();
        let capabilities_clone = capabilities.clone();
        let pending_requests_clone = pending_requests.clone();
        let url_clone = url.clone();
        let client_clone = client.clone();
        let should_stop_clone = should_stop.clone();
        
        let sse_reader_task = task::spawn(async move {
            // Connect to SSE endpoint
            let request = client_clone.get(&url_clone)
                .header("Accept", "text/event-stream");
            
            let response = match request.send().await {
                Ok(resp) => {
                    if !resp.status().is_success() {
                        error!("SSE connection failed with status {}: {}", 
                               resp.status(), server_name_clone);
                        // Signal writer to stop
                        *should_stop_clone.lock().await = true;
                        return;
                    }
                    resp
                },
                Err(e) => {
                    error!("Failed to connect to SSE endpoint for '{}': {}", 
                           server_name_clone, e);
                    // Signal writer to stop
                    *should_stop_clone.lock().await = true;
                    return;
                }
            };
            
            // Process SSE events
            let mut byte_stream = response.bytes_stream();
            let mut buffer = Vec::new();
            
            loop {
                tokio::select! {
                    // Process incoming SSE events
                    Some(chunk_result) = byte_stream.next() => {
                        match chunk_result {
                            Ok(chunk) => {
                                // Process the chunk data, looking for complete JSON-RPC messages
                                for byte in chunk {
                                    buffer.push(byte);
                                    
                                    // Check if we have a complete message (for simplicity, assuming each message
                                    // is a separate SSE event with proper line endings)
                                    if buffer.ends_with(b"\n\n") {
                                        // Try to parse as JSON-RPC message
                                        let json_str = String::from_utf8_lossy(&buffer);
                                        
                                        // Extract JSON content from SSE format (data: {...}\n\n)
                                        let json_content = json_str.lines()
                                            .filter(|line| line.starts_with("data: "))
                                            .map(|line| &line[6..]) // Skip "data: " prefix
                                            .collect::<Vec<_>>()
                                            .join("");
                                        
                                        if !json_content.is_empty() {
                                            debug!("Received SSE event from '{}': {}", 
                                                   server_name_clone, json_content);
                                            
                                            // Process the JSON-RPC message
                                            match serde_json::from_str::<rpc::Message>(&json_content) {
                                                Ok(rpc::Message::Response(response)) => {
                                                    message_handler::handle_response(
                                                        &server_name_clone,
                                                        response,
                                                        capabilities_clone.clone(),
                                                        pending_requests_clone.clone()
                                                    ).await;
                                                },
                                                Ok(rpc::Message::Notification(notification)) => {
                                                    message_handler::handle_notification(
                                                        &server_name_clone,
                                                        notification
                                                    ).await;
                                                },
                                                Ok(rpc::Message::Request(request)) => {
                                                    message_handler::handle_server_request(
                                                        &server_name_clone,
                                                        request
                                                    ).await;
                                                },
                                                Err(e) => {
                                                    error!("Error parsing SSE message from '{}': {}", 
                                                           server_name_clone, e);
                                                }
                                            }
                                        }
                                        
                                        // Clear buffer for next message
                                        buffer.clear();
                                    }
                                }
                            },
                            Err(e) => {
                                error!("Error reading SSE stream for '{}': {}", 
                                       server_name_clone, e);
                                break;
                            }
                        }
                    },
                    
                    else => break
                }
            }
            
            info!("SSE reader task for '{}' exited", server_name_clone);
        });
        
        // SSE writer task
        let server_name_clone = server_name.clone();
        let client_clone = client.clone();
        let url_clone = url.clone();
        let should_stop_clone = should_stop.clone();
        
        let sse_writer_task = task::spawn(async move {
            // Process outgoing messages to send to the SSE server
            loop {
                // First check if we should stop
                if *should_stop_clone.lock().await {
                    info!("SSE writer for '{}' stopping due to stop flag", server_name_clone);
                    break;
                }
                
                // Use a timeout to periodically check the should_stop flag
                let message = tokio::select! {
                    Some(msg) = stdin_rx.recv() => Some(msg),
                    _ = tokio::time::sleep(Duration::from_millis(100)) => None,
                };
                
                if let Some(message) = message {
                    debug!("Sending to SSE server '{}': {}", server_name_clone, message);
                    
                    // Send message via HTTP POST to the SSE server's submission endpoint
                    let result = client_clone.post(&url_clone)
                        .header("Content-Type", "application/json")
                        .body(message)
                        .send()
                        .await;
                    
                    if let Err(e) = result {
                        error!("Failed to send message to SSE server '{}': {}", 
                               server_name_clone, e);
                    }
                }
            }
            
            info!("SSE writer task for '{}' exited", server_name_clone);
        });
        
        // Setup initialization request
        let init_request_id = next_request_id_ref.fetch_add(1, Ordering::Relaxed);
        let (init_responder_tx, init_responder_rx) = oneshot::channel();
        
        pending_requests.lock().await.insert(
            init_request_id,
            PendingRequest {
                responder: init_responder_tx,
                method: "initialize".to_string(),
            },
        );
        
        let client_info = ClientInfo {
            name: APP_NAME.to_string(),
            version: APP_VERSION.to_string(),
        };
        
        let init_params = rpc::InitializeParams {
            client_info,
            trace: None,
        };
        
        let init_request = Request::new(
            Some(json!(init_request_id)),
            "initialize".to_string(),
            Some(serde_json::to_value(init_params).map_err(|e| {
                format!("Failed to serialize init params: {}", e)
            })?),
        );
        
        let request_json = serde_json::to_string(&init_request)
            .map_err(|e| format!("Failed to serialize init request: {}", e))?;
        
        // Send initialization request
        if let Err(e) = stdin_tx.send(request_json).await {
            pending_requests.lock().await.remove(&init_request_id);
            return Err(format!("Failed to send initialize request: {}", e));
        }
        
        // Create ActiveServer instance
        let active_server = ActiveServer {
            config,
            process,
            stdin_tx: stdin_tx.clone(),
            capabilities: capabilities.clone(),
            pending_requests: pending_requests.clone(),
            reader_task: Arc::new(Mutex::new(Some(sse_reader_task))),
            writer_task: Arc::new(Mutex::new(Some(sse_writer_task))),
            stderr_task: Arc::new(Mutex::new(None)), // No stderr task for SSE
            shutdown_signal: Arc::new(Mutex::new(Some(shutdown_tx))),
            should_stop: should_stop,
        };
        
        // Return server and initialization future
        let init_timeout = Duration::from_secs(10);
        Ok((
            active_server,
            tokio::time::timeout(init_timeout, init_responder_rx),
        ))
    }

    // Launch a server using WebSocket transport
    pub async fn launch_websocket(
        next_request_id_ref: &Arc<AtomicU64>,
        config: McpServerConfig,
        url: String,
        headers: Option<HashMap<String, String>>,
    ) -> Result<
        (
            ActiveServer,
            tokio::time::Timeout<oneshot::Receiver<Result<Value, JsonRpcError>>>,
        ),
        String,
    > {
        let server_name = config.name.clone();
        info!("Connecting to MCP server (WebSocket): {} at {}", server_name, url);
        
        // Create shared state
        let capabilities = Arc::new(Mutex::new(None));
        let pending_requests = Arc::new(Mutex::new(HashMap::new()));
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(CHANNEL_BUFFER_SIZE);
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
        
        // Create a flag to indicate when writer should stop
        let should_stop = Arc::new(Mutex::new(false));
        
        // Process for WebSocket is created as a dummy process that's never used
        // This is to maintain compatibility with the stdio API
        let process = Arc::new(Mutex::new(
            tokio::process::Command::new("echo")
                .arg("dummy")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("Failed to create dummy process: {}", e))?
        ));
        
        // WebSocket connection
        // Note: This implementation sketch assumes tokio_tungstenite for WebSocket handling
        // Real implementation would need to add this dependency to Cargo.toml
        /*
        let ws_url = url::Url::parse(&url)
            .map_err(|e| format!("Invalid WebSocket URL: {}", e))?;
            
        let mut request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(ws_url.as_str())
            .header("User-Agent", format!("{}/{}", APP_NAME, APP_VERSION));
            
        // Add custom headers if provided
        if let Some(header_map) = &headers {
            for (key, value) in header_map {
                request = request.header(key, value);
            }
        }
        
        let request = request.body(())
            .map_err(|e| format!("Failed to build WebSocket request: {}", e))?;
            
        // Connect to WebSocket server
        let (ws_stream, _) = tokio_tungstenite::connect_async(request).await
            .map_err(|e| format!("Failed to connect to WebSocket server: {}", e))?;
            
        let (write, read) = ws_stream.split();
        */
        
        // For now, we'll just log that WebSocket is not fully implemented
        warn!("WebSocket transport not fully implemented. Use stdio transport for now.");
        
        // Setup initialization request
        let init_request_id = next_request_id_ref.fetch_add(1, Ordering::Relaxed);
        let (init_responder_tx, init_responder_rx) = oneshot::channel();
        
        pending_requests.lock().await.insert(
            init_request_id,
            PendingRequest {
                responder: init_responder_tx,
                method: "initialize".to_string(),
            },
        );
        
        // Create dummy tasks that just wait for shutdown
        let server_name_clone = server_name.clone();
        let ws_reader_task = task::spawn(async move {
            info!("WebSocket reader task for '{}' started (placeholder)", server_name_clone);
            let _ = shutdown_rx.await;
            info!("WebSocket reader task for '{}' received shutdown signal", server_name_clone);
        });
        
        let server_name_clone = server_name.clone();
        let ws_writer_task = task::spawn(async move {
            info!("WebSocket writer task for '{}' started (placeholder)", server_name_clone);
            while let Some(message) = stdin_rx.recv().await {
                debug!("Received message to send via WebSocket (dropping): {}", message);
            }
            info!("WebSocket writer task for '{}' exited", server_name_clone);
        });
        
        // Create ActiveServer instance
        let active_server = ActiveServer {
            config,
            process,
            stdin_tx: stdin_tx.clone(),
            capabilities: capabilities.clone(),
            pending_requests: pending_requests.clone(),
            reader_task: Arc::new(Mutex::new(Some(ws_reader_task))),
            writer_task: Arc::new(Mutex::new(Some(ws_writer_task))),
            stderr_task: Arc::new(Mutex::new(None)), // No stderr task for WebSocket
            shutdown_signal: Arc::new(Mutex::new(Some(shutdown_tx))),
            should_stop: Arc::new(Mutex::new(false)),
        };
        
        // Return error for now since WebSocket is not fully implemented
        // In a real implementation, we would return the active server and init future
        Err("WebSocket transport not fully implemented. Use stdio transport for now.".to_string())
    }

    // Sends a "tool/execute" request to the server and awaits the response.
    pub async fn execute_tool(&self, request_id: u64, params: ExecuteToolParams) -> Result<Value, JsonRpcError> {
        // For file operations, we can first get metadata if needed
        if params.tool_name == "file_read" || params.tool_name == "file_write" {
            if let Some(path) = params.arguments.get("path").and_then(|p| p.as_str()) {
                // Get file metadata first to verify file exists/permissions
                if let Ok(metadata) = self.get_file_metadata(path).await {
                    debug!("Retrieved file metadata for '{}': {:?}", path, metadata);
                    // Continue with operation knowing file is accessible
                }
            }
        }

        // 1. Create Request
        let request = Request::new(
            Some(json!(request_id)),
            "mcp/tool/execute".to_string(),
            Some(serde_json::to_value(&params).map_err(|e| {
                // Convert serialization error to a JsonRpcError
                JsonRpcError {
                    code: -32603, // Internal error
                    message: format!("Failed to serialize ExecuteToolParams: {}", e),
                    data: None,
                }
            })?),
        );

        // 2. Serialize Request
        let request_json = serde_json::to_string(&request).map_err(|e| JsonRpcError {
            code: -32603, // Internal error
            message: format!("Failed to serialize tool/execute request: {}", e),
            data: None,
        })?;

        // 3. Create Responder Channel
        let (responder_tx, responder_rx) = oneshot::channel();

        // 4. Store Pending Request
        self.pending_requests.lock().await.insert(
            request_id,
            PendingRequest {
                responder: responder_tx,
                method: request.method, // Store "tool/execute"
            },
        );

        // 5. Send Request
        debug!("Sending tool/execute req id={} to {}", request_id, self.config.name);
        if let Err(send_err) = self.stdin_tx.send(request_json).await {
            error!(
                "Failed to send tool/execute request to '{}': {}",
                self.config.name,
                send_err
            );
            // Remove pending request if send fails immediately
            self.pending_requests.lock().await.remove(&request_id);
            return Err(JsonRpcError {
                code: -32000, // Example server error code
                message: format!("Failed to send request to server: {}", send_err),
                data: None,
            });
        }

        // 6. Await Response (with timeout)
        let timeout_duration = Duration::from_secs(60); // Example timeout
        match tokio::time::timeout(timeout_duration, responder_rx).await {
            Ok(Ok(result)) => {
                // Received Result<Value, JsonRpcError> from the channel
                info!("Tool execution response received for req id={}", request_id);
                result // Propagate the inner result
            }
            Ok(Err(recv_err)) => { // oneshot channel closed unexpectedly
                error!("Responder channel closed for tool request id={}: {}", request_id, recv_err);
                self.pending_requests.lock().await.remove(&request_id);
                Err(JsonRpcError {
                    code: -32001, // Example server error code
                    message: format!("Failed to receive tool response: channel closed"),
                    data: Some(json!(recv_err.to_string())),
                })
            }
            Err(_) => { // Timeout elapsed
                warn!("Tool execution timed out for request id={}", request_id);
                // Remove the pending request on timeout
                self.pending_requests.lock().await.remove(&request_id);
                Err(JsonRpcError {
                    code: -32002, // Example server error code
                    message: format!("Tool execution timed out after {}s", timeout_duration.as_secs()),
                    data: None,
                })
            }
        }
    }

    // Sends a "resource/get" request to the server and awaits the response.
    pub async fn get_resource(&self, request_id: u64, params: GetResourceParams) -> Result<Value, JsonRpcError> {
        // 1. Create Request
        let request = Request::new(
            Some(json!(request_id)),
            "resource/get".to_string(),
            Some(serde_json::to_value(&params).map_err(|e| {
                JsonRpcError {
                    code: -32603, // Internal error
                    message: format!("Failed to serialize GetResourceParams: {}", e),
                    data: None,
                }
            })?),
        );

        // 2. Serialize Request
        let request_json = serde_json::to_string(&request).map_err(|e| JsonRpcError {
            code: -32603, // Internal error
            message: format!("Failed to serialize resource/get request: {}", e),
            data: None,
        })?;

        // 3. Create Responder Channel
        let (responder_tx, responder_rx) = oneshot::channel();

        // 4. Store Pending Request
        self.pending_requests.lock().await.insert(
            request_id,
            PendingRequest {
                responder: responder_tx,
                method: request.method, // Store "resource/get"
            },
        );

        // 5. Send Request
        debug!("Sending resource/get req id={} to {}", request_id, self.config.name);
        if let Err(send_err) = self.stdin_tx.send(request_json).await {
            error!(
                "Failed to send resource/get request to '{}': {}",
                self.config.name,
                send_err
            );
            self.pending_requests.lock().await.remove(&request_id);
            return Err(JsonRpcError {
                code: -32000,
                message: format!("Failed to send request to server: {}", send_err),
                data: None,
            });
        }

        // 6. Await Response (with timeout)
        let timeout_duration = Duration::from_secs(30); // Example timeout
        match tokio::time::timeout(timeout_duration, responder_rx).await {
            Ok(Ok(result)) => {
                info!("Resource retrieval response received for req id={}", request_id);
                result // Propagate the inner Result<Value, JsonRpcError>
            }
            Ok(Err(recv_err)) => { // Channel closed
                error!("Responder channel closed for resource request id={}: {}", request_id, recv_err);
                self.pending_requests.lock().await.remove(&request_id);
                Err(JsonRpcError {
                    code: -32001,
                    message: format!("Failed to receive resource response: channel closed"),
                    data: Some(json!(recv_err.to_string())),
                })
            }
            Err(_) => { // Timeout
                warn!("Resource retrieval timed out for request id={}", request_id);
                self.pending_requests.lock().await.remove(&request_id);
                Err(JsonRpcError {
                    code: -32002,
                    message: format!("Resource retrieval timed out after {}s", timeout_duration.as_secs()),
                    data: None,
                })
            }
        }
    }

    // Helper method to get file metadata as resource
    pub async fn get_file_metadata(&self, file_path: &str) -> Result<Value, JsonRpcError> {
        // Create GetResourceParams to request file metadata
        let params = GetResourceParams {
            name: "file_info".to_string(),
            params: Some(json!({
                "path": file_path
            })),
        };
        
        // Use get_resource to fetch the metadata
        let request_id = std::sync::atomic::AtomicU64::new(0).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.get_resource(request_id, params).await
    }

    // Send a notification to the server
    pub async fn send_notification(&self, notification: &Notification) -> Result<(), String> {
        // Serialize the notification
        let notification_json = match serde_json::to_string(notification) {
            Ok(json) => json,
            Err(e) => return Err(format!("Failed to serialize notification: {}", e)),
        };
        
        // Send to server
        if let Err(e) = self.stdin_tx.send(notification_json).await {
            return Err(format!("Failed to send notification: {}", e));
        }
        
        Ok(())
    }
} 