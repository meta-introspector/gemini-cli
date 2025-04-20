use diffy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};

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
struct Notification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
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
struct InitializeParams {
    #[serde(rename = "clientInfo")]
    client_info: ClientInfo,
    trace: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ClientInfo {
    name: String,
    version: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ExecuteToolParams {
    tool_name: String,
    arguments: Value,
}

#[derive(Serialize, Deserialize, Debug)]
struct GetResourceParams {
    name: String,
    params: Option<Value>,
}

// Filesystem operation result
#[derive(Serialize, Deserialize, Debug)]
struct FileInfo {
    name: String,
    path: String,
    is_dir: bool,
    size: Option<u64>,
    modified: Option<String>,
    created: Option<String>,
    is_symlink: bool,
}

// Restore local definitions
#[derive(Serialize, Deserialize, Debug)]
struct InitializeResult {
    #[serde(rename = "serverInfo")]
    server_info: ServerInfo,
    capabilities: ServerCapabilities,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerCapabilities {
    tools: Vec<Tool>,
    resources: Vec<Resource>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Tool {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Resource {
    name: String,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<Value>,
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("Filesystem MCP server starting...");

    // Send immediate initialization response
    {
        let mut stdout = io::stdout();
        let server_info = ServerInfo {
            name: "filesystem-mcp".to_string(),
            version: "1.0.0".to_string(),
        };
        let tools = vec![
            Tool {
                name: "list_directory".to_string(),
                description: "Lists contents of a directory.".to_string(),
                schema: None,
            },
            Tool {
                name: "read_file".to_string(),
                description: "Reads content of a file.".to_string(),
                schema: None,
            },
            Tool {
                name: "write_file".to_string(),
                description: "Writes content to a file.".to_string(),
                schema: None,
            },
            Tool {
                name: "apply_patch".to_string(),
                description: "Applies a patch to a file.".to_string(),
                schema: None,
            },
            Tool {
                name: "delete".to_string(),
                description: "Deletes a file or directory.".to_string(),
                schema: None,
            },
            Tool {
                name: "create_directory".to_string(),
                description: "Creates a directory.".to_string(),
                schema: None,
            },
            Tool {
                name: "rename".to_string(),
                description: "Renames a file or directory.".to_string(),
                schema: None,
            },
            Tool {
                name: "file_info".to_string(),
                description: "Gets information about a file or directory.".to_string(),
                schema: None,
            },
        ];
        let resources = vec![
            Resource {
                name: "current_directory".to_string(),
                description: "Gets the current working directory.".to_string(),
                schema: None,
            },
            Resource {
                name: "home_directory".to_string(),
                description: "Gets the user's home directory.".to_string(),
                schema: None,
            },
        ];
        let capabilities = ServerCapabilities { tools, resources };
        let init_result = InitializeResult {
            server_info,
            capabilities,
        };

        let response = Response {
            jsonrpc: "2.0".to_string(),
            // Assuming a common ID pattern or using a fixed one for the proactive response
            id: json!(1), // Use the typical ID for filesystem server if known, otherwise a placeholder
            result: Some(json!({
                "serverInfo": init_result.server_info,
                "capabilities": init_result.capabilities,
                "status": "initialized"
            })),
            error: None,
        };

        match serde_json::to_string(&response) {
            Ok(response_json) => {
                let message = format!(
                    "Content-Length: {}\r\n\r\n{}",
                    response_json.len(),
                    response_json
                );
                println!(
                    "Sending early init response ({} bytes)",
                    response_json.len()
                );
                if let Err(e) = stdout.write_all(message.as_bytes()) {
                    eprintln!("Failed to write early init response: {}", e);
                    // Decide if we should exit or just log
                }
                if let Err(e) = stdout.flush() {
                    eprintln!("Failed to flush early init response: {}", e);
                }
                println!("Early init response sent.");
            }
            Err(e) => {
                eprintln!("Failed to serialize early init response: {}", e);
            }
        }
    }

    // Main processing loop
    match process_stdin().await {
        Ok(_) => Ok(()),
        Err(err) => {
            eprintln!("Error: {}", err);
            // process::exit(1); // Removed process::exit
            Err(err.into()) // Return error
        }
    }
}

async fn process_stdin() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();

    let mut buffer = Vec::new();
    let mut shutdown_requested = false;

    loop {
        if shutdown_requested {
            println!("Shutdown requested, exiting loop.");
            break;
        }

        // Read headers until we find a blank line
        let mut content_length: Option<usize> = None;
        let mut line = String::new();

        loop {
            match stdin_lock.read_line(&mut line) {
                Ok(0) => {
                    return Ok(());
                } // EOF, exit gracefully
                Ok(_) => {
                    let line_trim = line.trim();
                    if line_trim.is_empty() {
                        // End of headers, read the content if we have a content-length
                        break;
                    } else if line_trim.starts_with("Content-Length:") {
                        // Extract clean number from Content-Length header
                        if let Some(len_str) = line_trim.strip_prefix("Content-Length:") {
                            let len_str = len_str.trim();
                            println!("Parsing Content-Length value: '{}'", len_str);

                            // Extract only the numeric part
                            let numeric_part = len_str
                                .chars()
                                .take_while(|c| c.is_ascii_digit())
                                .collect::<String>();

                            match numeric_part.parse::<usize>() {
                                Ok(len) => {
                                    println!("Successfully parsed Content-Length: {}", len);
                                    content_length = Some(len);
                                }
                                Err(e) => {
                                    // Better error handling - continue processing other headers
                                    eprintln!("Invalid Content-Length value '{}': {}", len_str, e);
                                }
                            }
                        }
                    }
                    // Reset line for next read
                    line.clear();
                }
                Err(e) => {
                    return Err(format!("Failed to read line: {}", e));
                }
            }
        }

        // If we have a content-length, read that many bytes
        if let Some(length) = content_length {
            println!("Reading {} bytes of content", length);

            // Set buffer to exact size
            buffer.resize(length, 0);

            // Read exactly the number of bytes specified
            let mut total_read = 0;
            while total_read < length {
                match stdin_lock.read(&mut buffer[total_read..]) {
                    Ok(0) => {
                        return Err("Unexpected EOF while reading content".to_string());
                    }
                    Ok(n) => {
                        total_read += n;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                        continue;
                    }
                    Err(e) => {
                        return Err(format!("Failed to read message content: {}", e));
                    }
                }
            }

            // Process the message
            match String::from_utf8(buffer.clone()) {
                Ok(json_str) => {
                    // Parse the request first to check the method
                    let request_peek: Result<Request, _> = serde_json::from_str(&json_str);
                    let mut should_shutdown = false;

                    if let Ok(ref req) = request_peek {
                        if req.method == "shutdown" {
                            should_shutdown = true;
                        }
                        if req.method == "exit" {
                            // Exit immediately as per spec for notifications
                            println!("Received exit notification. Exiting immediately.");
                            break;
                        }
                    }

                    // Process the message (which sends the response)
                    if let Err(e) = process_message(&json_str, &mut stdout).await {
                        eprintln!("Error processing message: {}", e);
                        // Decide if we should break or continue on processing errors
                        // return Err(e); // Option to propagate error
                    }

                    // If it was a shutdown request, set the flag to exit on the next iteration
                    if should_shutdown {
                        shutdown_requested = true;
                        println!("Shutdown response sent, flag set to exit loop.");
                        continue; // Jump to the start of the loop to check the flag
                    }
                }
                Err(e) => {
                    return Err(format!("Invalid UTF-8 in message content: {}", e));
                }
            }
        } else {
            // Send error response about missing Content-Length
            let error_response = json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {
                    "code": -32700,
                    "message": "No Content-Length header found"
                }
            });

            let response_json = serde_json::to_string(&error_response)
                .map_err(|e| format!("Failed to serialize error response: {}", e))?;

            // Write response with proper Content-Length framing
            let message = format!(
                "Content-Length: {}\r\n\r\n{}",
                response_json.len(),
                response_json
            );
            if let Err(e) = stdout.write_all(message.as_bytes()) {
                return Err(format!("Failed to write error response: {}", e));
            }
            if let Err(e) = stdout.flush() {
                return Err(format!("Failed to flush output: {}", e));
            }

            // Continue processing next message
            continue;
        }
    }
    // Added explicit return Ok(()) here
    Ok(())
}

async fn process_message(json_str: &str, stdout: &mut impl Write) -> Result<(), String> {
    match serde_json::from_str::<Request>(json_str) {
        Ok(request) => {
            println!(
                "Received request method: {} (ID: {:?})",
                request.method, request.id
            );

            match request.method.as_str() {
                "initialize" => handle_initialize(request, stdout).await,
                "shutdown" => handle_shutdown(request, stdout).await,
                "exit" => {
                    // Exit notification - Host should close connection. Nothing to do here.
                    println!("Received exit notification. Host should terminate connection.");
                    Ok(())
                }
                "tool/execute" => handle_execute_tool(request, stdout).await,
                "resource/get" => handle_get_resource(request, stdout).await,
                _ => {
                    println!("Unknown method: {}", request.method);
                    send_response(
                        stdout,
                        request.id.unwrap_or(json!(null)),
                        None,
                        Some(JsonRpcError {
                            code: -32601,
                            message: format!("Method not found: {}", request.method),
                            data: None,
                        }),
                    )
                    .await
                }
            }
        }
        Err(e) => {
            println!("Failed to parse JSON-RPC request: {}", e);
            send_response(
                stdout,
                json!(null),
                None,
                Some(JsonRpcError {
                    code: -32700,
                    message: format!("Parse error: {}", e),
                    data: None,
                }),
            )
            .await
        }
    }
}

async fn handle_initialize(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    println!("Received initialize request (ID: {:?})", request.id);

    // Define server capabilities
    let server_info = ServerInfo {
        name: "filesystem-mcp".to_string(),
        version: "1.0.0".to_string(),
    };

    // Define tools with updated schemas based on CLI versions
    let tools = vec![
        Tool {
            name: "list_directory".to_string(),
            description: "Lists contents of a directory.".to_string(),
            schema: Some(json!({
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
            description: "Reads content of a file, optionally a specific range of lines."
                .to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read."
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Line number to start reading from (1-indexed)."
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Line number to end reading at (1-indexed, inclusive)."
                    }
                },
                "required": ["path"]
            })),
        },
        Tool {
            name: "write_file".to_string(),
            description: "Writes content to a file, optionally creating it if it doesn't exist."
                .to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write."
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file."
                    },
                    "create": {
                        "type": "boolean",
                        "description": "Whether to create the file if it doesn't exist.",
                        "default": true
                    },
                    "append": {
                        "type": "boolean",
                        "description": "Whether to append to the file instead of overwriting.",
                        "default": false
                    }
                },
                "required": ["path", "content"]
            })),
        },
        Tool {
            name: "apply_patch".to_string(),
            description: "Applies a patch to a file.".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to patch."
                    },
                    "patch": {
                        "type": "string",
                        "description": "Patch content in unified diff format."
                    }
                },
                "required": ["path", "patch"]
            })),
        },
        Tool {
            name: "delete".to_string(),
            description: "Deletes a file or directory.".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file or directory to delete."
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to delete directories recursively.",
                        "default": false
                    }
                },
                "required": ["path"]
            })),
        },
        Tool {
            name: "create_directory".to_string(),
            description: "Creates a directory.".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory to create."
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to create parent directories as needed.",
                        "default": false
                    }
                },
                "required": ["path"]
            })),
        },
        Tool {
            name: "rename".to_string(),
            description: "Renames a file or directory.".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "from": {
                        "type": "string",
                        "description": "Path to the file or directory to rename."
                    },
                    "to": {
                        "type": "string",
                        "description": "New path for the file or directory."
                    }
                },
                "required": ["from", "to"]
            })),
        },
        Tool {
            name: "file_info".to_string(),
            description: "Gets information about a file or directory.".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file or directory."
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
            description: "Gets the current working directory.".to_string(),
            schema: None,
        },
        Resource {
            name: "home_directory".to_string(),
            description: "Gets the user's home directory.".to_string(),
            schema: None,
        },
    ];

    // Create capabilities object
    let capabilities = ServerCapabilities { tools, resources };

    // Create response with proper format
    let response = Response {
        jsonrpc: "2.0".to_string(),
        id: request.id.unwrap_or(json!(null)),
        result: Some(json!({
            "serverInfo": server_info,
            "capabilities": capabilities,
            "status": "initialized"
        })),
        error: None,
    };

    // Serialize and send the response with proper headers
    let response_json = serde_json::to_string(&response)
        .map_err(|e| format!("Failed to serialize response: {}", e))?;

    println!(
        "Sending initialization response (length: {})",
        response_json.len()
    );
    let message = format!(
        "Content-Length: {}\r\n\r\n{}",
        response_json.len(),
        response_json
    );
    stdout
        .write_all(message.as_bytes())
        .map_err(|e| format!("Failed to write response: {}", e))?;
    stdout
        .flush()
        .map_err(|e| format!("Failed to flush output: {}", e))?;

    println!("Initialization response sent successfully");
    Ok(())
}

