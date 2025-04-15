use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::io::{self, Read, Write};
use std::process;

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

fn main() {
    // Initialize logger if needed
    // env_logger::init();
    
    // Main processing loop
    match process_stdin() {
        Ok(_) => {},
        Err(err) => {
            eprintln!("Error: {}", err);
            process::exit(1);
        }
    }
}

fn process_stdin() -> Result<(), String> {
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();
    
    let mut buffer = Vec::new();
    let mut content_length: Option<usize> = None;
    
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
                            return Err(format!("Failed to read message content: {}", e));
                        }
                        
                        // Process the message
                        let json_str = String::from_utf8_lossy(&buffer);
                        process_message(&json_str, &mut stdout)?;
                        
                        // Reset for next message
                        content_length = None;
                        buffer.clear();
                    }
                } else if line.starts_with("Content-Length:") {
                    let parts: Vec<&str> = line.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        match parts[1].trim().parse::<usize>() {
                            Ok(len) => content_length = Some(len),
                            Err(e) => return Err(format!("Invalid Content-Length: {}", e)),
                        }
                    }
                }
            },
            Err(e) => return Err(format!("Failed to read line: {}", e)),
        }
    }
    
    Ok(())
}

fn process_message(json_str: &str, stdout: &mut impl Write) -> Result<(), String> {
    match serde_json::from_str::<Request>(json_str) {
        Ok(request) => {
            match request.method.as_str() {
                "initialize" => handle_initialize(request, stdout),
                "shutdown" => {
                    // Just acknowledge shutdown
                    send_response(
                        stdout,
                        request.id.unwrap_or(json!(null)),
                        Some(json!({})),
                        None,
                    )?;
                    Ok(())
                },
                "exit" => {
                    // Exit the process
                    process::exit(0);
                },
                "tool/execute" => handle_execute_tool(request, stdout),
                "resource/get" => handle_get_resource(request, stdout),
                _ => {
                    send_response(
                        stdout,
                        request.id.unwrap_or(json!(null)),
                        None,
                        Some(JsonRpcError {
                            code: -32601,
                            message: format!("Method not found: {}", request.method),
                            data: None,
                        }),
                    )?;
                    Ok(())
                }
            }
        },
        Err(e) => {
            send_response(
                stdout,
                json!(null),
                None,
                Some(JsonRpcError {
                    code: -32700,
                    message: format!("Parse error: {}", e),
                    data: None,
                }),
            )?;
            Ok(())
        }
    }
}

fn handle_initialize(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    // Parse initialization parameters if needed
    // let params: InitializeParams = serde_json::from_value(request.params.unwrap_or_default())
    //    .map_err(|e| format!("Invalid initialize params: {}", e))?;
    
    // Define server capabilities
    let server_info = ServerInfo {
        name: "filesystem-mcp".to_string(),
        version: "1.0.0".to_string(),
    };
    
    // Define tools
    let tools = vec![
        Tool {
            name: "list_directory".to_string(),
            description: "Lists contents of a directory".to_string(),
            schema: Some(json!({
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
            description: "Reads content of a file".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    },
                    "encoding": {
                        "type": "string",
                        "description": "File encoding (defaults to utf-8)",
                        "default": "utf-8"
                    }
                },
                "required": ["path"]
            })),
        },
        Tool {
            name: "write_file".to_string(),
            description: "Writes content to a file".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    },
                    "mode": {
                        "type": "string",
                        "description": "Write mode: 'create', 'append', or 'overwrite'",
                        "default": "overwrite",
                        "enum": ["create", "append", "overwrite"]
                    }
                },
                "required": ["path", "content"]
            })),
        },
        Tool {
            name: "delete".to_string(),
            description: "Deletes a file or directory".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file or directory to delete"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to recursively delete directories",
                        "default": false
                    }
                },
                "required": ["path"]
            })),
        },
        Tool {
            name: "create_directory".to_string(),
            description: "Creates a new directory".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory to create"
                    },
                    "recursive": {
                        "type": "boolean",
                        "description": "Whether to create parent directories if they don't exist",
                        "default": false
                    }
                },
                "required": ["path"]
            })),
        },
        Tool {
            name: "file_info".to_string(),
            description: "Gets information about a file or directory".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file or directory"
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
            description: "Gets the current working directory".to_string(),
            schema: None,
        },
        Resource {
            name: "home_directory".to_string(),
            description: "Gets the user's home directory".to_string(),
            schema: None,
        },
    ];
    
    let capabilities = ServerCapabilities { tools, resources };
    let result = InitializeResult { server_info, capabilities };
    
    send_response(
        stdout,
        request.id.unwrap_or(json!(null)),
        Some(json!(result)),
        None,
    )?;
    
    Ok(())
}

