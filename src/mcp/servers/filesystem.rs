use std::io::{self, BufRead, Read, Write};
use serde_json::json;
use std::process;
use std::error::Error;
use dirs; // Added for home_directory resource
use crate::mcp::rpc::{Request, Response, JsonRpcError, InitializeResult, ServerInfo, ServerCapabilities, Tool, Resource}; // Added import

// Define basic JSON-RPC structures - TODO: Move to shared rpc module

// MCP server capabilities - TODO: Move to shared rpc module


/// Run the application as a filesystem MCP server
pub async fn run() -> Result<(), Box<dyn Error>> {
    println!("Starting filesystem MCP server...");

    // Process standard input
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();

    let mut buffer = Vec::new();
    let mut content_length: Option<usize> = None;

    // Main processing loop
    loop {
        // Read headers until we find a blank line
        let mut line = String::new();
        match stdin_lock.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    // End of headers, read the content
                    if let Some(length) = content_length {
                        buffer.resize(length, 0);
                        if let Err(e) = stdin_lock.read_exact(&mut buffer) {
                            eprintln!("Failed to read message content: {}", e);
                            break;
                        }

                        // Process the message
                        let json_str = String::from_utf8_lossy(&buffer);

                        // Try to parse as a Request
                        match serde_json::from_str::<Request>(&json_str) {
                            Ok(request) => {
                                match request.method.as_str() {
                                    "initialize" => {
                                        // Define server capabilities
                                        let server_info = ServerInfo {
                                            name: "filesystem-mcp".to_string(),
                                            version: "1.0.0".to_string(),
                                        };

                                        // Define tools
                                        let tools = vec![
                                            Tool {
                                                name: "list_directory".to_string(),
                                                description: Some("Lists contents of a directory".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the directory to list"
                                                        },
                                                        "recursive": {
                                                            "type": "boolean",
                                                            "description": "Whether to list subdirectories recursively",
                                                            "default": false
                                                        }
                                                    },
                                                    "required": ["path"]
                                                })),
                                            },
                                            Tool {
                                                name: "read_file".to_string(),
                                                description: Some("Reads content of a file".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the file to read"
                                                        }
                                                    },
                                                    "required": ["path"]
                                                })),
                                            },
                                        ];

                                        // Define resources
                                        let resources = vec![
                                            Resource {
                                                name: "current_directory".to_string(),
                                                description: Some("Gets the current working directory".to_string()),
                                            },
                                            Resource {
                                                name: "home_directory".to_string(),
                                                description: Some("Gets the user's home directory".to_string()),
                                            },
                                        ];

                                        let capabilities = ServerCapabilities { tools, resources };
                                        let result = InitializeResult { server_info, capabilities };

                                        // Send response
                                        let response = Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: request.id.unwrap_or(json!(null)),
                                            result: Some(json!(result)),
                                            error: None,
                                        };

                                        let response_json = serde_json::to_string(&response).unwrap();

                                        // Send with correct headers
                                        let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                        stdout.write_all(header.as_bytes()).unwrap();
                                        stdout.write_all(response_json.as_bytes()).unwrap();
                                        stdout.flush().unwrap();
                                    },
                                    "mcp/tool/execute" => {
                                        // Handle tool execution request
                                        if let Some(params) = request.params {
                                            // Expected params format: {name: string, args: object}
                                            let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                            // Store arguments in a variable to avoid temporary value being dropped
                                            let arguments = params.get("args").cloned().unwrap_or(json!({}));

                                            eprintln!("Executing tool: '{}' with args: {:?}", tool_name, arguments);

                                            // For MCP server, the tool_name should be "list_directory" (without server prefix)
                                            match tool_name {
                                                "list_directory" => {
                                                    // List directory implementation
                                                    let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                                                    let _recursive = arguments.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);
                                                    // Using _recursive to avoid the unused variable warning

                                                    match std::fs::read_dir(path) {
                                                        Ok(entries) => {
                                                            let mut files = Vec::new();

                                                            for entry in entries {
                                                                if let Ok(entry) = entry {
                                                                    // Simplified file type handling - just unwrap without a fallback
                                                                    if let Ok(file_type) = entry.file_type() {
                                                                        let name = entry.file_name().to_string_lossy().to_string();
                                                                        let path = entry.path().to_string_lossy().to_string();

                                                                        files.push(json!({
                                                                            "name": name,
                                                                            "path": path,
                                                                            "is_dir": file_type.is_dir(),
                                                                            "is_file": file_type.is_file(),
                                                                        }));
                                                                    }

                                                                    // TODO: Implement recursive listing if needed
                                                                }
                                                            }

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "result": {
                                                                        "path": path,
                                                                        "files": files
                                                                    }
                                                                })),
                                                                error: None,
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        Err(e) => {
                                                            // Error response
                                                            let error = JsonRpcError {
                                                                code: -32000,
                                                                message: format!("Failed to list directory: {}", e),
                                                                data: None,
                                                            };

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: None,
                                                                error: Some(error),
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        }
                                                    }
                                                },
                                                "read_file" => {
                                                    // Read file implementation
                                                    // Check for both "path" and "filename" parameters
                                                    let path = arguments.get("path")
                                                        .or_else(|| arguments.get("filename"))
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("");

                                                    match std::fs::read_to_string(path) {
                                                        Ok(content) => {
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "result": {
                                                                        "path": path,
                                                                        "content": content
                                                                    }
                                                                })),
                                                                error: None,
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        Err(e) => {
                                                            // Error response
                                                            let error = JsonRpcError {
                                                                code: -32000,
                                                                message: format!("Failed to read file: {}", e),
                                                                data: None,
                                                            };

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: None,
                                                                error: Some(error),
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        }
                                                    }
                                                },
                                                _ => {
                                                    // Unknown tool
                                                    let error = JsonRpcError {
                                                        code: -32601,
                                                        message: format!("Unknown tool: {}", tool_name),
                                                        data: None,
                                                    };

                                                    let response = Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: None,
                                                        error: Some(error),
                                                    };

                                                    let response_json = serde_json::to_string(&response).unwrap();

                                                    let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                    stdout.write_all(header.as_bytes()).unwrap();
                                                    stdout.write_all(response_json.as_bytes()).unwrap();
                                                    stdout.flush().unwrap();
                                                }
                                            }
                                        } else {
                                            // Missing parameters
                                            let error = JsonRpcError {
                                                code: -32602,
                                                message: "Missing parameters for tool execution".to_string(),
                                                data: None,
                                            };

                                            let response = Response {
                                                jsonrpc: "2.0".to_string(),
                                                id: request.id.unwrap_or(json!(null)),
                                                result: None,
                                                error: Some(error),
                                            };

                                            let response_json = serde_json::to_string(&response).unwrap();

                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                            stdout.write_all(header.as_bytes()).unwrap();
                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                            stdout.flush().unwrap();
                                        }
                                    },
                                    "shutdown" => {
                                        // Just acknowledge shutdown
                                        let response = Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: request.id.unwrap_or(json!(null)),
                                            result: Some(json!({})),
                                            error: None,
                                        };

                                        let response_json = serde_json::to_string(&response).unwrap();

                                        // Send with correct headers
                                        let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                        stdout.write_all(header.as_bytes()).unwrap();
                                        stdout.write_all(response_json.as_bytes()).unwrap();
                                        stdout.flush().unwrap();
                                    },
                                    "resource/get" => {
                                        // Handle resource get request
                                        if let Some(params) = request.params {
                                            // Expected params format: {name: string, params?: object}
                                            let resource_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

                                            match resource_name {
                                                "current_directory" => {
                                                    // Get current directory
                                                    match std::env::current_dir() {
                                                        Ok(path) => {
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "path": path.to_string_lossy().to_string()
                                                                })),
                                                                error: None,
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        Err(e) => {
                                                            // Error response
                                                            let error = JsonRpcError {
                                                                code: -32000,
                                                                message: format!("Failed to get current directory: {}", e),
                                                                data: None,
                                                            };

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: None,
                                                                error: Some(error),
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        }
                                                    }
                                                },
                                                "home_directory" => {
                                                    // Get home directory
                                                    match dirs::home_dir() {
                                                        Some(path) => {
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "path": path.to_string_lossy().to_string()
                                                                })),
                                                                error: None,
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        None => {
                                                            // Error response
                                                            let error = JsonRpcError {
                                                                code: -32000,
                                                                message: "Failed to get home directory".to_string(),
                                                                data: None,
                                                            };

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: None,
                                                                error: Some(error),
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();

                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        }
                                                    }
                                                },
                                                _ => {
                                                    // Unknown resource
                                                    let error = JsonRpcError {
                                                        code: -32601,
                                                        message: format!("Unknown resource: {}", resource_name),
                                                        data: None,
                                                    };

                                                    let response = Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: None,
                                                        error: Some(error),
                                                    };

                                                    let response_json = serde_json::to_string(&response).unwrap();

                                                    let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                    stdout.write_all(header.as_bytes()).unwrap();
                                                    stdout.write_all(response_json.as_bytes()).unwrap();
                                                    stdout.flush().unwrap();
                                                }
                                            }
                                        } else {
                                            // Missing parameters
                                            let error = JsonRpcError {
                                                code: -32602,
                                                message: "Missing parameters for resource get".to_string(),
                                                data: None,
                                            };

                                            let response = Response {
                                                jsonrpc: "2.0".to_string(),
                                                id: request.id.unwrap_or(json!(null)),
                                                result: None,
                                                error: Some(error),
                                            };

                                            let response_json = serde_json::to_string(&response).unwrap();

                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                            stdout.write_all(header.as_bytes()).unwrap();
                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                            stdout.flush().unwrap();
                                        }
                                    },
                                    "exit" => {
                                        // Exit the process
                                        process::exit(0);
                                    },
                                    _ => {
                                        // Method not found
                                        let error = JsonRpcError {
                                            code: -32601,
                                            message: format!("Method not found: {}", request.method),
                                            data: None,
                                        };

                                        let response = Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: request.id.unwrap_or(json!(null)),
                                            result: None,
                                            error: Some(error),
                                        };

                                        let response_json = serde_json::to_string(&response).unwrap();

                                        // Send with correct headers
                                        let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                        stdout.write_all(header.as_bytes()).unwrap();
                                        stdout.write_all(response_json.as_bytes()).unwrap();
                                        stdout.flush().unwrap();
                                    }
                                }
                            },
                            Err(e) => {
                                // Parse error
                                let error = JsonRpcError {
                                    code: -32700,
                                    message: format!("Parse error: {}", e),
                                    data: None,
                                };

                                let response = Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: json!(null),
                                    result: None,
                                    error: Some(error),
                                };

                                let response_json = serde_json::to_string(&response).unwrap();

                                // Send with correct headers
                                let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                stdout.write_all(header.as_bytes()).unwrap();
                                stdout.write_all(response_json.as_bytes()).unwrap();
                                stdout.flush().unwrap();
                            }
                        }

                        // Reset for next message
                        content_length = None;
                        buffer.clear();
                    }
                } else if line.starts_with("Content-Length:") {
                    let parts: Vec<&str> = line.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        match parts[1].trim().parse::<usize>() {
                            Ok(len) => content_length = Some(len),
                            Err(e) => eprintln!("Invalid Content-Length: {}", e),
                        }
                    }
                }
            },
            Err(e) => {
                eprintln!("Failed to read line: {}", e);
                break;
            }
        }
    }

    Ok(())
} 