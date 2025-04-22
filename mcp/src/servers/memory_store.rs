// use std::io::{}; // Removed unused import
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error;
use std::time::{Duration, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::signal;

// Import necessary types from sibling modules or core

// Import memory crate
use gemini_memory::MemoryStore;

// JSON-RPC 2.0 structures (Local definitions)
#[derive(Serialize, Deserialize, Debug)]
struct Request {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Response {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize, Deserialize, Debug)]
struct JsonRpcError {
    code: i64,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

// MCP specific types (Local definitions)
#[derive(Serialize, Deserialize, Debug)]
struct InitializeResult {
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
    capabilities: ServerCapabilities,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerCapabilities {
    tools: Vec<Tool>,
    resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Tool {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Resource {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

/// Run the application as a memory store MCP server
pub async fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("Starting memory-store MCP server...");

    let mut stdout = BufWriter::new(tokio::io::stdout()); // Async stdout

    // Create memory store with default configuration
    let memory_store = MemoryStore::new(None, None, None).await?; // Use ? for error handling

    // Process standard input using Tokio
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut buffer = Vec::with_capacity(4096); // Reusable buffer for content
    let mut line_buffer = String::new(); // Reusable buffer for reading lines

    info!("Memory store server ready. Waiting for messages...");

    // Special handling for initialization
    // Try to read the raw message first to see what's getting sent
    info!("Attempting to read initialize request");

    // Send immediate initialization response with hardcoded ID 3 to avoid timeout
    // This is similar to how filesystem and command servers handle initialization
    info!("Sending early init response");

    let server_info = ServerInfo {
        name: "memory-store-mcp".to_string(),
        version: "1.0.0".to_string(),
    };

    let tools = vec![
        Tool {
            name: "store_memory".to_string(),
            description: "Stores a memory item".to_string(),
            schema: None,
        },
        Tool {
            name: "retrieve_memory_by_key".to_string(),
            description: "Retrieves a memory by key".to_string(),
            schema: None,
        },
        Tool {
            name: "retrieve_memory_by_tag".to_string(),
            description: "Retrieves memories by tag".to_string(),
            schema: None,
        },
        Tool {
            name: "list_all_memories".to_string(),
            description: "Lists all memories".to_string(),
            schema: None,
        },
        Tool {
            name: "delete_memory_by_key".to_string(),
            description: "Deletes a memory by key".to_string(),
            schema: None,
        },
    ];

    let capabilities = ServerCapabilities {
        tools,
        resources: vec![],
    };

    let early_response = Response {
        jsonrpc: "2.0".to_string(),
        id: json!(3), // Hardcoded ID 3 based on logs
        result: Some(json!({
            "capabilities": capabilities,
            "serverInfo": server_info,
            "status": "initialized"
        })),
        error: None,
    };

    // Send the early response immediately
    debug!("Sending early init response");
    match send_response(early_response, &mut stdout).await {
        Ok(_) => info!("Early init response sent."),
        Err(e) => {
            error!("Failed to send early init response: {}", e);
            return Err(e.into());
        }
    }

    // Now we can try to read the initialize message without pressure of timeout
    let initialize_result = read_message(&mut reader, &mut line_buffer, &mut buffer).await;

    match initialize_result {
        Ok(Some(json_str)) => {
            debug!("Received first message: {}", json_str);
            // We don't need to send another response since we've already sent the early one
            // Just verify it's an initialize request for logging purposes
            match serde_json::from_str::<Request>(&json_str) {
                Ok(request) => {
                    if request.method == "initialize" {
                        debug!("Confirmed initialize request with ID: {:?}", request.id);
                    } else {
                        warn!(
                            "First message was not an initialize request: {}",
                            request.method
                        );
                    }
                }
                Err(e) => {
                    warn!("Could not parse initial message as Request: {}", e);
                }
            }
        }
        Ok(None) => {
            debug!("Received empty message or EOF during initialization");
        }
        Err(e) => {
            warn!("Error reading initialization message: {:?}", e);
        }
    }

    // Main message processing loop
    let mut shutdown_requested = false;

    loop {
        if shutdown_requested {
            info!("Shutdown was requested, exiting main loop.");
            break;
        }

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received Ctrl+C signal. Shutting down memory-store MCP server...");
                break;
            }
            result = read_message(&mut reader, &mut line_buffer, &mut buffer) => {
                match result {
                    Ok(Some(json_str)) => {
                        debug!("Received message: {}", json_str);
                        match serde_json::from_str::<Request>(&json_str) {
                            Ok(request) => {
                                // First check for special control methods
                                match request.method.as_str() {
                                    "exit" => {
                                        info!("Received exit notification. Exiting immediately.");
                                        // Exit without responding
                                        break;
                                    }
                                    "shutdown" => {
                                        info!("Received shutdown request (ID: {:?}). Will exit after responding.", request.id.clone().unwrap_or(json!(null)));
                                        // Send response before exiting
                                        let response = Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: request.id.clone().unwrap_or(json!(null)),
                                            result: Some(json!(null)), // Use null result
                                            error: None,
                                        };

                                        if let Err(e) = send_response(response, &mut stdout).await {
                                            error!("Failed to send shutdown response: {}", e);
                                            break; // Exit on error
                                        }

                                        info!("Shutdown response sent successfully.");
                                        // Set flag to exit after this iteration
                                        shutdown_requested = true;
                                        continue; // Continue to next loop iteration to check the flag
                                    }
                                    _ => {
                                        // Handle regular requests
                                        if let Err(e) = handle_request(request, &memory_store, &mut stdout).await {
                                            error!("Error handling request: {}", e);
                                            // Optionally send a generic error response back to host
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse JSON-RPC request: {}", e);
                                let error_response = Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: json!(null), // No ID available for parse errors
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32700, // Parse error
                                        message: format!("Parse error: {}", e),
                                        data: None,
                                    }),
                                };
                                if let Err(e) = send_response(error_response, &mut stdout).await {
                                    error!("Failed to send parse error response: {}", e);
                                }
                            }
                        }
                    }
                    Ok(None) => {
                         error!("No Content-Length header found or invalid headers.");
                         let error_response = Response {
                            jsonrpc: "2.0".to_string(),
                            id: json!(null),
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32700, // Using Parse Error code for header issues
                                message: "Invalid headers or missing Content-Length".to_string(),
                                data: None,
                            }),
                        };
                        if let Err(e) = send_response(error_response, &mut stdout).await {
                            error!("Failed to send header error response: {}", e);
                        }
                    }
                    Err(ReadMessageError::Io(e)) => {
                        if e.kind() == std::io::ErrorKind::UnexpectedEof {
                             info!("Stdin closed (EOF). Exiting.");
                        } else {
                            error!("Error reading from stdin: {}", e);
                        }
                        break; // Exit on read errors or EOF
                    }
                     Err(ReadMessageError::InvalidContentLength(val)) => {
                         error!("Invalid Content-Length value received: '{}'", val);
                         break;
                     }
                }
            }
        }
    }

    info!("Memory-store MCP server shutting down.");
    // Perform any final cleanup here if needed
    Ok(())
}