async fn handle_shutdown(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    println!("Received shutdown request (ID: {:?})", request.id);

    // Explicitly create the response
    let response = Response {
        jsonrpc: "2.0".to_string(),
        id: request.id.unwrap_or(json!(null)),
        result: Some(json!(null)),
        error: None,
    };

    // Serialize and format with Content-Length header
    let json_str = match serde_json::to_string(&response) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to serialize shutdown response: {}", e);
            return Err(format!("Failed to serialize shutdown response: {}", e));
        }
    };

    let message = format!("Content-Length: {}\r\n\r\n{}", json_str.len(), json_str);

    // Log the response being sent
    println!("Sending shutdown response: {}", json_str);

    // Write and explicitly flush
    match stdout.write_all(message.as_bytes()) {
        Ok(_) => println!("Shutdown response written"),
        Err(e) => {
            eprintln!("Failed to write shutdown response: {}", e);
            return Err(format!("Failed to write shutdown response: {}", e));
        }
    }

    match stdout.flush() {
        Ok(_) => {
            println!("Shutdown response sent successfully");
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to flush shutdown response: {}", e);
            Err(format!("Failed to flush shutdown response: {}", e))
        }
    }
}

async fn handle_execute_tool(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    let params: ExecuteToolParams = match request.params {
        Some(p) => {
            serde_json::from_value(p).map_err(|e| format!("Invalid execute tool params: {}", e))?
        }
        None => {
            return send_error(
                stdout,
                request.id.unwrap_or(json!(null)),
                -32602,
                "Missing params for tool/execute",
            )
            .await;
        }
    };

    let request_id = request.id.unwrap_or(json!(null));

    match params.tool_name.as_str() {
        "list_directory" => execute_list_directory(params.arguments, request_id, stdout).await,
        "read_file" => execute_read_file(params.arguments, request_id, stdout).await,
        "write_file" => execute_write_file(params.arguments, request_id, stdout).await,
        "apply_patch" => execute_apply_patch(params.arguments, request_id, stdout).await,
        "delete" => execute_delete(params.arguments, request_id, stdout).await,
        "create_directory" => execute_create_directory(params.arguments, request_id, stdout).await,
        "rename" => execute_rename(params.arguments, request_id, stdout).await,
        "file_info" => execute_file_info(params.arguments, request_id, stdout).await,
        _ => {
            send_error(
                stdout,
                request_id,
                -32601,
                &format!("Tool not found: {}", params.tool_name),
            )
            .await
        }
    }
}

