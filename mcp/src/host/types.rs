use crate::config::McpServerConfig;
use crate::rpc::Notification;
use gemini_core::rpc_types::{JsonRpcError, Request, Response, ServerCapabilities};
use log::{debug, error, info, warn};
use serde_json;
use std::collections::HashMap;
use std::future::Future;
use std::io::ErrorKind;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tokio::task;
use tokio::time::{error::Elapsed, Duration};

// ActiveServer manages an active MCP server connection
#[derive(Clone, Debug)]
pub(crate) struct ActiveServer {
    pub config: McpServerConfig,

    // Capabilities discovered during initialize
    pub capabilities: Arc<Mutex<Option<ServerCapabilities>>>,

    // Channel to send requests to the server
    request_tx: mpsc::Sender<(Request, oneshot::Sender<Result<Response, JsonRpcError>>)>,

    // Channel to send notifications to the server
    notification_tx: mpsc::Sender<Notification>,

    // For stdio transport only: handle to child process
    #[allow(dead_code)] // Used in take_process but clippy doesn't see it
    process: Arc<Mutex<Option<tokio::process::Child>>>,

    // Flag to indicate shutdown in progress
    shutdown: Arc<AtomicBool>,
}

// Use tokio oneshot for request/response
use tokio::sync::oneshot;

// Define a concrete future type for the initialization future
type InitFuture = Pin<Box<dyn Future<Output = Result<Result<(), JsonRpcError>, Elapsed>> + Send>>;

// Constants for buffer sizes
const STDIO_BUFFER_SIZE: usize = 8192;
const CHANNEL_BUFFER_SIZE: usize = 32;
const JSON_RPC_PARSE_BUFFER_SIZE: usize = 4096;

// Simple structure to track pending requests
#[derive(Debug)]
struct PendingRequest {
    responder: oneshot::Sender<Result<Response, JsonRpcError>>,
    method: String, // For debugging/logging
}