fn handle_execute_tool(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    let params = match request.params {
        Some(params) => params,
        None => {
            return send_response(
                stdout,
                request.id.unwrap_or(json!(null)),
                None,
                Some(JsonRpcError {
                    code: -32602,
                    message: "Invalid params: params are required".to_string(),
                    data: None,
                }),
            );
        }
    };
    
    let execute_params: ExecuteToolParams = match serde_json::from_value(params) {
        Ok(params) => params,
        Err(e) => {
            return send_response(
                stdout,
                request.id.unwrap_or(json!(null)),
                None,
                Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", e),
                    data: None,
                }),
            );
        }
    };
    
    // Execute the appropriate tool
    match execute_params.tool_name.as_str() {
        "list_directory" => execute_list_directory(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
        "read_file" => execute_read_file(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
        "write_file" => execute_write_file(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
        "delete" => execute_delete(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
        "create_directory" => execute_create_directory(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
        "file_info" => execute_file_info(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
        _ => {
            send_response(
                stdout,
                request.id.unwrap_or(json!(null)),
                None,
                Some(JsonRpcError {
                    code: -32601,
                    message: format!("Tool not found: {}", execute_params.tool_name),
                    data: None,
                }),
            )
        }
    }
}

fn handle_get_resource(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    let params = match request.params {
        Some(params) => params,
        None => {
            return send_response(
                stdout,
                request.id.unwrap_or(json!(null)),
                None,
                Some(JsonRpcError {
                    code: -32602,
                    message: "Invalid params: params are required".to_string(),
                    data: None,
                }),
            );
        }
    };
    
    let resource_params: GetResourceParams = match serde_json::from_value(params) {
        Ok(params) => params,
        Err(e) => {
            return send_response(
                stdout,
                request.id.unwrap_or(json!(null)),
                None,
                Some(JsonRpcError {
                    code: -32602,
                    message: format!("Invalid params: {}", e),
                    data: None,
                }),
            );
        }
    };
    
    // Get the appropriate resource
    match resource_params.name.as_str() {
        "current_directory" => get_current_directory(resource_params.params, request.id.unwrap_or(json!(null)), stdout),
        "home_directory" => get_home_directory(resource_params.params, request.id.unwrap_or(json!(null)), stdout),
        _ => {
            send_response(
                stdout,
                request.id.unwrap_or(json!(null)),
                None,
                Some(JsonRpcError {
                    code: -32601,
                    message: format!("Resource not found: {}", resource_params.name),
                    data: None,
                }),
            )
        }
    }
}

// Tool execution functions
fn execute_list_directory(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path = match args.get("path") {
        Some(path) => path.as_str().ok_or_else(|| "path must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: path"),
    };
    
    let recursive = args.get("recursive")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    
    let path = PathBuf::from(path);
    if !path.exists() {
        return send_error(stdout, id, -32602, &format!("Path does not exist: {}", path.display()));
    }
    
    if !path.is_dir() {
        return send_error(stdout, id, -32602, &format!("Path is not a directory: {}", path.display()));
    }
    
    let entries = match list_directory_contents(&path, recursive) {
        Ok(entries) => entries,
        Err(e) => return send_error(stdout, id, -32000, &format!("Failed to list directory: {}", e)),
    };
    
    send_response(stdout, id, Some(json!(entries)), None)
}

fn execute_read_file(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path = match args.get("path") {
        Some(path) => path.as_str().ok_or_else(|| "path must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: path"),
    };
    
    let _encoding = args.get("encoding")
        .and_then(|e| e.as_str())
        .unwrap_or("utf-8");
    
    let path = PathBuf::from(path);
    if !path.exists() {
        return send_error(stdout, id, -32602, &format!("File does not exist: {}", path.display()));
    }
    
    if !path.is_file() {
        return send_error(stdout, id, -32602, &format!("Path is not a file: {}", path.display()));
    }
    
    match fs::read_to_string(&path) {
        Ok(content) => {
            send_response(stdout, id, Some(json!({ "content": content })), None)
        },
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to read file: {}", e)),
    }
}

fn execute_write_file(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path = match args.get("path") {
        Some(path) => path.as_str().ok_or_else(|| "path must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: path"),
    };
    
    let content = match args.get("content") {
        Some(content) => content.as_str().ok_or_else(|| "content must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: content"),
    };
    
    let mode = args.get("mode")
        .and_then(|m| m.as_str())
        .unwrap_or("overwrite");
    
    let path = PathBuf::from(path);
    
    // Check if parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return send_error(stdout, id, -32602, &format!("Parent directory does not exist: {}", parent.display()));
        }
    }
    
    let result = match mode {
        "create" => {
            if path.exists() {
                Err(io::Error::new(io::ErrorKind::AlreadyExists, "File already exists"))
            } else {
                fs::write(&path, content)
            }
        },
        "append" => {
            let mut file = match fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path) {
                Ok(file) => file,
                Err(e) => return send_error(stdout, id, -32000, &format!("Failed to open file: {}", e)),
            };
            
            match file.write_all(content.as_bytes()) {
                Ok(_) => Ok(()),
                Err(e) => Err(e),
            }
        },
        "overwrite" => fs::write(&path, content),
        _ => return send_error(stdout, id, -32602, &format!("Invalid mode: {}", mode)),
    };
    
    match result {
        Ok(_) => {
            send_response(stdout, id, Some(json!({ "success": true, "path": path.to_string_lossy() })), None)
        },
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to write file: {}", e)),
    }
}

fn execute_delete(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path = match args.get("path") {
        Some(path) => path.as_str().ok_or_else(|| "path must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: path"),
    };
    
    let recursive = args.get("recursive")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    
    let path = PathBuf::from(path);
    if !path.exists() {
        return send_error(stdout, id, -32602, &format!("Path does not exist: {}", path.display()));
    }
    
    let result = if path.is_dir() {
        if recursive {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_dir(&path)
        }
    } else {
        fs::remove_file(&path)
    };
    
    match result {
        Ok(_) => {
            send_response(stdout, id, Some(json!({ "success": true, "path": path.to_string_lossy() })), None)
        },
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to delete: {}", e)),
    }
}

fn execute_create_directory(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path = match args.get("path") {
        Some(path) => path.as_str().ok_or_else(|| "path must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: path"),
    };
    
    let recursive = args.get("recursive")
        .and_then(|r| r.as_bool())
        .unwrap_or(false);
    
    let path = PathBuf::from(path);
    
    let result = if recursive {
        fs::create_dir_all(&path)
    } else {
        fs::create_dir(&path)
    };
    
    match result {
        Ok(_) => {
            send_response(stdout, id, Some(json!({ "success": true, "path": path.to_string_lossy() })), None)
        },
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to create directory: {}", e)),
    }
}

fn execute_file_info(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let path = match args.get("path") {
        Some(path) => path.as_str().ok_or_else(|| "path must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: path"),
    };
    
    let path = PathBuf::from(path);
    if !path.exists() {
        return send_error(stdout, id, -32602, &format!("Path does not exist: {}", path.display()));
    }
    
    match get_file_info(&path) {
        Ok(info) => send_response(stdout, id, Some(json!(info)), None),
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to get file info: {}", e)),
    }
}

// Resource getters
fn get_current_directory(_params: Option<Value>, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    match std::env::current_dir() {
        Ok(path) => {
            send_response(stdout, id, Some(json!({ "path": path.to_string_lossy() })), None)
        },
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to get current directory: {}", e)),
    }
}

fn get_home_directory(_params: Option<Value>, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    match dirs::home_dir() {
        Some(path) => {
            send_response(stdout, id, Some(json!({ "path": path.to_string_lossy() })), None)
        },
        None => send_error(stdout, id, -32000, "Failed to get home directory"),
    }
}

// Helper functions
fn list_directory_contents(path: &Path, recursive: bool) -> Result<Vec<FileInfo>, io::Error> {
    let mut entries = Vec::new();
    
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        
        let file_type = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().to_string();
        let path_str = entry.path().to_string_lossy().to_string();
        
        let mut info = FileInfo {
            name,
            path: path_str,
            is_dir: file_type.is_dir(),
            size: if file_type.is_file() { Some(metadata.len()) } else { None },
            modified: metadata.modified().ok().map(|t| format!("{:?}", t)),
            created: metadata.created().ok().map(|t| format!("{:?}", t)),
            is_symlink: file_type.is_symlink(),
        };
        
        entries.push(info);
        
        // If recursive and it's a directory, add its contents
        if recursive && file_type.is_dir() {
            match list_directory_contents(&entry.path(), true) {
                Ok(sub_entries) => entries.extend(sub_entries),
                Err(_) => continue, // Skip directories we can't read
            }
        }
    }
    
    Ok(entries)
}

fn get_file_info(path: &Path) -> Result<FileInfo, io::Error> {
    let metadata = path.metadata()?;
    let file_type = metadata.file_type();
    
    Ok(FileInfo {
        name: path.file_name().map_or_else(
            || path.to_string_lossy().to_string(),
            |n| n.to_string_lossy().to_string()
        ),
        path: path.to_string_lossy().to_string(),
        is_dir: file_type.is_dir(),
        size: if file_type.is_file() { Some(metadata.len()) } else { None },
        modified: metadata.modified().ok().map(|t| format!("{:?}", t)),
        created: metadata.created().ok().map(|t| format!("{:?}", t)),
        is_symlink: file_type.is_symlink(),
    })
}

// JSON-RPC response helpers
fn send_response(
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
    
    let response_json = match serde_json::to_string(&response) {
        Ok(json) => json,
        Err(e) => return Err(format!("Failed to serialize response: {}", e)),
    };
    
    let message = format!(
        "Content-Length: {}\r\n\r\n{}",
        response_json.len(),
        response_json
    );
    
    match stdout.write_all(message.as_bytes()) {
        Ok(_) => {
            stdout.flush().map_err(|e| format!("Failed to flush stdout: {}", e))?;
            Ok(())
        },
        Err(e) => Err(format!("Failed to write response: {}", e)),
    }
}

fn send_error(stdout: &mut impl Write, id: Value, code: i64, message: &str) -> Result<(), String> {
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
} 