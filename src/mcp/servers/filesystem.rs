use std::io::{self, BufRead, Read, Write};
use std::fs::{self, File};
use std::path::Path;
use serde_json::json;
use std::process;
use std::error::Error;
use dirs; // Added for home_directory resource
use crate::mcp::rpc::{Request, Response, JsonRpcError, InitializeResult, ServerInfo, ServerCapabilities, Tool, Resource}; // Added import
use diffy; // Use diffy for patching

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
                                                description: Some("Lists contents of a directory.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the directory to list."
                                                        },
                                                        "recursive": {
                                                            "type": "boolean",
                                                            "description": "Whether to list subdirectories recursively.",
                                                            "default": false
                                                        }
                                                    },
                                                    "required": ["path"]
                                                })),
                                            },
                                            Tool {
                                                name: "read_file".to_string(),
                                                description: Some("Reads content of a file, optionally a specific range of lines.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the file to read."
                                                        },
                                                        "start_line": {
                                                           "type": "integer",
                                                           "description": "The 1-indexed line number to start reading from (inclusive).",
                                                            "minimum": 1
                                                        },
                                                        "end_line": {
                                                           "type": "integer",
                                                           "description": "The 1-indexed line number to end reading at (inclusive).",
                                                            "minimum": 1
                                                        }
                                                    },
                                                    "required": ["path"]
                                                })),
                                            },
                                            Tool {
                                                name: "write_file".to_string(),
                                                description: Some("Writes content to a file, creating it if it doesn't exist or overwriting it if it does.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the file to write."
                                                        },
                                                        "content": {
                                                            "type": "string",
                                                            "description": "The content to write to the file."
                                                        }
                                                    },
                                                    "required": ["path", "content"]
                                                }))
                                            },
                                            Tool {
                                                name: "apply_patch".to_string(),
                                                description: Some("Applies a unified diff patch to a file.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the file to patch."
                                                        },
                                                        "patch_content": {
                                                            "type": "string",
                                                            "description": "The unified diff patch content."
                                                        }
                                                    },
                                                    "required": ["path", "patch_content"]
                                                }))
                                            },
                                            Tool {
                                                name: "create_directory".to_string(),
                                                description: Some("Creates a new directory.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the directory to create."
                                                        },
                                                         "create_parents": {
                                                            "type": "boolean",
                                                            "description": "Whether to create parent directories if they don't exist.",
                                                            "default": false
                                                        }
                                                    },
                                                    "required": ["path"]
                                                }))
                                            },
                                            Tool {
                                                name: "delete_file".to_string(),
                                                description: Some("Deletes a file.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the file to delete."
                                                        }
                                                    },
                                                    "required": ["path"]
                                                }))
                                            },
                                            Tool {
                                                name: "delete_directory".to_string(),
                                                description: Some("Deletes a directory.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the directory to delete."
                                                        },
                                                         "recursive": {
                                                            "type": "boolean",
                                                            "description": "Whether to delete recursively if the directory is not empty.",
                                                            "default": false
                                                        }
                                                    },
                                                    "required": ["path"]
                                                }))
                                            },
                                            Tool {
                                                name: "rename".to_string(),
                                                description: Some("Renames or moves a file or directory.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "from_path": {
                                                            "type": "string",
                                                            "description": "The original path."
                                                        },
                                                         "to_path": {
                                                            "type": "string",
                                                            "description": "The new path."
                                                        }
                                                    },
                                                    "required": ["from_path", "to_path"]
                                                }))
                                            },
                                             Tool {
                                                name: "get_metadata".to_string(),
                                                description: Some("Gets metadata for a file or directory.".to_string()),
                                                parameters: Some(json!({
                                                    "type": "object",
                                                    "properties": {
                                                        "path": {
                                                            "type": "string",
                                                            "description": "Path to the file or directory."
                                                        }
                                                    },
                                                    "required": ["path"]
                                                }))
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
                                        let header = format!("Content-Length: {}

", response_json.len());
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

                                            // For MCP server, the tool_name should be the base name (e.g., "list_directory")
                                            match tool_name {
                                                // --- Existing Tools ---
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
                                                                    let file_type = entry.file_type(); // Handle error below
                                                                    let name = entry.file_name().to_string_lossy().to_string();
                                                                    let path = entry.path().to_string_lossy().to_string();

                                                                    match file_type {
                                                                        Ok(ft) => {
                                                                            files.push(json!({
                                                                                "name": name,
                                                                                "path": path,
                                                                                "is_dir": ft.is_dir(),
                                                                                "is_file": ft.is_file(),
                                                                                "is_symlink": ft.is_symlink(),
                                                                            }));
                                                                        },
                                                                        Err(e) => {
                                                                            eprintln!("Could not get file type for {:?}: {}", entry.path(), e);
                                                                            // Optionally skip or add an error indicator
                                                                             files.push(json!({
                                                                                "name": name,
                                                                                "path": path,
                                                                                "error": format!("Could not get file type: {}", e),
                                                                            }));
                                                                        }
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

                                                            let header = format!("Content-Length: {}

", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        Err(e) => {
                                                            // Error response
                                                            let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to list directory '{}': {}", path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                "read_file" => {
                                                    // Read file implementation
                                                    let path = arguments.get("path")
                                                        .and_then(|v| v.as_str())
                                                        .unwrap_or("");
                                                    let start_line = arguments.get("start_line").and_then(|v| v.as_u64()).map(|v| v as usize);
                                                    let end_line = arguments.get("end_line").and_then(|v| v.as_u64()).map(|v| v as usize);

                                                     if path.is_empty() {
                                                        let error = JsonRpcError {
                                                            code: -32602, // Invalid params
                                                            message: "Missing or empty 'path' parameter".to_string(),
                                                            data: None,
                                                        };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue; // Use continue to proceed to the next message
                                                     }

                                                    match read_file_content(path, start_line, end_line) {
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
                                                            send_response(&mut stdout, &response);
                                                        },
                                                        Err(e) => {
                                                             let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to read file '{}': {}", path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                // --- New Tools ---
                                                "write_file" => {
                                                    let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                                    let content = arguments.get("content").and_then(|v| v.as_str()).unwrap_or("");

                                                    if path.is_empty() {
                                                        let error = JsonRpcError {
                                                            code: -32602, // Invalid params
                                                            message: "Missing or empty 'path' parameter".to_string(),
                                                            data: None,
                                                        };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }

                                                    match fs::write(path, content) {
                                                        Ok(_) => {
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({"result": {"path": path, "success": true}})),
                                                                error: None,
                                                            };
                                                            send_response(&mut stdout, &response);
                                                        },
                                                        Err(e) => {
                                                             let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to write file '{}': {}", path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                "apply_patch" => {
                                                    let path_str = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                                    let patch_content = arguments.get("patch_content").and_then(|v| v.as_str()).unwrap_or("");

                                                    if path_str.is_empty() {
                                                        let error = JsonRpcError { code: -32602, message: "Missing or empty 'path' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }
                                                    if patch_content.is_empty() {
                                                         let error = JsonRpcError { code: -32602, message: "Missing or empty 'patch_content' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }

                                                    let path = Path::new(path_str);

                                                    match apply_patch_to_file(path, patch_content) {
                                                        Ok(_) => {
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({"result": {"path": path_str, "success": true}})),
                                                                error: None,
                                                            };
                                                             send_response(&mut stdout, &response);
                                                        }
                                                        Err(e) => {
                                                            let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to apply patch to '{}': {}", path_str, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                "create_directory" => {
                                                    let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                                    let create_parents = arguments.get("create_parents").and_then(|v| v.as_bool()).unwrap_or(false);

                                                    if path.is_empty() {
                                                        let error = JsonRpcError { code: -32602, message: "Missing or empty 'path' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }

                                                    let create_result = if create_parents {
                                                        fs::create_dir_all(path)
                                                    } else {
                                                        fs::create_dir(path)
                                                    };

                                                    match create_result {
                                                        Ok(_) => {
                                                             let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({"result": {"path": path, "success": true}})),
                                                                error: None,
                                                            };
                                                             send_response(&mut stdout, &response);
                                                        },
                                                         Err(e) => {
                                                             let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to create directory '{}': {}", path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                "delete_file" => {
                                                    let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                                     if path.is_empty() {
                                                        let error = JsonRpcError { code: -32602, message: "Missing or empty 'path' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }

                                                    match fs::remove_file(path) {
                                                          Ok(_) => {
                                                             let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({"result": {"path": path, "success": true}})),
                                                                error: None,
                                                            };
                                                             send_response(&mut stdout, &response);
                                                        },
                                                         Err(e) => {
                                                             let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to delete file '{}': {}", path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                "delete_directory" => {
                                                    let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                                    let recursive = arguments.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false);

                                                    if path.is_empty() {
                                                        let error = JsonRpcError { code: -32602, message: "Missing or empty 'path' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }

                                                    let delete_result = if recursive {
                                                        fs::remove_dir_all(path)
                                                    } else {
                                                        fs::remove_dir(path)
                                                    };

                                                    match delete_result {
                                                          Ok(_) => {
                                                             let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({"result": {"path": path, "success": true}})),
                                                                error: None,
                                                            };
                                                             send_response(&mut stdout, &response);
                                                        },
                                                         Err(e) => {
                                                             let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to delete directory '{}': {}", path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                 "rename" => {
                                                    let from_path = arguments.get("from_path").and_then(|v| v.as_str()).unwrap_or("");
                                                    let to_path = arguments.get("to_path").and_then(|v| v.as_str()).unwrap_or("");

                                                     if from_path.is_empty() {
                                                        let error = JsonRpcError { code: -32602, message: "Missing or empty 'from_path' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }
                                                     if to_path.is_empty() {
                                                        let error = JsonRpcError { code: -32602, message: "Missing or empty 'to_path' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }

                                                    match fs::rename(from_path, to_path) {
                                                        Ok(_) => {
                                                             let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({"result": {"from_path": from_path, "to_path": to_path, "success": true}})),
                                                                error: None,
                                                            };
                                                             send_response(&mut stdout, &response);
                                                        },
                                                        Err(e) => {
                                                             let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to rename from '{}' to '{}': {}", from_path, to_path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                 "get_metadata" => {
                                                    let path = arguments.get("path").and_then(|v| v.as_str()).unwrap_or("");
                                                    if path.is_empty() {
                                                        let error = JsonRpcError { code: -32602, message: "Missing or empty 'path' parameter".to_string(), data: None };
                                                        send_error_response(&mut stdout, request.id, error);
                                                        continue;
                                                    }

                                                    match fs::metadata(path) {
                                                        Ok(metadata) => {
                                                            let modified_time = metadata.modified().ok()
                                                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                                                .map(|d| d.as_secs()); // Convert to Unix timestamp (seconds)

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "result": {
                                                                        "path": path,
                                                                        "is_dir": metadata.is_dir(),
                                                                        "is_file": metadata.is_file(),
                                                                        "is_symlink": metadata.is_symlink(),
                                                                        "size": metadata.len(),
                                                                        "modified_timestamp_secs": modified_time,
                                                                        // Add other relevant metadata if needed (permissions, etc.)
                                                                    }
                                                                })),
                                                                error: None,
                                                            };
                                                            send_response(&mut stdout, &response);
                                                        },
                                                        Err(e) => {
                                                            let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to get metadata for '{}': {}", path, e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                 },
                                                // --- Default ---
                                                _ => {
                                                    // Unknown tool
                                                    let error = JsonRpcError {
                                                        code: -32601, // Method not found
                                                        message: format!("Unknown tool: {}", tool_name),
                                                        data: None,
                                                    };
                                                    send_error_response(&mut stdout, request.id, error);
                                                }
                                            }
                                        } else {
                                            // Missing parameters for tool execution
                                            let error = JsonRpcError {
                                                code: -32602, // Invalid params
                                                message: "Missing parameters for tool execution".to_string(),
                                                data: None,
                                            };
                                           send_error_response(&mut stdout, request.id, error);
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
                                        send_response(&mut stdout, &response);
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
                                                            send_response(&mut stdout, &response);
                                                        },
                                                        Err(e) => {
                                                             let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: format!("Failed to get current directory: {}", e),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
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
                                                           send_response(&mut stdout, &response);
                                                        },
                                                        None => {
                                                            let error = JsonRpcError {
                                                                code: -32000, // Server error
                                                                message: "Failed to get home directory".to_string(),
                                                                data: None,
                                                            };
                                                            send_error_response(&mut stdout, request.id, error);
                                                        }
                                                    }
                                                },
                                                _ => {
                                                    // Unknown resource
                                                    let error = JsonRpcError {
                                                        code: -32601, // Method not found
                                                        message: format!("Unknown resource: {}", resource_name),
                                                        data: None,
                                                    };
                                                   send_error_response(&mut stdout, request.id, error);
                                                }
                                            }
                                        } else {
                                            // Missing parameters for resource get
                                             let error = JsonRpcError {
                                                code: -32602, // Invalid params
                                                message: "Missing parameters for resource get".to_string(),
                                                data: None,
                                            };
                                            send_error_response(&mut stdout, request.id, error);
                                        }
                                    },
                                    "exit" => {
                                        // Exit the process
                                        process::exit(0);
                                    },
                                    _ => {
                                        // Method not found
                                        let error = JsonRpcError {
                                            code: -32601, // Method not found
                                            message: format!("Method not found: {}", request.method),
                                            data: None,
                                        };
                                        send_error_response(&mut stdout, request.id, error);
                                    }
                                }
                            },
                            Err(e) => {
                                // Parse error
                                let error = JsonRpcError {
                                    code: -32700, // Parse error
                                    message: format!("Parse error: {}", e),
                                    data: None,
                                };
                                // No valid ID available in case of parse error
                                send_error_response(&mut stdout, Some(json!(null)), error);
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

// --- Helper Functions ---

/// Sends a JSON-RPC response to stdout
fn send_response(stdout: &mut io::Stdout, response: &Response) {
    match serde_json::to_string(response) {
        Ok(response_json) => {
            let header = format!("Content-Length: {}

", response_json.len());
            if let Err(e) = stdout.write_all(header.as_bytes()) {
                 eprintln!("Failed to write response header: {}", e);
                 return; // Exit if writing fails
            }
            if let Err(e) = stdout.write_all(response_json.as_bytes()) {
                 eprintln!("Failed to write response body: {}", e);
                 return; // Exit if writing fails
            }
            if let Err(e) = stdout.flush() {
                 eprintln!("Failed to flush stdout: {}", e);
            }
        },
        Err(e) => {
             eprintln!("Failed to serialize response: {}", e);
        }
    }
}


/// Sends a JSON-RPC error response to stdout
fn send_error_response(stdout: &mut io::Stdout, id: Option<serde_json::Value>, error: JsonRpcError) {
    let response = Response {
        jsonrpc: "2.0".to_string(),
        id: id.unwrap_or(json!(null)), // Use null ID if original was None
        result: None,
        error: Some(error),
    };
   send_response(stdout, &response);
}

/// Reads the content of a file, optionally a specific range of lines.
fn read_file_content(path: &str, start_line: Option<usize>, end_line: Option<usize>) -> Result<String, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = io::BufReader::new(file);

    let mut content = String::new();
    let mut current_line = 1;

    match (start_line, end_line) {
        (Some(start), Some(end)) => {
            if start > end {
                return Err("start_line cannot be greater than end_line".into());
            }
            for line_result in reader.lines() {
                if current_line >= start && current_line <= end {
                    let line = line_result?;
                    content.push_str(&line);
                    content.push_str("\n"); // Use push_str for newline
                }
                if current_line > end {
                    break;
                }
                current_line += 1;
            }
             // Remove trailing newline if content was added
            if !content.is_empty() {
                content.pop();
            }
        },
        (Some(start), None) => {
             for line_result in reader.lines() {
                if current_line >= start {
                    let line = line_result?;
                    content.push_str(&line);
                    content.push_str("\n"); // Use push_str for newline
                }
                current_line += 1;
            }
             if !content.is_empty() {
                content.pop();
            }
        },
        (None, Some(end)) => {
             for line_result in reader.lines() {
                 if current_line <= end {
                    let line = line_result?;
                    content.push_str(&line);
                    content.push_str("\n"); // Use push_str for newline
                } else {
                    break;
                }
                current_line += 1;
            }
             if !content.is_empty() {
                content.pop();
            }
        },
        (None, None) => {
            // Read entire file if no range specified
             content = fs::read_to_string(path)?;
        }
    }

    Ok(content)
}


/// Applies a patch to a file using diffy.
fn apply_patch_to_file(path: &Path, patch_content: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    // 1. Read the original file content
    let original_content = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read original file '{}': {}", path.display(), e))?;

    // 2. Parse the patch using diffy
    let patch = diffy::Patch::from_str(patch_content)
        .map_err(|e| format!("Failed to parse patch: {}", e))?; // Diffy's parse error is simpler

    // 3. Apply the patch using diffy
    let patched_content = diffy::apply(&original_content, &patch)
        .map_err(|e| format!("Failed to apply patch to '{}': {}", path.display(), e))?; // Diffy's apply error

    // 4. Write the patched content back
    fs::write(path, patched_content)
         .map_err(|e| format!("Failed to write patched file '{}': {}", path.display(), e).into())
} 