impl ActiveServer {
    // Create a new server with stdio transport
    pub(crate) async fn launch_stdio(
        _next_request_id: &Arc<std::sync::atomic::AtomicU64>,
        config: McpServerConfig,
    ) -> Result<(Self, InitFuture), String> {
        let server_name = config.name.clone();
        info!("Launching MCP server (stdio): {}", server_name);

        if config.command.is_empty() {
            return Err(format!("Server '{}': Empty command", server_name));
        }

        // Set up command with executable and arguments
        let mut command_parts = config.command.iter();
        let executable = command_parts.next().unwrap(); // Safe due to is_empty check
        let mut cmd = Command::new(executable);
        cmd.args(command_parts);
        cmd.args(&config.args);
        cmd.envs(&config.env);

        // Configure stdin, stdout, stderr pipes
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn the process
        let mut process = cmd
            .spawn()
            .map_err(|e| format!("Server '{}': Failed to spawn: {}", server_name, e))?;

        // Get stdin/stdout/stderr handles
        let _actual_stdin = process
            .stdin
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stdin", server_name))?;
        let _actual_stdout = process
            .stdout
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stdout", server_name))?;
        let _actual_stderr = process
            .stderr
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stderr", server_name))?;

        // Create channels for communication
        let (_request_tx, mut _request_rx): (
            mpsc::Sender<(Request, oneshot::Sender<Result<Response, JsonRpcError>>)>,
            _,
        ) = mpsc::channel(CHANNEL_BUFFER_SIZE);
        let (_notification_tx, mut _notification_rx): (mpsc::Sender<Notification>, _) =
            mpsc::channel(CHANNEL_BUFFER_SIZE);
        let (_stdin_tx, mut _stdin_rx): (mpsc::Sender<String>, _) =
            mpsc::channel(CHANNEL_BUFFER_SIZE);

        // Create shared state
        let _capabilities = Arc::new(Mutex::new(None::<ServerCapabilities>));
        let _pending_requests = Arc::new(Mutex::new(HashMap::<u64, PendingRequest>::new()));
        let _process_arc = Arc::new(Mutex::new(Some(process)));
        let _shutdown = Arc::new(AtomicBool::new(false));

        // Clone for tasks
        let shutdown_for_request = _shutdown.clone();
        let shutdown_for_notification = _shutdown.clone();
        let shutdown_for_stdin = _shutdown.clone();
        let server_name_stderr = server_name.clone();
        let server_name_stdin = server_name.clone();
        let server_name_stdout = server_name.clone();
        let pending_requests_clone = _pending_requests.clone();
        let capabilities_clone = _capabilities.clone();

        // Spawn stderr handler task
        task::spawn(async move {
            let mut reader = BufReader::new(_actual_stderr);
            let mut line = String::new();
            loop {
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF
                        info!("MCP Server '{}' stderr closed", server_name_stderr);
                        break;
                    }
                    Ok(_) => {
                        warn!("[MCP stderr - {}]: {}", server_name_stderr, line.trim_end());
                        line.clear();
                    }
                    Err(e) => {
                        error!(
                            "Error reading MCP stderr for '{}': {}",
                            server_name_stderr, e
                        );
                        break;
                    }
                }
            }
        });

        // Spawn stdout reader task
        task::spawn(async move {
            let mut reader = BufReader::with_capacity(STDIO_BUFFER_SIZE, _actual_stdout);
            let mut buffer = Vec::with_capacity(JSON_RPC_PARSE_BUFFER_SIZE); // Use Vec for easier clearing and resizing

            loop {
                let mut content_length: Option<usize> = None;
                buffer.clear(); // Clear buffer for headers

                // Read headers until empty line
                debug!("Stdout({}): Waiting for headers...", server_name_stdout);
                loop {
                    let start_len = buffer.len(); // Track buffer growth
                    match reader.read_until(b'\n', &mut buffer).await {
                        Ok(0) => {
                            // EOF
                            info!(
                                "Stdout({}): Stream closed (EOF received while reading headers).",
                                server_name_stdout
                            );
                            // Clean up pending requests on EOF
                            let mut requests = pending_requests_clone.lock().await;
                            for (_, pending) in requests.drain() {
                                let _ = pending.responder.send(Err(JsonRpcError {
                                    code: -32001, // Custom code for unexpected close
                                    message: "Connection closed unexpectedly while reading headers"
                                        .to_string(),
                                    data: None,
                                }));
                            }
                            return;
                        }
                        Ok(bytes_read) => {
                            let line_bytes = &buffer[start_len..];
                            let line = String::from_utf8_lossy(line_bytes);
                            let line_trimmed = line.trim_end(); // Trim \r\n or \n
                            debug!(
                                "Stdout({}): Read header line ({} bytes): '{}'",
                                server_name_stdout, bytes_read, line_trimmed
                            );

                            if line_trimmed.is_empty() {
                                // End of headers
                                debug!("Stdout({}): Empty header line received, proceeding to read content.", server_name_stdout);
                                break;
                            }

                            // Parse Content-Length, case-insensitive
                            if line_trimmed
                                .to_ascii_lowercase()
                                .starts_with("content-length:")
                            {
                                if let Some(len_str) = line_trimmed.split(':').nth(1) {
                                    if let Ok(len) = len_str.trim().parse::<usize>() {
                                        debug!(
                                            "Stdout({}): Parsed Content-Length: {}",
                                            server_name_stdout, len
                                        );
                                        content_length = Some(len);
                                    } else {
                                        warn!("Stdout({}): Failed to parse Content-Length value: '{}'", server_name_stdout, len_str.trim());
                                    }
                                } else {
                                    warn!(
                                        "Stdout({}): Malformed Content-Length line: '{}'",
                                        server_name_stdout, line_trimmed
                                    );
                                }
                            }
                            // Note: LSP spec allows other headers, we just ignore them
                        }
                        Err(e) => {
                            error!(
                                "Stdout({}): Error reading headers: {}",
                                server_name_stdout, e
                            );
                            // Clean up pending requests on error
                            let mut requests = pending_requests_clone.lock().await;
                            for (_, pending) in requests.drain() {
                                let _ = pending.responder.send(Err(JsonRpcError {
                                    code: -32002, // Custom code for read error
                                    message: format!("Error reading stdout headers: {}", e),
                                    data: None,
                                }));
                            }
                            return;
                        }
                    }
                } // End header reading loop

                // Read the content based on content-length
                if let Some(length) = content_length {
                    if length == 0 {
                        warn!(
                            "Stdout({}): Received Content-Length: 0, skipping content read.",
                            server_name_stdout
                        );
                        continue; // Nothing to read, wait for next message
                    }

                    let mut content = vec![0; length];
                    debug!(
                        "Stdout({}): Attempting to read {} bytes of content...",
                        server_name_stdout, length
                    );
                    match reader.read_exact(&mut content).await {
                        Ok(_) => {
                            let json_str_result = String::from_utf8(content);
                            match json_str_result {
                                Ok(json_str) => {
                                    debug!(
                                        "Stdout({}): Received content ({} bytes): {}",
                                        server_name_stdout, length, json_str
                                    );

                                    // Process the message
                                    match serde_json::from_str::<serde_json::Value>(&json_str) {
                                        Ok(json_value) => {
                                            // Check if it's a Response or Request/Notification (though servers shouldn't send requests)
                                            if json_value.get("id").is_some()
                                                && (json_value.get("result").is_some()
                                                    || json_value.get("error").is_some())
                                            {
                                                match serde_json::from_value::<Response>(json_value)
                                                {
                                                    Ok(response) => {
                                                        // Handle response
                                                        let request_id = match &response.id {
                                                            serde_json::Value::Number(n) => {
                                                                n.as_u64()
                                                            }
                                                            _ => None, // Ignore responses with non-numeric IDs for now
                                                        };

                                                        if let Some(id) = request_id {
                                                            // Remove from pending requests and send response to requester
                                                            if let Some(pending) =
                                                                pending_requests_clone
                                                                    .lock()
                                                                    .await
                                                                    .remove(&id)
                                                            {
                                                                debug!("Stdout({}): Matched response ID {} to pending request '{}'", server_name_stdout, id, pending.method);
                                                                if pending.method == "initialize" {
                                                                    // If initialize, update capabilities
                                                                    if let Ok(result) =
                                                                        &response.result()
                                                                    {
                                                                        match serde_json::from_value::<
                                                                            ServerInitializeResult,
                                                                        >(
                                                                            result.clone()
                                                                        ) {
                                                                            Ok(init_result) => {
                                                                                debug!("Stdout({}): Received capabilities: {:?}", server_name_stdout, init_result.capabilities);
                                                                                *capabilities_clone.lock().await = Some(init_result.capabilities);
                                                                            }
                                                                            Err(e) => {
                                                                                error!("Stdout({}): Failed to deserialize InitializeResult from {:?}: {}", server_name_stdout, result, e);
                                                                            }
                                                                        }
                                                                    } else if let Some(err) =
                                                                        &response.error
                                                                    {
                                                                        error!("Stdout({}): Initialize request failed with error: {:?}", server_name_stdout, err);
                                                                    }
                                                                }

                                                                // Send response back
                                                                debug!("Stdout({}): Sending response for ID {} back to requester.", server_name_stdout, id);
                                                                if let Err(_) = pending
                                                                    .responder
                                                                    .send(Ok(response))
                                                                {
                                                                    warn!("Stdout({}): Failed to send response for ID {} back to requester (receiver dropped).", server_name_stdout, id);
                                                                }
                                                            } else {
                                                                warn!("Stdout({}): Received response for unknown or timed-out request ID: {}", server_name_stdout, id);
                                                            }
                                                        } else {
                                                            warn!("Stdout({}): Received response with non-numeric or null ID: {:?}", server_name_stdout, response.id);
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("Stdout({}): Failed to parse valid JSON as Response: {}. JSON: {}", server_name_stdout, e, json_str);
                                                    }
                                                }
                                            } else if json_value.get("method").is_some() {
                                                // Handle potential notifications or requests from server (e.g. logging)
                                                warn!("Stdout({}): Received notification/request from server (unexpected): {}", server_name_stdout, json_str);
                                                // TODO: Implement notification handling from server if needed
                                            } else {
                                                error!("Stdout({}): Received JSON is not a recognizable RPC message: {}", server_name_stdout, json_str);
                                            }
                                        }
                                        Err(e) => {
                                            error!("Stdout({}): Failed to parse received content as JSON: {}. Content: '{}'", server_name_stdout, e, json_str);
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Use from_utf8_lossy for logging if original conversion failed
                                    let lossy_content = String::from_utf8_lossy(e.as_bytes());
                                    error!("Stdout({}): Invalid UTF-8 received ({} bytes): {}. Lossy representation: '{}'", server_name_stdout, length, e, lossy_content);
                                }
                            }
                        }
                        Err(e) => {
                            error!(
                                "Stdout({}): Error reading content (expected {} bytes): {}",
                                server_name_stdout, length, e
                            );
                            if e.kind() == ErrorKind::UnexpectedEof {
                                // EOF during content read - server probably crashed or closed stream prematurely
                                info!(
                                    "Stdout({}): EOF encountered while reading message content.",
                                    server_name_stdout
                                );
                                // Clean up pending requests
                                let mut requests = pending_requests_clone.lock().await;
                                for (_, pending) in requests.drain() {
                                    let _ = pending.responder.send(Err(JsonRpcError {
                                        code: -32003, // Custom code for unexpected close during content read
                                        message: "Connection closed unexpectedly while reading message content".to_string(),
                                        data: None,
                                    }));
                                }
                                break; // Exit the loop
                            }
                            // For other errors, we might be able to recover, but it's risky. Let's log and continue waiting for headers.
                            // Consider adding logic to break if errors persist.
                        }
                    }
                } else {
                    warn!("Stdout({}): No Content-Length header found after reading headers. Raw buffer: '{}'", server_name_stdout, String::from_utf8_lossy(&buffer));
                    // If no content-length, we might be out of sync. Try reading headers again.
                }
            } // End main loop

            info!("Stdout({}): Reader task exiting.", server_name_stdout);
            // Final cleanup in case the loop exited cleanly but requests were still pending (shouldn't happen with proper shutdown)
            let mut requests = pending_requests_clone.lock().await;
            if !requests.is_empty() {
                warn!(
                    "Stdout({}): Reader task exiting with {} pending requests.",
                    server_name_stdout,
                    requests.len()
                );
                for (_, pending) in requests.drain() {
                    let _ = pending.responder.send(Err(JsonRpcError {
                        code: -32004, // Custom code for exit with pending requests
                        message: "Stdout reader task exited".to_string(),
                        data: None,
                    }));
                }
            }
        });

        // Spawn stdin writer task (Inline implementation)
        let _stdin_writer_handle = task::spawn(async move {
            let mut writer = BufWriter::with_capacity(STDIO_BUFFER_SIZE, _actual_stdin);
            loop {
                tokio::select! {
                    // Use biased select to prioritize checking messages first, then shutdown/sleep
                    biased;

                    Some(message) = _stdin_rx.recv() => {
                        debug!("Stdin({}): Received message string for sending ({} bytes): {}", server_name_stdin, message.len(), message);
                        let message_with_header = format!(
                            "Content-Length: {}\\r\\n\\r\\n{}",
                            message.len(),
                            message
                        );
                         debug!("Stdin({}): Formatted message with header ({} bytes): {}", server_name_stdin, message_with_header.len(), message_with_header.replace("\r\n", "<CRLF>")); // Log CRLF clearly
                        match writer.write_all(message_with_header.as_bytes()).await {
                           Ok(_) => {
                                debug!("Stdin({}): Successfully wrote message bytes to buffer.", server_name_stdin);
                                match writer.flush().await {
                                    Ok(_) => {
                                        debug!("Stdin({}): Successfully flushed buffer to process stdin.", server_name_stdin);
                                    }
                                    Err(e) => {
                                        error!("Stdin({}): Error flushing stdin: {}", server_name_stdin, e);
                                        // Attempt to close stdin_rx to signal upstream errors?
                                        _stdin_rx.close(); // Close the receiver side
                                        break; // Exit task on flush error
                                    }
                                }
                           }
                           Err(e) => {
                                error!("Stdin({}): Error writing to stdin buffer: {}", server_name_stdin, e);
                                // Attempt to close stdin_rx to signal upstream errors?
                                _stdin_rx.close(); // Close the receiver side
                                break; // Exit task on write error
                           }
                        }
                    }

                    // Check shutdown or wait if no message is ready
                    _ = tokio::time::sleep(Duration::from_millis(100)), if shutdown_for_stdin.load(Ordering::SeqCst) => {
                        info!("Stdin({}): Shutdown signal received, writer task shutting down.", server_name_stdin);
                        break;
                    }
                    else => {
                        // Channel closed (likely means request/notification handlers exited)
                        info!("Stdin({}): Input channel closed, writer task exiting.", server_name_stdin);
                        break; // Exit loop if channel is closed
                    }
                }
            }
            // Try a final flush on exit? Might error if pipe is broken.
            if let Err(e) = writer.flush().await {
                warn!(
                    "Stdin({}): Error during final flush on exit: {}",
                    server_name_stdin, e
                );
            }
            info!("Stdin({}): Writer task finished.", server_name_stdin);
        });

        // Spawn request handler task
        let _stdin_tx_req = _stdin_tx.clone();
        let pending_requests_for_request = _pending_requests.clone();
        let server_name_req = server_name.clone(); // Clone for request handler
        task::spawn(async move {
            while let Some((request, responder)) = _request_rx.recv().await {
                if shutdown_for_request.load(Ordering::SeqCst) {
                    let _ = responder.send(Err(JsonRpcError {
                        code: -32099,
                        message: "Server shutdown in progress".to_string(),
                        data: None,
                    }));
                    continue;
                }

                // Extract request ID
                let request_id = match &request.id {
                    Some(serde_json::Value::Number(n)) => n.as_u64(),
                    _ => None,
                };

                match serde_json::to_string(&request) {
                    Ok(request_json) => {
                        // Store in pending requests if we have an ID
                        if let Some(id) = request_id {
                            pending_requests_for_request.lock().await.insert(
                                id,
                                PendingRequest {
                                    responder,
                                    method: request.method.clone(),
                                },
                            );

                            debug!(
                                "ReqHandler({}): Sending request ID {} ('{}') to stdin writer.",
                                server_name_req, id, request.method
                            );
                            if let Err(e) = _stdin_tx_req.send(request_json).await {
                                error!("ReqHandler({}): Failed to send request ID {} to stdin writer: {}", server_name_req, id, e);
                                // Remove from pending and send error
                                if let Some(pending) =
                                    pending_requests_for_request.lock().await.remove(&id)
                                {
                                    let _ = pending.responder.send(Err(JsonRpcError {
                                        code: -32000,
                                        message: format!("Failed to send request: {}", e),
                                        data: None,
                                    }));
                                }
                            }
                        } else {
                            // No ID, treat as notification-style request
                            warn!("ReqHandler({}): Request has no ID (method: '{}'). Treating as notification.", server_name_req, request.method);
                            debug!("ReqHandler({}): Sending notification-style request ('{}') to stdin writer.", server_name_req, request.method);
                            if let Err(e) = _stdin_tx_req.send(request_json).await {
                                error!(
                                    "ReqHandler({}): Failed to send notification-style request: {}",
                                    server_name_req, e
                                );
                                let _ = responder.send(Err(JsonRpcError {
                                    code: -32000,
                                    message: format!("Failed to send request: {}", e),
                                    data: None,
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize request: {}", e);
                        let _ = responder.send(Err(JsonRpcError {
                            code: -32700,
                            message: format!("Failed to serialize request: {}", e),
                            data: None,
                        }));
                    }
                }
            }
        });

        // Spawn notification handler task
        let _stdin_tx_notif = _stdin_tx.clone();
        let server_name_notif = server_name.clone(); // Clone for notification handler
        task::spawn(async move {
            while let Some(notification) = _notification_rx.recv().await {
                if shutdown_for_notification.load(Ordering::SeqCst) {
                    debug!(
                        "NotifHandler({}): Shutdown in progress, dropping notification: {:?}",
                        server_name_notif, notification.method
                    );
                    continue;
                }

                match serde_json::to_string(&notification) {
                    Ok(notification_json) => {
                        debug!(
                            "NotifHandler({}): Sending notification ('{}') to stdin writer.",
                            server_name_notif, notification.method
                        );
                        if let Err(e) = _stdin_tx_notif.send(notification_json).await {
                            error!("NotifHandler({}): Failed to send notification ('{}') to stdin writer: {}", server_name_notif, notification.method, e);
                            // Maybe break here if the channel is broken?
                        }
                    }
                    Err(e) => {
                        error!(
                            "NotifHandler({}): Failed to serialize notification ('{}'): {}",
                            server_name_notif, notification.method, e
                        );
                    }
                }
            }
            info!(
                "NotifHandler({}): Notification handler task exiting.",
                server_name_notif
            );
        });

        // Create the server instance
        let server_name_for_init = server_name.clone(); // Clone for initialize logging
        let server = ActiveServer {
            config,
            capabilities: _capabilities.clone(),
            request_tx: _request_tx,
            notification_tx: _notification_tx,
            process: _process_arc,
            shutdown: _shutdown,
        };

        // Send initialize request
        let init_request_id = _next_request_id.fetch_add(1, Ordering::Relaxed);
        let (init_tx, init_rx) = oneshot::channel::<Result<Response, JsonRpcError>>();

        // Store in pending requests
        _pending_requests.lock().await.insert(
            init_request_id,
            PendingRequest {
                responder: init_tx,
                method: "initialize".to_string(),
            },
        );

        // Create initialize request
        let init_request = Request {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::Number(serde_json::Number::from(
                init_request_id,
            ))),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "clientInfo": {
                    "name": "gemini-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };

        // Send request
        let init_request_json = serde_json::to_string(&init_request)
            .map_err(|e| format!("Failed to serialize initialize request: {}", e))?;

        debug!(
            "Launch({}): Sending initialize request (ID {}) to stdin writer channel.",
            server_name_for_init, init_request_id
        );
        if let Err(e) = _stdin_tx.send(init_request_json).await {
            error!(
                "Launch({}): Failed to send initialize request (ID {}) to stdin writer channel: {}",
                server_name_for_init, init_request_id, e
            );
            // Clean up pending request if sending failed immediately
            if let Some(pending) = _pending_requests.lock().await.remove(&init_request_id) {
                let _ = pending.responder.send(Err(JsonRpcError {
                    code: -32005, // Custom code for init send failure
                    message: format!("Failed to queue initialize request: {}", e),
                    data: None,
                }));
            }
            return Err(format!("Failed to send initialize request: {}", e));
        }
        info!(
            "Launch({}): Initialize request (ID {}) queued for sending.",
            server_name_for_init, init_request_id
        );

        // Set up timeout for initialization
        let init_timeout = Duration::from_secs(
            std::env::var("GEMINI_MCP_TIMEOUT")
                .unwrap_or("10".to_string())
                .parse::<u64>()
                .unwrap_or(120),
        ); // Extend timeout to 120 seconds for slower servers
        info!(
            "Setting up initialization timeout of {}s for server '{}'",
            init_timeout.as_secs(),
            server_name
        );

        let server_name_clone = server_name.clone();
        let init_future = Box::pin(tokio::time::timeout(init_timeout, async move {
            debug!(
                "Waiting for initialization response from server '{}'",
                server_name_clone
            );
            match init_rx.await {
                Ok(res) => {
                    debug!(
                        "Received initialization response from '{}': {:?}",
                        server_name_clone, res
                    );
                    res.map(|_| ()) // Convert Response -> ()
                }
                Err(e) => {
                    error!(
                        "Failed to receive initialization response from '{}': {}",
                        server_name_clone, e
                    );
                    Err(JsonRpcError {
                        code: -32603,
                        message: format!("Failed to receive init response: {}", e),
                        data: None,
                    })
                }
            }
        })) as InitFuture;

        Ok((server, init_future))
    }

    // Create a new server with server-sent events transport
    pub(crate) async fn launch_sse(
        _next_request_id: &Arc<std::sync::atomic::AtomicU64>,
        config: McpServerConfig,
        _url: String,
        _headers: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(Self, InitFuture), String> {
        let server_name = config.name.clone();
        info!("Launching MCP server (SSE): {} at {}", server_name, _url);

        // Since we need to run the server via stdio instead, we'll convert the config
        // to use local execution but remember it was originally SSE
        if config.command.is_empty() {
            return Err(format!(
                "Server '{}': Empty command, cannot launch local SSE server",
                server_name
            ));
        }

        info!(
            "Note: Using stdio transport for SSE server '{}' as a fallback",
            server_name
        );

        // Now we follow the same implementation as the stdio version

        // Set up command with executable and arguments
        let mut command_parts = config.command.iter();
        let executable = command_parts.next().unwrap(); // Safe due to is_empty check
        let mut cmd = Command::new(executable);
        cmd.args(command_parts);
        cmd.args(&config.args);
        cmd.envs(&config.env);

        // Configure stdin, stdout, stderr pipes
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn the process
        let mut process = cmd
            .spawn()
            .map_err(|e| format!("Server '{}': Failed to spawn: {}", server_name, e))?;

        // Get stdin/stdout/stderr handles
        let _actual_stdin = process
            .stdin
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stdin", server_name))?;
        let _actual_stdout = process
            .stdout
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stdout", server_name))?;
        let _actual_stderr = process
            .stderr
            .take()
            .ok_or_else(|| format!("Server '{}': Failed to get stderr", server_name))?;

        // Create channels for communication
        let (_request_tx, mut _request_rx): (
            mpsc::Sender<(Request, oneshot::Sender<Result<Response, JsonRpcError>>)>,
            _,
        ) = mpsc::channel(CHANNEL_BUFFER_SIZE);
        let (_notification_tx, mut _notification_rx): (mpsc::Sender<Notification>, _) =
            mpsc::channel(CHANNEL_BUFFER_SIZE);
        let (_stdin_tx, mut _stdin_rx): (mpsc::Sender<String>, _) =
            mpsc::channel(CHANNEL_BUFFER_SIZE);

        // Create shared state
        let _capabilities = Arc::new(Mutex::new(None::<ServerCapabilities>));
        let _pending_requests = Arc::new(Mutex::new(HashMap::<u64, PendingRequest>::new()));
        let _process_arc = Arc::new(Mutex::new(Some(process)));
        let _shutdown = Arc::new(AtomicBool::new(false));

        // Clone for tasks
        let shutdown_for_request = _shutdown.clone();
        let shutdown_for_notification = _shutdown.clone();
        let shutdown_for_stdin = _shutdown.clone();
        let server_name_stderr = server_name.clone();
        let server_name_stdin = server_name.clone();
        let server_name_stdout = server_name.clone();
        let pending_requests_clone = _pending_requests.clone();
        let capabilities_clone = _capabilities.clone();

        // Spawn stderr handler task
        task::spawn(async move {
            let mut reader = BufReader::new(_actual_stderr);
            let mut line = String::new();
            loop {
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF
                        info!("MCP Server '{}' stderr closed", server_name_stderr);
                        break;
                    }
                    Ok(_) => {
                        warn!("[MCP stderr - {}]: {}", server_name_stderr, line.trim_end());
                        line.clear();
                    }
                    Err(e) => {
                        error!(
                            "Error reading MCP stderr for '{}': {}",
                            server_name_stderr, e
                        );
                        break;
                    }
                }
            }
        });

        // Spawn stdout reader task
        task::spawn(async move {
            let mut reader = BufReader::with_capacity(STDIO_BUFFER_SIZE, _actual_stdout);
            let mut buffer = Vec::with_capacity(JSON_RPC_PARSE_BUFFER_SIZE); // Use Vec for easier clearing and resizing

            loop {
                let mut content_length: Option<usize> = None;
                buffer.clear(); // Clear buffer for headers

                // Read headers until empty line
                debug!("Stdout({}): Waiting for headers...", server_name_stdout);
                loop {
                    let start_len = buffer.len(); // Track buffer growth
                    match reader.read_until(b'\n', &mut buffer).await {
                        Ok(0) => {
                            // EOF
                            info!(
                                "Stdout({}): Stream closed (EOF received while reading headers).",
                                server_name_stdout
                            );
                            // Clean up pending requests on EOF
                            let mut requests = pending_requests_clone.lock().await;
                            for (_, pending) in requests.drain() {
                                let _ = pending.responder.send(Err(JsonRpcError {
                                    code: -32001, // Custom code for unexpected close
                                    message: "Connection closed unexpectedly while reading headers"
                                        .to_string(),
                                    data: None,
                                }));
                            }
                            return;
                        }
                        Ok(bytes_read) => {
                            let line_bytes = &buffer[start_len..];
                            let line = String::from_utf8_lossy(line_bytes);
                            let line_trimmed = line.trim_end(); // Trim \r\n or \n
                            debug!(
                                "Stdout({}): Read header line ({} bytes): '{}'",
                                server_name_stdout, bytes_read, line_trimmed
                            );

                            if line_trimmed.is_empty() {
                                // End of headers
                                debug!("Stdout({}): Empty header line received, proceeding to read content.", server_name_stdout);
                                break;
                            }

                            // Parse Content-Length, case-insensitive
                            if line_trimmed
                                .to_ascii_lowercase()
                                .starts_with("content-length:")
                            {
                                if let Some(len_str) = line_trimmed.split(':').nth(1) {
                                    if let Ok(len) = len_str.trim().parse::<usize>() {
                                        debug!(
                                            "Stdout({}): Parsed Content-Length: {}",
                                            server_name_stdout, len
                                        );
                                        content_length = Some(len);
                                    } else {
                                        warn!("Stdout({}): Failed to parse Content-Length value: '{}'", server_name_stdout, len_str.trim());
                                    }
                                } else {
                                    warn!(
                                        "Stdout({}): Malformed Content-Length line: '{}'",
                                        server_name_stdout, line_trimmed
                                    );
                                }
                            }
                            // Note: LSP spec allows other headers, we just ignore them
                        }
                        Err(e) => {
                            error!(
                                "Stdout({}): Error reading headers: {}",
                                server_name_stdout, e
                            );
                            // Clean up pending requests on error
                            let mut requests = pending_requests_clone.lock().await;
                            for (_, pending) in requests.drain() {
                                let _ = pending.responder.send(Err(JsonRpcError {
                                    code: -32002, // Custom code for read error
                                    message: format!("Error reading stdout headers: {}", e),
                                    data: None,
                                }));
                            }
                            return;
                        }
                    }
                } // End header reading loop

                // Read the content based on content-length
                if let Some(length) = content_length {
                    if length == 0 {
                        warn!(
                            "Stdout({}): Received Content-Length: 0, skipping content read.",
                            server_name_stdout
                        );
                        continue; // Nothing to read, wait for next message
                    }

                    let mut content = vec![0; length];
                    debug!(
                        "Stdout({}): Attempting to read {} bytes of content...",
                        server_name_stdout, length
                    );
                    match reader.read_exact(&mut content).await {
                        Ok(_) => {
                            let json_str_result = String::from_utf8(content);
                            match json_str_result {
                                Ok(json_str) => {
                                    debug!(
                                        "Stdout({}): Received content ({} bytes): {}",
                                        server_name_stdout, length, json_str
                                    );

                                    // Process the message
                                    match serde_json::from_str::<serde_json::Value>(&json_str) {
                                        Ok(json_value) => {
                                            // Check if it's a Response or Request/Notification (though servers shouldn't send requests)
                                            if json_value.get("id").is_some()
                                                && (json_value.get("result").is_some()
                                                    || json_value.get("error").is_some())
                                            {
                                                match serde_json::from_value::<Response>(json_value)
                                                {
                                                    Ok(response) => {
                                                        // Handle response
                                                        let request_id = match &response.id {
                                                            serde_json::Value::Number(n) => {
                                                                n.as_u64()
                                                            }
                                                            _ => None, // Ignore responses with non-numeric IDs for now
                                                        };

                                                        if let Some(id) = request_id {
                                                            // Remove from pending requests and send response to requester
                                                            if let Some(pending) =
                                                                pending_requests_clone
                                                                    .lock()
                                                                    .await
                                                                    .remove(&id)
                                                            {
                                                                debug!("Stdout({}): Matched response ID {} to pending request '{}'", server_name_stdout, id, pending.method);
                                                                if pending.method == "initialize" {
                                                                    // If initialize, update capabilities
                                                                    if let Ok(result) =
                                                                        &response.result()
                                                                    {
                                                                        match serde_json::from_value::<
                                                                            ServerInitializeResult,
                                                                        >(
                                                                            result.clone()
                                                                        ) {
                                                                            Ok(init_result) => {
                                                                                debug!("Stdout({}): Received capabilities: {:?}", server_name_stdout, init_result.capabilities);
                                                                                *capabilities_clone.lock().await = Some(init_result.capabilities);
                                                                            }
                                                                            Err(e) => {
                                                                                error!("Stdout({}): Failed to deserialize InitializeResult from {:?}: {}", server_name_stdout, result, e);
                                                                            }
                                                                        }
                                                                    } else if let Some(err) =
                                                                        &response.error
                                                                    {
                                                                        error!("Stdout({}): Initialize request failed with error: {:?}", server_name_stdout, err);
                                                                    }
                                                                }

                                                                // Send response back
                                                                debug!("Stdout({}): Sending response for ID {} back to requester.", server_name_stdout, id);
                                                                if let Err(_) = pending
                                                                    .responder
                                                                    .send(Ok(response))
                                                                {
                                                                    warn!("Stdout({}): Failed to send response for ID {} back to requester (receiver dropped).", server_name_stdout, id);
                                                                }
                                                            } else {
                                                                warn!("Stdout({}): Received response for unknown or timed-out request ID: {}", server_name_stdout, id);
                                                            }
                                                        } else {
                                                            warn!("Stdout({}): Received response with non-numeric or null ID: {:?}", server_name_stdout, response.id);
                                                        }
                                                    }
                                                    Err(e) => {
                                                        error!("Stdout({}): Failed to parse valid JSON as Response: {}. JSON: {}", server_name_stdout, e, json_str);
                                                    }
                                                }
                                            } else if json_value.get("method").is_some() {
                                                // Handle potential notifications or requests from server (e.g. logging)
                                                warn!("Stdout({}): Received notification/request from server (unexpected): {}", server_name_stdout, json_str);
                                                // TODO: Implement notification handling from server if needed
                                            } else {
                                                error!("Stdout({}): Received JSON is not a recognizable RPC message: {}", server_name_stdout, json_str);
                                            }
                                        }
                                        Err(e) => {
                                            error!("Stdout({}): Failed to parse received content as JSON: {}. Content: '{}'", server_name_stdout, e, json_str);
                                        }
                                    }
                                }
                                Err(e) => {
                                    // Use from_utf8_lossy for logging if original conversion failed
                                    let lossy_content = String::from_utf8_lossy(e.as_bytes());
                                    error!("Stdout({}): Invalid UTF-8 received ({} bytes): {}. Lossy representation: '{}'", server_name_stdout, length, e, lossy_content);
                                }
                            }
                        }
                        Err(e) => {
                            error!(
                                "Stdout({}): Error reading content (expected {} bytes): {}",
                                server_name_stdout, length, e
                            );
                            if e.kind() == ErrorKind::UnexpectedEof {
                                // EOF during content read - server probably crashed or closed stream prematurely
                                info!(
                                    "Stdout({}): EOF encountered while reading message content.",
                                    server_name_stdout
                                );
                                // Clean up pending requests
                                let mut requests = pending_requests_clone.lock().await;
                                for (_, pending) in requests.drain() {
                                    let _ = pending.responder.send(Err(JsonRpcError {
                                        code: -32003, // Custom code for unexpected close during content read
                                        message: "Connection closed unexpectedly while reading message content".to_string(),
                                        data: None,
                                    }));
                                }
                                break; // Exit the loop
                            }
                            // For other errors, we might be able to recover, but it's risky. Let's log and continue waiting for headers.
                            // Consider adding logic to break if errors persist.
                        }
                    }
                } else {
                    warn!("Stdout({}): No Content-Length header found after reading headers. Raw buffer: '{}'", server_name_stdout, String::from_utf8_lossy(&buffer));
                    // If no content-length, we might be out of sync. Try reading headers again.
                }
            } // End main loop

            info!("Stdout({}): Reader task exiting.", server_name_stdout);
            // Final cleanup in case the loop exited cleanly but requests were still pending (shouldn't happen with proper shutdown)
            let mut requests = pending_requests_clone.lock().await;
            if !requests.is_empty() {
                warn!(
                    "Stdout({}): Reader task exiting with {} pending requests.",
                    server_name_stdout,
                    requests.len()
                );
                for (_, pending) in requests.drain() {
                    let _ = pending.responder.send(Err(JsonRpcError {
                        code: -32004, // Custom code for exit with pending requests
                        message: "Stdout reader task exited".to_string(),
                        data: None,
                    }));
                }
            }
        });

        // Spawn stdin writer task (Inline implementation)
        let _stdin_writer_handle = task::spawn(async move {
            let mut writer = BufWriter::with_capacity(STDIO_BUFFER_SIZE, _actual_stdin);
            loop {
                tokio::select! {
                    // Use biased select to prioritize checking messages first, then shutdown/sleep
                    biased;

                    Some(message) = _stdin_rx.recv() => {
                        debug!("Stdin({}): Received message string for sending ({} bytes): {}", server_name_stdin, message.len(), message);
                        let message_with_header = format!(
                            "Content-Length: {}\\r\\n\\r\\n{}",
                            message.len(),
                            message
                        );
                         debug!("Stdin({}): Formatted message with header ({} bytes): {}", server_name_stdin, message_with_header.len(), message_with_header.replace("\r\n", "<CRLF>")); // Log CRLF clearly
                        match writer.write_all(message_with_header.as_bytes()).await {
                           Ok(_) => {
                                debug!("Stdin({}): Successfully wrote message bytes to buffer.", server_name_stdin);
                                match writer.flush().await {
                                    Ok(_) => {
                                        debug!("Stdin({}): Successfully flushed buffer to process stdin.", server_name_stdin);
                                    }
                                    Err(e) => {
                                        error!("Stdin({}): Error flushing stdin: {}", server_name_stdin, e);
                                        // Attempt to close stdin_rx to signal upstream errors?
                                        _stdin_rx.close(); // Close the receiver side
                                        break; // Exit task on flush error
                                    }
                                }
                           }
                           Err(e) => {
                                error!("Stdin({}): Error writing to stdin buffer: {}", server_name_stdin, e);
                                // Attempt to close stdin_rx to signal upstream errors?
                                _stdin_rx.close(); // Close the receiver side
                                break; // Exit task on write error
                           }
                        }
                    }

                    // Check shutdown or wait if no message is ready
                    _ = tokio::time::sleep(Duration::from_millis(100)), if shutdown_for_stdin.load(Ordering::SeqCst) => {
                        info!("Stdin({}): Shutdown signal received, writer task shutting down.", server_name_stdin);
                        break;
                    }
                    else => {
                        // Channel closed (likely means request/notification handlers exited)
                        info!("Stdin({}): Input channel closed, writer task exiting.", server_name_stdin);
                        break; // Exit loop if channel is closed
                    }
                }
            }
            // Try a final flush on exit? Might error if pipe is broken.
            if let Err(e) = writer.flush().await {
                warn!(
                    "Stdin({}): Error during final flush on exit: {}",
                    server_name_stdin, e
                );
            }
            info!("Stdin({}): Writer task finished.", server_name_stdin);
        });

        // Spawn request handler task
        let _stdin_tx_req = _stdin_tx.clone();
        let pending_requests_for_request = _pending_requests.clone();
        let server_name_req = server_name.clone(); // Clone for request handler
        task::spawn(async move {
            while let Some((request, responder)) = _request_rx.recv().await {
                if shutdown_for_request.load(Ordering::SeqCst) {
                    let _ = responder.send(Err(JsonRpcError {
                        code: -32099,
                        message: "Server shutdown in progress".to_string(),
                        data: None,
                    }));
                    continue;
                }

                // Extract request ID
                let request_id = match &request.id {
                    Some(serde_json::Value::Number(n)) => n.as_u64(),
                    _ => None,
                };

                match serde_json::to_string(&request) {
                    Ok(request_json) => {
                        // Store in pending requests if we have an ID
                        if let Some(id) = request_id {
                            pending_requests_for_request.lock().await.insert(
                                id,
                                PendingRequest {
                                    responder,
                                    method: request.method.clone(),
                                },
                            );

                            debug!(
                                "ReqHandler({}): Sending request ID {} ('{}') to stdin writer.",
                                server_name_req, id, request.method
                            );
                            if let Err(e) = _stdin_tx_req.send(request_json).await {
                                error!("ReqHandler({}): Failed to send request ID {} to stdin writer: {}", server_name_req, id, e);
                                // Remove from pending and send error
                                if let Some(pending) =
                                    pending_requests_for_request.lock().await.remove(&id)
                                {
                                    let _ = pending.responder.send(Err(JsonRpcError {
                                        code: -32000,
                                        message: format!("Failed to send request: {}", e),
                                        data: None,
                                    }));
                                }
                            }
                        } else {
                            // No ID, treat as notification-style request
                            warn!("ReqHandler({}): Request has no ID (method: '{}'). Treating as notification.", server_name_req, request.method);
                            debug!("ReqHandler({}): Sending notification-style request ('{}') to stdin writer.", server_name_req, request.method);
                            if let Err(e) = _stdin_tx_req.send(request_json).await {
                                error!(
                                    "ReqHandler({}): Failed to send notification-style request: {}",
                                    server_name_req, e
                                );
                                let _ = responder.send(Err(JsonRpcError {
                                    code: -32000,
                                    message: format!("Failed to send request: {}", e),
                                    data: None,
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to serialize request: {}", e);
                        let _ = responder.send(Err(JsonRpcError {
                            code: -32700,
                            message: format!("Failed to serialize request: {}", e),
                            data: None,
                        }));
                    }
                }
            }
        });

        // Spawn notification handler task
        let _stdin_tx_notif = _stdin_tx.clone();
        let server_name_notif = server_name.clone(); // Clone for notification handler
        task::spawn(async move {
            while let Some(notification) = _notification_rx.recv().await {
                if shutdown_for_notification.load(Ordering::SeqCst) {
                    debug!(
                        "NotifHandler({}): Shutdown in progress, dropping notification: {:?}",
                        server_name_notif, notification.method
                    );
                    continue;
                }

                match serde_json::to_string(&notification) {
                    Ok(notification_json) => {
                        debug!(
                            "NotifHandler({}): Sending notification ('{}') to stdin writer.",
                            server_name_notif, notification.method
                        );
                        if let Err(e) = _stdin_tx_notif.send(notification_json).await {
                            error!("NotifHandler({}): Failed to send notification ('{}') to stdin writer: {}", server_name_notif, notification.method, e);
                            // Maybe break here if the channel is broken?
                        }
                    }
                    Err(e) => {
                        error!(
                            "NotifHandler({}): Failed to serialize notification ('{}'): {}",
                            server_name_notif, notification.method, e
                        );
                    }
                }
            }
            info!(
                "NotifHandler({}): Notification handler task exiting.",
                server_name_notif
            );
        });

        // Create the server instance
        let server_name_for_init = server_name.clone(); // Clone for initialize logging
        let server = ActiveServer {
            config,
            capabilities: _capabilities.clone(),
            request_tx: _request_tx,
            notification_tx: _notification_tx,
            process: _process_arc,
            shutdown: _shutdown,
        };

        // Send initialize request
        let init_request_id = _next_request_id.fetch_add(1, Ordering::Relaxed);
        let (init_tx, init_rx) = oneshot::channel::<Result<Response, JsonRpcError>>();

        // Store in pending requests
        _pending_requests.lock().await.insert(
            init_request_id,
            PendingRequest {
                responder: init_tx,
                method: "initialize".to_string(),
            },
        );

        // Create initialize request
        let init_request = Request {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::Number(serde_json::Number::from(
                init_request_id,
            ))),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "clientInfo": {
                    "name": "gemini-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        };

        // Send request
        let init_request_json = serde_json::to_string(&init_request)
            .map_err(|e| format!("Failed to serialize initialize request: {}", e))?;

        debug!(
            "Launch({}): Sending initialize request (ID {}) to stdin writer channel.",
            server_name_for_init, init_request_id
        );
        if let Err(e) = _stdin_tx.send(init_request_json).await {
            error!(
                "Launch({}): Failed to send initialize request (ID {}) to stdin writer channel: {}",
                server_name_for_init, init_request_id, e
            );
            // Clean up pending request if sending failed immediately
            if let Some(pending) = _pending_requests.lock().await.remove(&init_request_id) {
                let _ = pending.responder.send(Err(JsonRpcError {
                    code: -32005, // Custom code for init send failure
                    message: format!("Failed to queue initialize request: {}", e),
                    data: None,
                }));
            }
            return Err(format!("Failed to send initialize request: {}", e));
        }
        info!(
            "Launch({}): Initialize request (ID {}) queued for sending.",
            server_name_for_init, init_request_id
        );

        // Set up timeout for initialization
        let init_timeout = Duration::from_secs(120); // Extend timeout to 120 seconds for slower servers
        info!(
            "Setting up initialization timeout of {}s for server '{}'",
            init_timeout.as_secs(),
            server_name
        );

        let server_name_clone = server_name.clone();
        let init_future = Box::pin(tokio::time::timeout(init_timeout, async move {
            debug!(
                "Waiting for initialization response from server '{}'",
                server_name_clone
            );
            match init_rx.await {
                Ok(res) => {
                    debug!(
                        "Received initialization response from '{}': {:?}",
                        server_name_clone, res
                    );
                    res.map(|_| ()) // Convert Response -> ()
                }
                Err(e) => {
                    error!(
                        "Failed to receive initialization response from '{}': {}",
                        server_name_clone, e
                    );
                    Err(JsonRpcError {
                        code: -32603,
                        message: format!("Failed to receive init response: {}", e),
                        data: None,
                    })
                }
            }
        })) as InitFuture;

        Ok((server, init_future))
    }

    // Create a new server with WebSocket transport
    pub(crate) async fn launch_websocket(
        _next_request_id: &Arc<std::sync::atomic::AtomicU64>,
        config: McpServerConfig,
        _url: String,
        _headers: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(Self, InitFuture), String> {
        let server_name = config.name.clone();
        info!(
            "Launching MCP server (WebSocket): {} at {}",
            server_name, _url
        );

        // For now, just return an error instructing to use stdio directly
        Err(format!(
            "For WebSocket server '{}': Please use Stdio transport directly instead of WebSocket",
            server_name
        ))
    }

    // Send a request to the server and wait for a response
    pub(crate) async fn send_request(&self, request: Request) -> Result<Response, JsonRpcError> {
        let (resp_tx, resp_rx) = oneshot::channel();

        // Send the request to the handler task
        self.request_tx
            .send((request, resp_tx))
            .await
            .map_err(|_| JsonRpcError {
                code: -32603, // Internal error
                message: "Failed to send request to server".to_string(),
                data: None,
            })?;

        // Wait for the response
        resp_rx.await.map_err(|_| JsonRpcError {
            code: -32603, // Internal error
            message: "Failed to receive response from server".to_string(),
            data: None,
        })?
    }

    // Send a notification to the server (no response expected)
    pub(crate) async fn send_notification(&self, notification: Notification) -> Result<(), String> {
        self.notification_tx
            .send(notification)
            .await
            .map_err(|_| "Failed to send notification to server".to_string())
    }

    // Set the shutdown flag to interrupt any blocked tasks
    pub(crate) async fn set_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    // Take ownership of the process (for shutdown)
    pub(crate) async fn take_process(&self) -> Option<tokio::process::Child> {
        self.process.lock().await.take()
    }
}

// Initialize result structure
#[derive(serde::Deserialize, Debug)]
struct ServerInitializeResult {
    capabilities: ServerCapabilities,
}