async fn handle_get_resource(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    let params: GetResourceParams = match request.params {
        Some(p) => {
            serde_json::from_value(p).map_err(|e| format!("Invalid get resource params: {}", e))?
        }
        None => {
            return send_error(
                stdout,
                request.id.unwrap_or(json!(null)),
                -32602,
                "Missing params for resource/get",
            )
            .await;
        }
    };

    match params.name.as_str() {
        "current_directory" => {
            get_current_directory(params.params, request.id.unwrap_or(json!(null)), stdout).await
        }
        "home_directory" => {
            get_home_directory(params.params, request.id.unwrap_or(json!(null)), stdout).await
        }
        _ => {
            send_error(
                stdout,
                request.id.unwrap_or(json!(null)),
                -32601,
                &format!("Resource not found: {}", params.name),
            )
            .await
        }
    }
}

// Tool implementations
async fn execute_list_directory(
    args: Value,
    id: Value,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;
    let path = Path::new(path_str);
    let recursive = args["recursive"].as_bool().unwrap_or(false);

    match list_directory_contents(path, recursive).await {
        Ok(entries) => send_response(stdout, id, Some(json!(entries)), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to list directory '{}': {}", path_str, e),
            )
            .await
        }
    }
}

async fn execute_read_file(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;
    let start_line = args
        .get("start_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let end_line = args
        .get("end_line")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    let path = Path::new(path_str);

    match read_file_content_helper(path, start_line, end_line) {
        Ok(content) => send_response(stdout, id, Some(json!({ "content": content })), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to read file '{}': {}", path_str, e),
            )
            .await
        }
    }
}

