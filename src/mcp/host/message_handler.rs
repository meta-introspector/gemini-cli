use crate::mcp::host::types::PendingRequest;
use crate::mcp::rpc::{self, JsonRpcError, Message, Notification, Response, Request, ServerCapabilities, LogMessageParams, ProgressParams, CancelParams};
use log::{debug, error, info, warn};
use serde_json::{self, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::io::{AsyncRead, BufReader};

// Helper loop for reading from stdout and dispatching messages
pub async fn stdout_reader_loop<R: AsyncRead + Unpin>(
    server_name: &str,
    reader: &mut BufReader<R>,
    capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
    pending_requests: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) {
    let mut buffer = Vec::with_capacity(crate::mcp::host::types::JSON_RPC_PARSE_BUFFER_SIZE);
    loop {
        match crate::mcp::host::io::read_json_rpc_message(reader, &mut buffer).await {
            Ok(Some(json_payload)) => {
                debug!("Received from '{}': {}", server_name, json_payload);
                match serde_json::from_str::<Message>(&json_payload) {
                    Ok(Message::Response(response)) => {
                        // Debug logging for command results
                        if server_name == "command" && response.result.is_some() {
                            if let Some(result) = &response.result {
                                if let Some(stdout) = result.get("stdout") {
                                    info!("Command result stdout: {}", stdout);
                                }
                            }
                        }
                        
                        handle_response(server_name, response, capabilities.clone(), pending_requests.clone()).await;
                    }
                    Ok(Message::Notification(notification)) => {
                        handle_notification(server_name, notification).await;
                    }
                    Ok(Message::Request(request)) => {
                        handle_server_request(server_name, request).await;
                    }
                    Err(e) => {
                        error!("Error deserializing MCP message from '{}': {}. Payload: {}", server_name, e, json_payload);
                    }
                }
            }
            Ok(None) => {
                info!("MCP Server '{}' stdout closed (EOF).", server_name);
                break; // EOF
            }
            Err(e) => {
                error!("Error reading MCP message from '{}': {}", server_name, e);
                // Attempt to clean up pending requests with an error
                let mut requests = pending_requests.lock().await;
                for (_, pending) in requests.drain() {
                    let _ = pending.responder.send(Err(JsonRpcError {
                        code: -32000, // Generic server error
                        message: format!("Connection error: {}", e),
                        data: None,
                    }));
                }
                break; // Exit loop on read error
            }
        }
        buffer.clear(); // Clear buffer for next message
    }
    // Clean up remaining pending requests on exit (e.g., if server crashed without response)
    let mut requests = pending_requests.lock().await;
    if !requests.is_empty() {
        warn!("{} pending requests outstanding for server '{}' on reader exit.", requests.len(), server_name);
        for (_, pending) in requests.drain() {
            let _ = pending.responder.send(Err(JsonRpcError {
                code: -32001, // Internal error maybe?
                message: "Server connection closed unexpectedly".to_string(),
                data: None,
            }));
        }
    }
}

// Handles a server-initiated request
pub async fn handle_server_request(server_name: &str, request: Request) {
    info!("Received request '{}' from server '{}': params={:?}", request.method, server_name, request.params);
    
    // Handle different types of server-initiated requests
    match request.method.as_str() {
        "sampling/start" => {
            // Handle sampling start request (if implemented)
            info!("Server '{}' requested sampling start", server_name);
            // Implementation would depend on the sampling mechanics
            // For now, just log the request
        },
        _ => {
            warn!("Unsupported server request '{}' from '{}'", request.method, server_name);
            // We could send an error response back to the server here if needed
        }
    }
}

// Handles a response message from the server
pub async fn handle_response(
    server_name: &str,
    response: Response,
    capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
    pending_requests: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) {
    // Check if the response ID is null, which might be a notification-style response
    // that doesn't require tracking/handling (often the case with autoExecute responses)
    if response.id == Value::Null {
        debug!(
            "Received response with null ID from '{}'. This may be expected for auto-execute commands.",
            server_name
        );
        return;
    }

    // Attempt to parse the response ID as u64
    let request_id_opt: Option<u64> = match &response.id {
        Value::Number(n) => n.as_u64(),
        // Allow string IDs if they parse to u64, otherwise ignore
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None, // Ignore other types (should not happen due to null check above)
    };

    // Only proceed if we have a valid u64 ID
    let request_id = match request_id_opt {
        Some(id) => id,
        None => {
            error!(
                "Received response with invalid ID format from '{}': {:?}. Ignoring.",
                server_name, response.id
            );
            return;
        }
    };

    let pending = pending_requests.lock().await.remove(&request_id);

    if let Some(pending_request) = pending {
        // Check if this was the Initialize response
        if pending_request.method == "initialize" {
            debug!("[DEBUG handle_response] Received initialize response for server '{}', req_id={}", server_name, request_id);
            match response.result() {
                Ok(result) => {
                    debug!("[DEBUG handle_response] Initialize raw result value for '{}': {:?}", server_name, result);
                    match serde_json::from_value::<rpc::InitializeResult>(result.clone()) {
                        Ok(init_result) => {
                            info!(
                                "Received capabilities from server '{}': {:?}",
                                server_name, init_result.capabilities
                            );
                            // Log just before storing
                            debug!("[DEBUG handle_response] Storing capabilities for '{}': {:?}", server_name, init_result.capabilities);
                            *capabilities.lock().await = Some(init_result.capabilities);
                            debug!("[DEBUG handle_response] Capabilities potentially stored for '{}'", server_name);
                            // Send success back to the initialization waiter
                            let _ = pending_request.responder.send(Ok(result)); // Send original result value
                        }
                        Err(e) => {
                            error!(
                                "Failed to deserialize initialize result from '{}': {}. Value: {:?}",
                                server_name, e, result
                            );
                            debug!("[DEBUG handle_response] Clearing capabilities due to parse error for '{}'", server_name);
                            let _ = pending_request.responder.send(Err(JsonRpcError {
                                code: -32603, // Internal error
                                message: format!("Failed to parse initialize result: {}", e),
                                data: Some(result),
                            }));
                            // Clear capabilities on error? Or leave as None?
                            *capabilities.lock().await = None;
                        }
                    }
                }
                Err(error) => {
                    warn!("Received error for req id={} method='{}' from '{}': {:?}", request_id, pending_request.method, server_name, error);
                    debug!("[DEBUG handle_response] Clearing capabilities due to error response for '{}'", server_name);
                    let _ = pending_request.responder.send(Err(error));
                    // Clear capabilities on error
                    *capabilities.lock().await = None;
                }
            }
        } else {
            // Handle other successful responses
            match response.result() {
                Ok(result) => {
                    debug!("Received result for req id={} method='{}' from '{}'", request_id, pending_request.method, server_name);
                    let _ = pending_request.responder.send(Ok(result));
                }
                Err(error) => {
                    warn!("Received error for req id={} method='{}' from '{}': {:?}", request_id, pending_request.method, server_name, error);
                    let _ = pending_request.responder.send(Err(error));
                }
            }
        }
    } else {
        warn!(
            "Received response for unknown or timed-out request ID {} from '{}'. Ignoring.",
            request_id, server_name
        );
    }
}

// Handles a notification message from the server
pub async fn handle_notification(server_name: &str, notification: Notification) {
    info!("Received notification '{}' from server '{}': params={:?}", 
          notification.method, server_name, notification.params);
    
    match notification.method.as_str() {
        "window/logMessage" => { // Standard LSP/MCP notification
            if let Some(params) = notification.params {
                if let Ok(log_params) = serde_json::from_value::<LogMessageParams>(params) {
                    // Map type to log level
                    match log_params.type_ {
                        1 => error!("[MCP Log - {}]: {}", server_name, log_params.message),
                        2 => warn!("[MCP Log - {}]: {}", server_name, log_params.message),
                        3 => info!("[MCP Log - {}]: {}", server_name, log_params.message),
                        _ => debug!("[MCP Log - {}]: {}", server_name, log_params.message),
                    }
                } else {
                    warn!("Failed to parse logMessage params from {}", server_name);
                }
            }
        },
        "$/progress" => { // LSP/MCP progress notification
            if let Some(params) = notification.params {
                if let Ok(progress_params) = serde_json::from_value::<ProgressParams>(params) {
                    // Log the progress notification
                    info!("[MCP Progress - {}]: token={:?}, value={:?}", 
                          server_name, progress_params.token, progress_params.value);
                    
                    // Additional progress handling could be implemented here
                    // For example, updating a progress bar or notifying the user
                } else {
                    warn!("Failed to parse progress params from {}", server_name);
                }
            }
        },
        "$/cancelRequest" => {
            if let Some(params) = notification.params {
                if let Ok(cancel_params) = serde_json::from_value::<CancelParams>(params) {
                    // Log the cancellation request
                    info!("[MCP Cancel - {}]: Cancellation requested for id={:?}", 
                          server_name, cancel_params.id);
                    
                    // Convert the ID to a u64 if possible
                    let request_id_opt: Option<u64> = match &cancel_params.id {
                        Value::Number(n) => n.as_u64(),
                        Value::String(s) => s.parse::<u64>().ok(),
                        _ => None,
                    };
                    
                    if let Some(id) = request_id_opt {
                        // Here we would implement the actual cancellation logic
                        // This could involve setting a flag in an active request handler
                        // or stopping ongoing work for the specified request ID
                        warn!("Request cancellation for ID {} is not fully implemented yet", id);
                    } else {
                        warn!("Invalid request ID format in cancellation request: {:?}", cancel_params.id);
                    }
                } else {
                    warn!("Failed to parse cancelRequest params from {}", server_name);
                }
            }
        },
        _ => {
            debug!("Unhandled notification '{}' from {}", notification.method, server_name);
        }
    }
} 