// --- Helper enum for read_message errors ---
#[derive(Debug)]
enum ReadMessageError {
    Io(tokio::io::Error),
    InvalidContentLength(String),
}

impl From<tokio::io::Error> for ReadMessageError {
    fn from(err: tokio::io::Error) -> Self {
        ReadMessageError::Io(err)
    }
}

// --- Helper function to read a single JSON-RPC message ---
async fn read_message<'a>(
    reader: &'a mut BufReader<tokio::io::Stdin>,
    line_buffer: &'a mut String,
    content_buffer: &'a mut Vec<u8>,
) -> Result<Option<String>, ReadMessageError> {
    let mut content_length: Option<usize> = None;

    debug!("Starting to read headers");

    // Clear any existing content in buffers
    line_buffer.clear();
    content_buffer.clear();

    // Read headers line by line
    loop {
        line_buffer.clear();
        let bytes_read = reader.read_line(line_buffer).await?;

        if bytes_read == 0 {
            debug!("EOF while reading headers");

            // If we've already parsed a Content-Length but got EOF, it might be
            // that we've received all data already but there's no more data to read.
            // In this case, if content_length is Some, we should attempt to send the fallback response.
            if content_length.is_some() {
                debug!("Got EOF but already parsed Content-Length, treating as potential complete message");
                // Return empty content, caller will need to handle this specially
                return Ok(None);
            }

            return Err(ReadMessageError::Io(tokio::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "EOF reading headers",
            )));
        }

        debug!("Read header line (raw): '{}'", line_buffer.escape_debug());

        // Check if this is a Content-Length header
        if line_buffer.to_lowercase().starts_with("content-length:") {
            // Extract just the numeric part using a simple digit extraction approach
            let mut digits_only = String::new();
            let start_pos = "content-length:".len();

            // Extract only the digits after the header name
            for c in line_buffer[start_pos..].chars() {
                if c.is_ascii_digit() {
                    digits_only.push(c);
                } else if !c.is_whitespace() && !digits_only.is_empty() {
                    // Stop at first non-digit, non-whitespace after we've seen digits
                    break;
                }
            }

            debug!("Extracted Content-Length digits: '{}'", digits_only);

            // Parse the numeric part
            match digits_only.parse::<usize>() {
                Ok(len) => {
                    debug!("Successfully parsed Content-Length: {}", len);
                    content_length = Some(len);

                    // Handle the case where the header line contains both the header and the content
                    let header_and_content = line_buffer.as_str();
                    if let Some(pos) = header_and_content.find("\r\n\r\n") {
                        let content_part = &header_and_content[pos + 4..]; // +4 to skip both CRLFs
                        if !content_part.is_empty() {
                            debug!(
                                "Found content after headers in the same read: '{}'",
                                content_part.escape_debug()
                            );
                            // Use cmp for clearer comparison logic
                            match content_part.len().cmp(&len) {
                                std::cmp::Ordering::Equal => {
                                    debug!("Content length matches expected length, returning message");
                                    return Ok(Some(content_part.to_string()));
                                }
                                std::cmp::Ordering::Less => {
                                    // Partial content, but we can't read more if we're about to encounter EOF
                                    debug!("Partial content found, but might be all we have");
                                    return Ok(Some(content_part.to_string()));
                                }
                                std::cmp::Ordering::Greater => {
                                    // This case wasn't explicitly handled before, but seems unlikely
                                    // given the context. Log a warning and return what we have, truncated?
                                    // Or maybe error? For now, return what we have (as it matches Less behaviour)
                                    warn!("Content part length {} is greater than expected len {}, returning partial.", content_part.len(), len);
                                    return Ok(Some(content_part.to_string()));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Invalid Content-Length value '{}': {}", digits_only, e);
                    return Err(ReadMessageError::InvalidContentLength(digits_only));
                }
            }
        }

        // Check if we have a double CRLF sequence, which might indicate headers + content in one read
        if let Some(end_of_headers_pos) = line_buffer.find("\r\n\r\n") {
            // Found double CRLF - extract the content part
            let possible_content_part = &line_buffer[end_of_headers_pos + 4..]; // +4 to skip both CRLFs

            // If we have content in the same read, handle it
            if !possible_content_part.is_empty() {
                debug!(
                    "Found potential content in same read: '{}'",
                    possible_content_part.escape_debug()
                );

                if let Some(length) = content_length {
                    // We already know the content length from the header
                    // Use cmp for clearer comparison logic
                    match possible_content_part.len().cmp(&length) {
                        std::cmp::Ordering::Equal => {
                            // We have the exact full content
                            debug!("Content length matches, returning full message");
                            return Ok(Some(possible_content_part.to_string()));
                        }
                        std::cmp::Ordering::Less => {
                            // We have partial content, read the rest
                            debug!(
                                "Partial content ({}/{}), reading the rest",
                                possible_content_part.len(),
                                length
                            );
                            content_buffer.clear();
                            content_buffer.extend_from_slice(possible_content_part.as_bytes());

                            // Read remaining bytes
                            let remaining_bytes = length - possible_content_part.len();
                            let mut remaining_buffer = vec![0; remaining_bytes];

                            // Try to read the remaining bytes, but handle EOF gracefully
                            match reader.read_exact(&mut remaining_buffer).await {
                                Ok(_) => {
                                    // Combine the content
                                    content_buffer.extend_from_slice(&remaining_buffer);
                                    match String::from_utf8(content_buffer.clone()) {
                                        Ok(json_str) => {
                                            debug!("Successfully read JSON content: {}", json_str);
                                            return Ok(Some(json_str));
                                        }
                                        Err(e) => {
                                            error!("Invalid UTF-8 in content: {}", e);
                                            return Err(ReadMessageError::Io(tokio::io::Error::new(
                                                std::io::ErrorKind::InvalidData,
                                                format!("Invalid UTF-8 in content: {}", e),
                                            )));
                                        }
                                    }
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                                    // Handle EOF by returning what we have so far
                                    debug!("EOF while reading content, returning partial content");
                                    match String::from_utf8(content_buffer.clone()) {
                                        Ok(json_str) => {
                                            return Ok(Some(json_str));
                                        }
                                        Err(e) => {
                                            error!("Invalid UTF-8 in partial content: {}", e);
                                            return Err(ReadMessageError::Io(tokio::io::Error::new(
                                                std::io::ErrorKind::InvalidData,
                                                format!("Invalid UTF-8 in content: {}", e),
                                            )));
                                        }
                                    }
                                }
                                Err(e) => {
                                    return Err(ReadMessageError::Io(e));
                                }
                            }
                        }
                        std::cmp::Ordering::Greater => {
                            // We have more data than expected, this is unusual
                            warn!(
                                "Received more data than expected Content-Length ({} > {})",
                                possible_content_part.len(),
                                length
                            );
                            // Just return the expected length
                            return Ok(Some(possible_content_part[..length].to_string()));
                        }
                    }
                }
            }

            // If we reach here, we've found the end of headers but don't have content yet
            debug!("Found end of headers marker, will read content separately");
            break;
        }

        // Empty line signals the end of headers (simple case)
        if line_buffer.trim().is_empty() {
            debug!("Empty line marks end of headers");
            break;
        }
    }

    // Read content if we have a content length
    if let Some(length) = content_length {
        if length == 0 {
            debug!("Content-Length is 0, no content to read");
            return Ok(None);
        }

        debug!("Attempting to read exactly {} bytes of content", length);
        content_buffer.resize(length, 0);

        // Read the exact number of bytes specified by Content-Length
        match reader.read_exact(content_buffer).await {
            Ok(_) => match String::from_utf8(content_buffer.clone()) {
                Ok(json_str) => {
                    debug!("Successfully read JSON content: {}", json_str);
                    Ok(Some(json_str))
                }
                Err(e) => {
                    error!("Invalid UTF-8 in content: {}", e);
                    Err(ReadMessageError::Io(tokio::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Invalid UTF-8 in content: {}", e),
                    )))
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // Handle EOF - return what we have, which might be empty
                debug!("EOF while reading content");
                if content_buffer.is_empty() {
                    return Ok(None);
                }
                match String::from_utf8(content_buffer.clone()) {
                    Ok(json_str) => {
                        debug!("Returning partial content from EOF: {}", json_str);
                        Ok(Some(json_str))
                    }
                    Err(e) => {
                        error!("Invalid UTF-8 in partial content: {}", e);
                        Err(ReadMessageError::Io(tokio::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Invalid UTF-8 in content: {}", e),
                        )))
                    }
                }
            }
            Err(e) => {
                error!("Failed to read {} bytes of content: {}", length, e);
                Err(ReadMessageError::Io(e))
            }
        }
    } else {
        error!("No Content-Length header found");
        Ok(None)
    }
}

// Helper function to send a JSON-RPC response (now async)
async fn send_response(
    response: Response,
    writer: &mut BufWriter<tokio::io::Stdout>,
) -> Result<(), tokio::io::Error> {
    // Serialize the response
    let json_str = match serde_json::to_string(&response) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to serialize response: {}", e);
            return Err(tokio::io::Error::new(
                tokio::io::ErrorKind::Other,
                format!("Serialization error: {}", e),
            ));
        }
    };

    // Format with proper Content-Length header
    let header = format!("Content-Length: {}\r\n\r\n", json_str.len());

    // Write header and content separately for better error handling
    debug!("Sending response: {}", json_str);
    debug!("Header written: {}", header);

    // Try to write header
    match writer.write_all(header.as_bytes()).await {
        Ok(_) => debug!("Header written successfully"),
        Err(e) => {
            error!("Failed to write header: {}", e);
            return Err(e);
        }
    }

    // Try to write content
    match writer.write_all(json_str.as_bytes()).await {
        Ok(_) => debug!("Content written successfully"),
        Err(e) => {
            error!("Failed to write content: {}", e);
            return Err(e);
        }
    }

    // Try to flush
    match writer.flush().await {
        Ok(_) => {
            debug!("Writer flushed successfully");
            Ok(())
        }
        Err(e) => {
            error!("Failed to flush writer: {}", e);
            Err(e)
        }
    }
}