async fn execute_write_file(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;
    let content = args["content"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'content' argument".to_string())?;
    let mode = args
        .get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("overwrite");

    let path = PathBuf::from(path_str);

    if mode == "overwrite" || mode == "append" {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return send_error(
                        stdout,
                        id,
                        -32000,
                        &format!(
                            "Failed to create parent directories for '{}': {}",
                            path.display(),
                            e
                        ),
                    )
                    .await;
                }
            }
        }
    }

    let write_result = match mode {
        "create" => {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => file.write_all(content.as_bytes()),
                Err(e) => Err(e),
            }
        }
        "append" => match fs::OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut file) => file.write_all(content.as_bytes()),
            Err(e) => Err(e),
        },
        "overwrite" => fs::write(&path, content),
        _ => {
            return send_error(stdout, id, -32602, &format!("Invalid write mode: {}", mode)).await;
        }
    };

    match write_result {
        Ok(_) => send_response(stdout, id, Some(json!({ "success": true })), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to write file '{}': {}", path.display(), e),
            )
            .await
        }
    }
}

async fn execute_apply_patch(
    args: Value,
    id: Value,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;
    let patch_content = args["patch_content"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'patch_content' argument".to_string())?;
    let path = Path::new(path_str);

    if !path.exists() {
        return send_error(
            stdout,
            id,
            -32001,
            &format!("File not found for patching: {}", path_str),
        )
        .await;
    }
    if !path.is_file() {
        return send_error(
            stdout,
            id,
            -32001,
            &format!("Path is not a file: {}", path_str),
        )
        .await;
    }

    match apply_patch_to_file_helper(path, patch_content) {
        Ok(_) => send_response(stdout, id, Some(json!({ "success": true })), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to apply patch to '{}': {}", path_str, e),
            )
            .await
        }
    }
}

