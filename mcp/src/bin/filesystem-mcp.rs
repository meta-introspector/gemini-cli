use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::signal;

// JSON-RPC 2.0 structures
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

// MCP specific types
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

// --- Helper function to read a single JSON-RPC message ---
async fn read_message(
    reader: &mut BufReader<tokio::io::Stdin>,
) -> Result<Option<String>, Box<dyn Error + Send + Sync>> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();

    // Read headers line by line
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Ok(None); // EOF
        }

        if line.to_lowercase().starts_with("content-length:") {
            let len_str = line
                .trim_start_matches("content-length:")
                .trim_start_matches("Content-Length:")
                .trim();
            content_length = Some(len_str.parse()?);
        }

        // Empty line signals the end of headers
        if line.trim().is_empty() {
            break;
        }
    }

    // Read content based on Content-Length
    if let Some(len) = content_length {
        let mut content = vec![0; len];
        reader.read_exact(&mut content).await?;
        let content_str = String::from_utf8(content)?;
        Ok(Some(content_str))
    } else {
        Ok(None) // No Content-Length header
    }
}

// --- Helper function to send a JSON-RPC response ---
async fn send_response(
    response: Response,
    writer: &mut BufWriter<tokio::io::Stdout>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let response_json = serde_json::to_string(&response)?;
    let headers = format!("Content-Length: {}\r\n\r\n", response_json.len());

    writer.write_all(headers.as_bytes()).await?;
    writer.write_all(response_json.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

// --- Functions for filesystem operations ---
fn list_directory(dir_path: &Path) -> Result<Vec<serde_json::Value>, Box<dyn Error + Send + Sync>> {
    let entries = fs::read_dir(dir_path)?;
    let mut file_list = Vec::new();

    for entry_result in entries {
        let entry = entry_result?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let is_dir = file_type.is_dir();
        let size = if file_type.is_file() {
            Some(fs::metadata(&path)?.len())
        } else {
            None
        };

        file_list.push(json!({
            "name": name,
            "path": path.to_string_lossy().to_string(),
            "is_directory": is_dir,
            "size": size
        }));
    }

    Ok(file_list)
}

fn read_file(
    file_path: &Path,
    max_size: Option<usize>,
) -> Result<String, Box<dyn Error + Send + Sync>> {
    let metadata = fs::metadata(file_path)?;
    if metadata.is_dir() {
        return Err(format!("Path is a directory, not a file: {}", file_path.display()).into());
    }

    let file_size = metadata.len() as usize;
    let size_limit = max_size.unwrap_or(file_size);

    if size_limit >= file_size {
        // Read the entire file
        let content = fs::read_to_string(file_path)?;
        Ok(content)
    } else {
        // Read only the specified amount
        let mut file = std::fs::File::open(file_path)?;
        let mut buffer = vec![0; size_limit];
        use std::io::Read;
        file.read_exact(&mut buffer)?;

        let content = String::from_utf8_lossy(&buffer).to_string();
        Ok(content)
    }
}

fn write_file(
    file_path: &Path,
    content: &str,
    create_dirs: bool,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    if create_dirs {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
    }

    fs::write(file_path, content)?;
    Ok(())
}

fn delete_file(file_path: &Path) -> Result<(), Box<dyn Error + Send + Sync>> {
    let metadata = fs::metadata(file_path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(file_path)?;
    } else {
        fs::remove_file(file_path)?;
    }
    Ok(())
}

// --- Handler for tool execute requests ---
async fn handle_tool_execute(params: Value) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let tool_name = params["name"]
        .as_str()
        .ok_or("Missing 'name' field in params")?;
    let arguments = params["arguments"].clone();

    match tool_name {
        "list_directory" => {
            let path_str = arguments["path"]
                .as_str()
                .ok_or("Missing 'path' field in arguments")?;
            let path = Path::new(path_str);

            let file_list = list_directory(path)?;
            Ok(json!({
                "files": file_list
            }))
        }
        "read_file" => {
            let path_str = arguments["path"]
                .as_str()
                .ok_or("Missing 'path' field in arguments")?;
            let path = Path::new(path_str);

            let max_size = arguments["max_size"].as_u64().map(|s| s as usize);
            let content = read_file(path, max_size)?;

            Ok(json!({
                "content": content
            }))
        }
        "write_file" => {
            let path_str = arguments["path"]
                .as_str()
                .ok_or("Missing 'path' field in arguments")?;
            let path = Path::new(path_str);

            let content = arguments["content"]
                .as_str()
                .ok_or("Missing 'content' field in arguments")?;

            let create_dirs = arguments["create_dirs"].as_bool().unwrap_or(false);

            write_file(path, content, create_dirs)?;

            Ok(json!({
                "success": true
            }))
        }
        "delete_file" => {
            let path_str = arguments["path"]
                .as_str()
                .ok_or("Missing 'path' field in arguments")?;
            let path = Path::new(path_str);

            delete_file(path)?;

            Ok(json!({
                "success": true
            }))
        }
        "get_current_dir" => {
            let current_dir = std::env::current_dir()?;
            Ok(json!({
                "path": current_dir.to_string_lossy().to_string()
            }))
        }
        _ => Err(format!("Unknown tool: {}", tool_name).into()),
    }
}

// --- Function to handle all incoming requests ---
async fn handle_request(
    request: Request,
    writer: &mut BufWriter<tokio::io::Stdout>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let id = request.id.clone().unwrap_or(json!(null));

    match request.method.as_str() {
        "initialize" => {
            info!("Handling initialize request");

            let server_info = ServerInfo {
                name: "filesystem-mcp".to_string(),
                version: "1.0.0".to_string(),
            };

            let tools = vec![
                Tool {
                    name: "list_directory".to_string(),
                    description: "Lists files in a directory".to_string(),
                    schema: None,
                },
                Tool {
                    name: "read_file".to_string(),
                    description: "Reads the content of a file".to_string(),
                    schema: None,
                },
                Tool {
                    name: "write_file".to_string(),
                    description: "Writes content to a file".to_string(),
                    schema: None,
                },
                Tool {
                    name: "delete_file".to_string(),
                    description: "Deletes a file or directory".to_string(),
                    schema: None,
                },
                Tool {
                    name: "get_current_dir".to_string(),
                    description: "Gets the current working directory".to_string(),
                    schema: None,
                },
            ];

            let capabilities = ServerCapabilities {
                tools,
                resources: vec![],
            };

            let response = Response {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(json!({
                    "serverInfo": server_info,
                    "capabilities": capabilities
                })),
                error: None,
            };

            send_response(response, writer).await?;
        }
        "mcp/tool/execute" => {
            info!("Handling mcp/tool/execute request");

            let params = request.params.clone().unwrap_or(json!({}));

            match handle_tool_execute(params).await {
                Ok(result) => {
                    let response = Response {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: Some(result),
                        error: None,
                    };
                    send_response(response, writer).await?;
                }
                Err(e) => {
                    let error_response = Response {
                        jsonrpc: "2.0".to_string(),
                        id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32000, // Server error
                            message: e.to_string(),
                            data: None,
                        }),
                    };
                    send_response(error_response, writer).await?;
                }
            }
        }
        _ => {
            // Method not found
            let error_response = Response {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601, // Method not found
                    message: format!("Method not found: {}", request.method),
                    data: None,
                }),
            };
            send_response(error_response, writer).await?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Initialize logging
    env_logger::init();

    info!("Starting filesystem MCP server...");

    let mut stdout = BufWriter::new(tokio::io::stdout());
    let mut stdin = BufReader::new(tokio::io::stdin());

    // Send early init response to avoid timeout
    let server_info = ServerInfo {
        name: "filesystem-mcp".to_string(),
        version: "1.0.0".to_string(),
    };

    let tools = vec![
        Tool {
            name: "list_directory".to_string(),
            description: "Lists files in a directory".to_string(),
            schema: None,
        },
        Tool {
            name: "read_file".to_string(),
            description: "Reads the content of a file".to_string(),
            schema: None,
        },
        Tool {
            name: "write_file".to_string(),
            description: "Writes content to a file".to_string(),
            schema: None,
        },
        Tool {
            name: "delete_file".to_string(),
            description: "Deletes a file or directory".to_string(),
            schema: None,
        },
        Tool {
            name: "get_current_dir".to_string(),
            description: "Gets the current working directory".to_string(),
            schema: None,
        },
    ];

    let capabilities = ServerCapabilities {
        tools,
        resources: vec![],
    };

    let early_response = Response {
        jsonrpc: "2.0".to_string(),
        id: json!(3), // Hardcoded ID for early response
        result: Some(json!({
            "capabilities": capabilities,
            "serverInfo": server_info,
            "status": "initialized"
        })),
        error: None,
    };

    send_response(early_response, &mut stdout).await?;
    info!("Sent early initialization response");

    // Main message processing loop
    let mut shutdown_requested = false;

    loop {
        if shutdown_requested {
            break;
        }

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("Received Ctrl+C signal. Shutting down...");
                break;
            }
            result = read_message(&mut stdin) => {
                match result {
                    Ok(Some(json_str)) => {
                        debug!("Received message: {}", json_str);
                        match serde_json::from_str::<Request>(&json_str) {
                            Ok(request) => {
                                // Check for special control methods
                                match request.method.as_str() {
                                    "exit" => {
                                        info!("Received exit notification. Exiting immediately.");
                                        break;
                                    }
                                    "shutdown" => {
                                        info!("Received shutdown request. Will exit after responding.");
                                        let response = Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: request.id.clone().unwrap_or(json!(null)),
                                            result: Some(json!(null)),
                                            error: None,
                                        };

                                        if let Err(e) = send_response(response, &mut stdout).await {
                                            error!("Failed to send shutdown response: {}", e);
                                            break;
                                        }

                                        shutdown_requested = true;
                                        continue;
                                    }
                                    _ => {
                                        // Handle regular requests
                                        if let Err(e) = handle_request(request, &mut stdout).await {
                                            error!("Error handling request: {}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse JSON-RPC request: {}", e);
                                let error_response = Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: json!(null),
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
                        error!("No Content-Length header found or EOF.");
                        break;
                    }
                    Err(e) => {
                        error!("Error reading message: {}", e);
                        break;
                    }
                }
            }
        }
    }

    info!("Filesystem MCP server shutting down.");
    Ok(())
}
