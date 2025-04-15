use crate::mcp::host::types::PendingRequest;
use crate::mcp::rpc::{self, JsonRpcError, Message, Notification, Response, ServerCapabilities, LogMessageParams};
use log::{debug, error, info, warn};
use serde_json::{self, Value};
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
                        handle_response(server_name, response, capabilities.clone(), pending_requests.clone()).await;
                    }
                    Ok(Message::Notification(notification)) => {
                        handle_notification(server_name, notification).await;
                    }
                    Ok(Message::Request(request)) => {
                        warn!("Received unexpected request from server '{}': {:?}", server_name, request);
                        // TODO: Handle server-initiated requests if MCP spec allows/requires (e.g., Sampling)
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

// Handles a response message from the server
pub async fn handle_response(
    server_name: &str,
    response: Response,
    capabilities: Arc<Mutex<Option<ServerCapabilities>>>,
    pending_requests: Arc<Mutex<HashMap<u64, PendingRequest>>>,
) {
    // Attempt to parse the response ID as u64
    let request_id_opt: Option<u64> = match &response.id {
        Value::Number(n) => n.as_u64(),
        // Allow string IDs if they parse to u64, otherwise ignore
        Value::String(s) => s.parse::<u64>().ok(),
        _ => None, // Ignore null or other types
    };

    // Only proceed if we have a valid u64 ID
    let request_id = match request_id_opt {
        Some(id) => id,
        None => {
            error!(
                "Received response with invalid or missing ID from '{}'. Ignoring.",
                server_name
            );
            return;
        }
    };

    let pending = pending_requests.lock().await.remove(&request_id);

    if let Some(pending_request) = pending {
        // Check if this was the Initialize response
        if pending_request.method == "initialize" {
            match response.result() {
                Ok(result) => {
                    match serde_json::from_value::<rpc::InitializeResult>(result.clone()) {
                        Ok(init_result) => {
                            info!(
                                "Received capabilities from server '{}': {:?}",
                                server_name, init_result.capabilities
                            );
                            *capabilities.lock().await = Some(init_result.capabilities);
                            // Send success back to the initialization waiter
                            let _ = pending_request.responder.send(Ok(result)); // Send original result value
                        }
                        Err(e) => {
                            error!(
                                "Failed to deserialize initialize result from '{}': {}. Value: {:?}",
                                server_name, e, result
                            );
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
    info!(
        "Received notification '{}' from server '{}': params={:?}",
        notification.method, server_name, notification.params
    );
    // TODO: Implement handling for specific notifications if needed (e.g., $/progress, logMessage)
    match notification.method.as_str() {
        "window/logMessage" => { // Example standard LSP/MCP notification
            if let Some(params) = notification.params {
                if let Ok(log_params) = serde_json::from_value::<LogMessageParams>(params) {
                    // Map type to log level (example)
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
        }
        "$/cancelRequest" => {
            warn!("Received unsupported cancelRequest from {}", server_name);
            // TODO: Implement request cancellation if needed
        }
        // Handle other notifications like $/progress, etc.
        _ => {
            debug!("Unhandled notification '{}' from {}", notification.method, server_name);
        }
    }
} 