async fn execute_delete(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;
    let path = Path::new(path_str);

    if !path.exists() {
        return send_error(stdout, id, -32001, &format!("Path not found: {}", path_str)).await;
    }

    let result = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };

    match result {
        Ok(_) => send_response(stdout, id, Some(json!({ "success": true })), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to delete '{}': {}", path_str, e),
            )
            .await
        }
    }
}

async fn execute_create_directory(
    args: Value,
    id: Value,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;
    let create_parents = args["create_parents"].as_bool().unwrap_or(false);
    let path = Path::new(path_str);

    let result = if create_parents {
        fs::create_dir_all(path)
    } else {
        fs::create_dir(path)
    };

    match result {
        Ok(_) => send_response(stdout, id, Some(json!({ "success": true })), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to create directory '{}': {}", path_str, e),
            )
            .await
        }
    }
}

async fn execute_rename(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let from_path_str = args["from_path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'from_path' argument".to_string())?;
    let to_path_str = args["to_path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'to_path' argument".to_string())?;

    let from_path = Path::new(from_path_str);
    let to_path = Path::new(to_path_str);

    if !from_path.exists() {
        return send_error(
            stdout,
            id,
            -32001,
            &format!("Source path not found: {}", from_path_str),
        )
        .await;
    }

    if let Some(parent) = to_path.parent() {
        if !parent.exists() {
            return send_error(
                stdout,
                id,
                -32001,
                &format!("Target directory not found: {}", parent.display()),
            )
            .await;
        }
    }

    match fs::rename(from_path, to_path) {
        Ok(_) => send_response(stdout, id, Some(json!({ "success": true })), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!(
                    "Failed to rename from '{}' to '{}': {}",
                    from_path_str, to_path_str, e
                ),
            )
            .await
        }
    }
}

async fn execute_file_info(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path_str = args["path"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'path' argument".to_string())?;
    let path = Path::new(path_str);

    match get_file_info(path).await {
        Ok(info) => send_response(stdout, id, Some(json!(info)), None).await,
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to get file info for '{}': {}", path_str, e),
            )
            .await
        }
    }
}

// Resource implementations
async fn get_current_directory(
    _params: Option<Value>,
    id: Value,
    stdout: &mut impl Write,
) -> Result<(), String> {
    match std::env::current_dir() {
        Ok(path) => {
            send_response(
                stdout,
                id,
                Some(json!({ "path": path.display().to_string() })),
                None,
            )
            .await
        }
        Err(e) => {
            send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to get current directory: {}", e),
            )
            .await
        }
    }
}

async fn get_home_directory(
    _params: Option<Value>,
    id: Value,
    stdout: &mut impl Write,
) -> Result<(), String> {
    match dirs::home_dir() {
        Some(path) => {
            send_response(
                stdout,
                id,
                Some(json!({ "path": path.display().to_string() })),
                None,
            )
            .await
        }
        None => send_error(stdout, id, -32000, "Failed to get home directory").await,
    }
}

