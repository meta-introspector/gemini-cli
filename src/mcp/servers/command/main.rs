use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::{Command, Stdio};
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

// Command execution result
#[derive(Serialize, Deserialize, Debug)]
struct CommandResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
    success: bool,
}

fn main() {
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
    // Define server capabilities
    let server_info = ServerInfo {
        name: "command-mcp".to_string(),
        version: "1.0.0".to_string(),
    };
    
    // Define tools
    let tools = vec![
        Tool {
            name: "execute_command".to_string(),
            description: "Executes a system command and returns the result".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to execute"
                    },
                    "arguments": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        },
                        "description": "Arguments to pass to the command",
                        "default": []
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Working directory for the command (optional)"
                    },
                    "environment": {
                        "type": "object",
                        "description": "Environment variables for the command",
                        "additionalProperties": {
                            "type": "string"
                        }
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Maximum execution time in milliseconds (0 for no timeout)",
                        "default": 0
                    }
                },
                "required": ["command"]
            })),
        },
        Tool {
            name: "execute_shell".to_string(),
            description: "Executes a shell command using the default system shell".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Working directory for the command (optional)"
                    },
                    "environment": {
                        "type": "object",
                        "description": "Environment variables for the command",
                        "additionalProperties": {
                            "type": "string"
                        }
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Maximum execution time in milliseconds (0 for no timeout)",
                        "default": 0
                    }
                },
                "required": ["command"]
            })),
        },
    ];
    
    // Define resources
    let resources = vec![
        Resource {
            name: "os_info".to_string(),
            description: "Gets information about the operating system".to_string(),
            schema: None,
        },
        Resource {
            name: "environment_variables".to_string(),
            description: "Gets the current environment variables".to_string(),
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
        "execute_command" => execute_command(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
        "execute_shell" => execute_shell(execute_params.arguments, request.id.unwrap_or(json!(null)), stdout),
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
        "os_info" => get_os_info(resource_params.params, request.id.unwrap_or(json!(null)), stdout),
        "environment_variables" => get_environment_variables(resource_params.params, request.id.unwrap_or(json!(null)), stdout),
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
fn execute_command(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let command_str = match args.get("command") {
        Some(cmd) => cmd.as_str().ok_or_else(|| "command must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: command"),
    };
    
    let arguments: Vec<String> = args.get("arguments")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    
    let working_dir = args.get("working_directory")
        .and_then(|wd| wd.as_str());
    
    let environment: HashMap<String, String> = args.get("environment")
        .and_then(|env| env.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    
    // Build the command
    let mut cmd = Command::new(command_str);
    
    // Add arguments
    if !arguments.is_empty() {
        cmd.args(&arguments);
    }
    
    // Set working directory if specified
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    
    // Add environment variables
    for (key, value) in environment {
        cmd.env(key, value);
    }
    
    // Capture stdout and stderr
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    
    // Execute the command
    match cmd.output() {
        Ok(output) => {
            let result = CommandResult {
                exit_code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                success: output.status.success(),
            };
            
            send_response(stdout, id, Some(json!(result)), None)
        },
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to execute command: {}", e)),
    }
}

fn execute_shell(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let command_str = match args.get("command") {
        Some(cmd) => cmd.as_str().ok_or_else(|| "command must be a string".to_string())?,
        None => return send_error(stdout, id, -32602, "Missing required parameter: command"),
    };
    
    let working_dir = args.get("working_directory")
        .and_then(|wd| wd.as_str());
    
    let environment: HashMap<String, String> = args.get("environment")
        .and_then(|env| env.as_object())
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    
    // Determine the shell to use based on platform
    #[cfg(target_os = "windows")]
    let (shell, shell_arg) = ("cmd", "/C");
    
    #[cfg(not(target_os = "windows"))]
    let (shell, shell_arg) = ("sh", "-c");
    
    // Build the command
    let mut cmd = Command::new(shell);
    cmd.arg(shell_arg);
    cmd.arg(command_str);
    
    // Set working directory if specified
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    
    // Add environment variables
    for (key, value) in environment {
        cmd.env(key, value);
    }
    
    // Capture stdout and stderr
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    
    // Execute the command
    match cmd.output() {
        Ok(output) => {
            let result = CommandResult {
                exit_code: output.status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                success: output.status.success(),
            };
            
            send_response(stdout, id, Some(json!(result)), None)
        },
        Err(e) => send_error(stdout, id, -32000, &format!("Failed to execute shell command: {}", e)),
    }
}

// Resource getters
fn get_os_info(_params: Option<Value>, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let os_type = "Linux";
    #[cfg(target_os = "windows")]
    let os_type = "Windows";
    #[cfg(target_os = "macos")]
    let os_type = "macOS";
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let os_type = "Unknown";
    
    // Get more detailed OS info
    let os_release = std::process::Command::new("uname")
        .arg("-r")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|_| "Unknown".to_string());
    
    let os_version = std::process::Command::new("uname")
        .arg("-v")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|_| "Unknown".to_string());
    
    send_response(
        stdout,
        id,
        Some(json!({
            "os_type": os_type,
            "os_release": os_release,
            "os_version": os_version,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        })),
        None,
    )
}

fn get_environment_variables(_params: Option<Value>, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let env_vars: HashMap<String, String> = std::env::vars().collect();
    
    send_response(stdout, id, Some(json!(env_vars)), None)
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