// Handles incoming JSON-RPC requests (now async)
async fn handle_request(
    request: Request,
    memory_store: &MemoryStore,
    writer: &mut BufWriter<tokio::io::Stdout>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // Return Result for better error propagation
    let request_id = request.id.clone().unwrap_or(json!(null));
    debug!(
        "Handling request method: {} (ID: {:?})",
        request.method, request_id
    );

    let response_result: Result<Option<Value>, JsonRpcError> = match request.method.as_str() {
        "initialize" => {
            info!(
                "Received duplicate initialize request (ID: {:?}), already initialized",
                request_id
            );
            // Return same response as initial initialize in case this is a retry
            let server_info = ServerInfo {
                name: "memory-store-mcp".to_string(),
                version: "1.0.0".to_string(),
            };
            let tools = vec![
                Tool {
                    name: "store_memory".to_string(),
                    description: "Stores a memory item".to_string(),
                    schema: None,
                },
                Tool {
                    name: "retrieve_memory_by_key".to_string(),
                    description: "Retrieves a memory by key".to_string(),
                    schema: None,
                },
                Tool {
                    name: "retrieve_memory_by_tag".to_string(),
                    description: "Retrieves memories by tag".to_string(),
                    schema: None,
                },
                Tool {
                    name: "list_all_memories".to_string(),
                    description: "Lists all memories".to_string(),
                    schema: None,
                },
                Tool {
                    name: "delete_memory_by_key".to_string(),
                    description: "Deletes a memory by key".to_string(),
                    schema: None,
                },
            ];
            let capabilities = ServerCapabilities {
                tools,
                resources: vec![],
            };
            Ok(Some(json!({
                "capabilities": capabilities,
                "serverInfo": server_info,
                "status": "initialized"
            })))
        }
        // "shutdown" is now handled directly in the main loop
        // "exit" is also handled in the main loop
        "mcp/tool/execute" => {
            // Changed from "tool/execute" to match host logs
            debug!(
                "Received mcp/tool/execute request with params: {:?}",
                request.params
            );
            handle_tool_execute(request.params.unwrap_or_default(), memory_store)
                .await
                .map(Some) // Wrap successful result in Some
                .map_err(|err_msg| {
                    error!("Tool execution failed: {}", err_msg);
                    JsonRpcError {
                        code: -32000,
                        message: err_msg,
                        data: None,
                    } // Map String error to JsonRpcError
                })
        }
        _ => {
            warn!("Method not found: {}", request.method);
            Err(JsonRpcError {
                code: -32601, // Method not found
                message: format!("Method not found: {}", request.method),
                data: None,
            })
        }
    };

    // Construct and send the final response
    let response = match response_result {
        Ok(result_value) => Response {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            result: result_value, // This is already Option<Value>
            error: None,
        },
        Err(error_obj) => Response {
            jsonrpc: "2.0".to_string(),
            id: request_id,
            result: None,
            error: Some(error_obj),
        },
    };

    send_response(response, writer).await?;
    Ok(()) // Indicate successful handling of the request itself
}