// Helper functions
async fn list_directory_contents(path: &Path, recursive: bool) -> Result<Vec<FileInfo>, io::Error> {
    let mut entries = Vec::new();

    let dir_entries = fs::read_dir(path)?;
    for entry_result in dir_entries {
        let entry = entry_result?;
        let entry_path = entry.path();
        let file_info = get_file_info(&entry_path).await?;
        entries.push(file_info);

        if recursive && entry_path.is_dir() {
            let sub_entries_future = Box::pin(list_directory_contents(&entry_path, recursive));
            let sub_entries = sub_entries_future.await?;
            entries.extend(sub_entries);
        }
    }

    Ok(entries)
}

async fn get_file_info(path: &Path) -> Result<FileInfo, io::Error> {
    let metadata = fs::symlink_metadata(path)?;

    let modified_time = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
        })
        .map(|dt| dt.to_rfc3339());
    let created_time = metadata
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| {
            chrono::DateTime::<chrono::Utc>::from_timestamp(d.as_secs() as i64, d.subsec_nanos())
        })
        .map(|dt| dt.to_rfc3339());

    Ok(FileInfo {
        name: path
            .file_name()
            .map_or_else(|| "".to_string(), |n| n.to_string_lossy().into_owned()),
        path: path.to_string_lossy().into_owned(),
        is_dir: metadata.is_dir(),
        size: if metadata.is_file() {
            Some(metadata.len())
        } else {
            None
        },
        modified: modified_time,
        created: created_time,
        is_symlink: metadata.file_type().is_symlink(),
    })
}

// Helper function for read_file logic
fn read_file_content_helper(
    path: &Path,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<String, Box<dyn std::error::Error>> {
    let file = fs::File::open(path)?;
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
                    content.push_str(&line_result?);
                    content.push('\n');
                }
                if current_line > end {
                    break;
                }
                current_line += 1;
            }
            if !content.is_empty() {
                content.pop();
            }
        }
        (Some(start), None) => {
            for line_result in reader.lines() {
                if current_line >= start {
                    content.push_str(&line_result?);
                    content.push('\n');
                }
                current_line += 1;
            }
            if !content.is_empty() {
                content.pop();
            }
        }
        (None, Some(end)) => {
            for line_result in reader.lines() {
                if current_line <= end {
                    content.push_str(&line_result?);
                    content.push('\n');
                } else {
                    break;
                }
                current_line += 1;
            }
            if !content.is_empty() {
                content.pop();
            }
        }
        (None, None) => {
            let mut full_content = String::new();
            let mut reader = io::BufReader::new(fs::File::open(path)?);
            reader.read_to_string(&mut full_content)?;
            content = full_content;
        }
    }

    Ok(content)
}

fn apply_patch_to_file_helper(
    path: &Path,
    patch_content: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let original_content = fs::read_to_string(path)?;
    let patch = diffy::Patch::from_str(patch_content)?;
    let patched_content = diffy::apply(&original_content, &patch)?;
    fs::write(path, patched_content)?;
    Ok(())
}

async fn send_response(
    stdout: &mut impl Write,
    id: Value,
    result: Option<Value>,
    error: Option<JsonRpcError>,
) -> Result<(), String> {
    let response = Response {
        jsonrpc: "2.0".to_string(),
        id,
        result,
        error,
    };

    let json_str = match serde_json::to_string(&response) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ERROR: Failed to serialize response: {}", e);
            return Err(format!("Failed to serialize response: {}", e));
        }
    };

    // Write response with proper Content-Length framing
    let message = format!("Content-Length: {}\r\n\r\n{}", json_str.len(), json_str);

    println!(
        "Sending response (ID: {}, length: {} bytes)",
        response.id,
        json_str.len()
    );

    // Write in steps with detailed logging
    match stdout.write_all(message.as_bytes()) {
        Ok(_) => println!("Response data written successfully"),
        Err(e) => {
            eprintln!("ERROR: Failed to write response: {}", e);
            return Err(format!("Failed to write response: {}", e));
        }
    }

    match stdout.flush() {
        Ok(_) => {
            println!("Response flushed successfully");
            Ok(())
        }
        Err(e) => {
            eprintln!("ERROR: Failed to flush output: {}", e);
            Err(format!("Failed to flush output: {}", e))
        }
    }
}

async fn send_error(
    stdout: &mut impl Write,
    id: Value,
    code: i64,
    message: &str,
) -> Result<(), String> {
    send_response(
        stdout,
        id,
        None,
        Some(JsonRpcError {
            code,
            message: message.to_string(),
            data: None,
        }),
    )
    .await
}