// Handles the execution of specific tool methods (remains async)
async fn handle_tool_execute(params: Value, memory_store: &MemoryStore) -> Result<Value, String> {
    // Return String error
    // Use "name" for tool name and "args" for arguments as per MCP spec/host logs
    let tool_name = params["name"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'name' in tool/execute params".to_string())?;
    let arguments = params.get("args").cloned().unwrap_or(json!({})); // Get args, default to empty object

    debug!(
        "Executing tool: {} with arguments: {:?}",
        tool_name, arguments
    );

    match tool_name {
        "store_memory" => {
            // ... (rest of the tool handling logic remains largely the same) ...
            // Ensure all calls to memory_store use .await
            let key = arguments["key"]
                .as_str()
                .ok_or("Missing 'key' for store_memory")?;
            let value = arguments["value"]
                .as_str()
                .ok_or("Missing 'value' for store_memory")?;
            let tags = arguments["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let session_id = arguments["session_id"].as_str().map(String::from);
            let source = arguments["source"].as_str().map(String::from);
            let related_keys = arguments["related_keys"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

            debug!("Adding memory with key: {}, tags: {:?}", key, tags);
            memory_store
                .add_memory(key, value, tags, session_id, source, related_keys)
                .await
                .map_err(|e| format!("Failed to add memory: {}", e))?;
            Ok(json!({ "success": true }))
        }
        "update_memory" => {
            // Assuming this tool exists, keep implementation
            let key = arguments["key"]
                .as_str()
                .ok_or("Missing 'key' for update_memory")?;
            let value = arguments["value"]
                .as_str()
                .ok_or("Missing 'value' for update_memory")?;
            let tags = arguments["tags"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let session_id = arguments["session_id"].as_str().map(String::from);
            let source = arguments["source"].as_str().map(String::from);
            let related_keys = arguments["related_keys"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

            debug!("Updating memory with key: {}", key);
            let updated = memory_store
                .update_memory(key, value, tags, session_id, source, related_keys)
                .await
                .map_err(|e| format!("Failed to update memory: {}", e))?;
            Ok(json!({ "success": true, "updated_existing": updated }))
        }
        "delete_memory_by_key" => {
            let key = arguments["key"]
                .as_str()
                .ok_or("Missing 'key' for delete_memory_by_key")?;
            debug!("Deleting memory with key: {}", key);
            let count = memory_store
                .delete_by_key(key)
                .await
                .map_err(|e| format!("Failed to delete memory: {}", e))?;
            Ok(json!({ "success": count > 0, "deleted_count": count })) // Indicate success based on count
        }
        "retrieve_memory_by_key" => {
            let key = arguments["key"]
                .as_str()
                .ok_or("Missing 'key' for retrieve_memory_by_key")?;
            debug!("Getting memory by key: {}", key);
            let memory_opt = memory_store
                .get_by_key(key)
                .await
                .map_err(|e| format!("Failed to get memory by key: {}", e))?;
            Ok(serde_json::to_value(memory_opt).unwrap_or(json!(null))) // Return memory or null
        }
        "retrieve_memory_by_tag" => {
            let tag = arguments["tag"]
                .as_str()
                .ok_or("Missing 'tag' for retrieve_memory_by_tag")?;
            debug!("Getting memories by tag: {}", tag);
            let memories = memory_store
                .get_by_tag(tag)
                .await
                .map_err(|e| format!("Failed to get memories by tag: {}", e))?;
            debug!("Found {} memories with tag: {}", memories.len(), tag);
            Ok(json!(memories))
        }
        "list_all_memories" => {
            debug!("Getting all memories");
            let memories = memory_store
                .get_all_memories()
                .await
                .map_err(|e| format!("Failed to get all memories: {}", e))?;
            debug!("Found {} memories in total", memories.len());
            Ok(json!(memories))
        }
        "get_recent_memories" => {
            // Keep this if it's a valid, intended tool
            let duration_secs = arguments["duration_seconds"]
                .as_u64()
                .ok_or("Missing or invalid 'duration_seconds' for get_recent_memories")?;
            debug!("Getting memories from last {} seconds", duration_secs);
            let memories = memory_store
                .get_recent(Duration::from_secs(duration_secs))
                .await
                .map_err(|e| format!("Failed to get recent memories: {}", e))?;
            debug!("Found {} recent memories", memories.len());
            Ok(json!(memories))
        }
        "get_memories_in_range" => {
            // Keep this if valid
            let start_secs = arguments["start_timestamp_secs"]
                .as_u64()
                .ok_or("Missing or invalid 'start_timestamp_secs' for get_memories_in_range")?;
            let end_secs = arguments["end_timestamp_secs"]
                .as_u64()
                .ok_or("Missing or invalid 'end_timestamp_secs' for get_memories_in_range")?;
            debug!(
                "Getting memories in range from {} to {} seconds since epoch",
                start_secs, end_secs
            );
            let start_time = UNIX_EPOCH + Duration::from_secs(start_secs);
            let end_time = UNIX_EPOCH + Duration::from_secs(end_secs);
            if start_time > end_time {
                return Err("Start time cannot be after end time".to_string());
            }
            let memories = memory_store
                .get_in_range(start_time, end_time)
                .await
                .map_err(|e| format!("Failed to get memories in range: {}", e))?;
            debug!("Found {} memories in specified time range", memories.len());
            Ok(json!(memories))
        }
        "get_semantically_similar" => {
            // Keep this if valid
            let query_text = arguments["query_text"]
                .as_str()
                .ok_or("Missing 'query_text' for get_semantically_similar")?;
            let top_k = arguments["top_k"].as_u64().unwrap_or(5) as usize;
            let min_relevance_score =
                arguments["min_relevance_score"].as_f64().unwrap_or(0.0) as f32;
            debug!(
                "Searching for semantically similar memories to: '{}' (top_k: {}, min_score: {})",
                query_text, top_k, min_relevance_score
            );
            let memories_with_scores = memory_store
                .get_semantically_similar(query_text, top_k, min_relevance_score)
                .await
                .map_err(|e| format!("Failed to get semantically similar memories: {}", e))?;
            debug!(
                "Found {} semantically similar memories",
                memories_with_scores.len()
            );
            let result = memories_with_scores
                .into_iter()
                .map(|(memory, score)| {
                    let mut memory_json = serde_json::to_value(memory).unwrap_or(json!({}));
                    if let Value::Object(ref mut obj) = memory_json {
                        obj.insert("relevance_score".to_string(), json!(score));
                    }
                    memory_json
                })
                .collect::<Vec<Value>>();
            Ok(json!(result))
        }
        _ => {
            error!("Unknown tool requested via mcp/tool/execute: {}", tool_name);
            Err(format!("Tool not found: {}", tool_name))
        }
    }
}
