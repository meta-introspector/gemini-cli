use std::io::{self, BufRead, Read, Write};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::process::{self, Command, Stdio};
use std::collections::HashMap;
use std::error::Error;
use crate::mcp::rpc::{Request, Response, JsonRpcError, InitializeResult, ServerInfo, ServerCapabilities, Tool, Resource};

// Define basic JSON-RPC structures - TODO: Move to shared rpc module

// MCP server capabilities - TODO: Move to shared rpc module

// Command execution result
#[derive(Serialize, Deserialize, Debug)]
struct CommandResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
    success: bool,
}

/// Run the application as a command MCP server
pub async fn run() -> Result<(), Box<dyn Error>> {
    println!("Starting command MCP server...");

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
                                            name: "command-mcp".to_string(),
                                            version: "1.0.0".to_string(),
                                        };

                                        // Define tools
                                        let tools = vec![
                                            Tool {
                                                name: "execute_command".to_string(),
                                                description: Some("Executes a system command and returns the result".to_string()),
                                                parameters: Some(json!({
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
                                                        }
                                                    },
                                                    "required": ["command"]
                                                })),
                                            },
                                            Tool {
                                                name: "execute_shell".to_string(),
                                                description: Some("Executes a shell command using the default system shell".to_string()),
                                                parameters: Some(json!({
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
                                                description: Some("Gets information about the operating system".to_string()),
                                            },
                                            Resource {
                                                name: "environment_variables".to_string(),
                                                description: Some("Gets the current environment variables".to_string()),
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

                                            // Process the appropriate tool
                                            match tool_name {
                                                "execute_command" => {
                                                    // Get command parameters
                                                    let command_str = arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
                                                    if command_str.is_empty() {
                                                        let error = JsonRpcError {
                                                            code: -32602,
                                                            message: "Missing required parameter: command".to_string(),
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
                                                        continue;
                                                    }

                                                    let arguments_arr = arguments.get("arguments").and_then(|a| a.as_array());
                                                    let working_dir = arguments.get("working_directory").and_then(|v| v.as_str());
                                                    let environment = arguments.get("environment").and_then(|v| v.as_object());

                                                    // Build the command
                                                    let mut cmd = Command::new(command_str);

                                                    // Add arguments if provided
                                                    if let Some(args) = arguments_arr {
                                                        for arg in args {
                                                            if let Some(arg_str) = arg.as_str() {
                                                                cmd.arg(arg_str);
                                                            }
                                                        }
                                                    }

                                                    // Set working directory if specified
                                                    if let Some(dir) = working_dir {
                                                        cmd.current_dir(dir);
                                                    }

                                                    // Add environment variables if provided
                                                    if let Some(env) = environment {
                                                        for (key, value) in env {
                                                            if let Some(value_str) = value.as_str() {
                                                                cmd.env(key, value_str);
                                                            }
                                                        }
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

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!(result)),
                                                                error: None,
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();
                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        Err(e) => {
                                                            let error = JsonRpcError {
                                                                code: -32000,
                                                                message: format!("Failed to execute command: {}", e),
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
                                                "execute_shell" => {
                                                    // Get command parameters
                                                    let command_str = arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
                                                    if command_str.is_empty() {
                                                        let error = JsonRpcError {
                                                            code: -32602,
                                                            message: "Missing required parameter: command".to_string(),
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
                                                        continue;
                                                    }

                                                    let working_dir = arguments.get("working_directory").and_then(|v| v.as_str());
                                                    let environment = arguments.get("environment").and_then(|v| v.as_object());

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

                                                    // Add environment variables if provided
                                                    if let Some(env) = environment {
                                                        for (key, value) in env {
                                                            if let Some(value_str) = value.as_str() {
                                                                cmd.env(key, value_str);
                                                            }
                                                        }
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

                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!(result)),
                                                                error: None,
                                                            };

                                                            let response_json = serde_json::to_string(&response).unwrap();
                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        Err(e) => {
                                                            let error = JsonRpcError {
                                                                code: -32000,
                                                                message: format!("Failed to execute shell command: {}", e),
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
                                                        message: format!("Tool not found: {}", tool_name),
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
                                            // Missing params
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
                                    "mcp/resource/get" => {
                                        // Handle resource request
                                        if let Some(params) = request.params {
                                            let resource_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

                                            match resource_name {
                                                "os_info" => {
                                                    // Get OS information
                                                    #[cfg(target_os = "linux")]
                                                    let os_type = "Linux";
                                                    #[cfg(target_os = "windows")]
                                                    let os_type = "Windows";
                                                    #[cfg(target_os = "macos")]
                                                    let os_type = "macOS";
                                                    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
                                                    let os_type = "Unknown";

                                                    // Get more detailed OS info
                                                    let os_release = process::Command::new("uname")
                                                        .arg("-r")
                                                        .output()
                                                        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                                                        .unwrap_or_else(|_| "Unknown".to_string());

                                                    let os_version = process::Command::new("uname")
                                                        .arg("-v")
                                                        .output()
                                                        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                                                        .unwrap_or_else(|_| "Unknown".to_string());

                                                    let response = Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!({
                                                            "os_type": os_type,
                                                            "os_release": os_release,
                                                            "os_version": os_version,
                                                            "arch": std::env::consts::ARCH,
                                                            "family": std::env::consts::FAMILY,
                                                        })),
                                                        error: None,
                                                    };

                                                    let response_json = serde_json::to_string(&response).unwrap();
                                                    let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                    stdout.write_all(header.as_bytes()).unwrap();
                                                    stdout.write_all(response_json.as_bytes()).unwrap();
                                                    stdout.flush().unwrap();
                                                },
                                                "environment_variables" => {
                                                    // Get environment variables
                                                    let env_vars: HashMap<String, String> = std::env::vars().collect();

                                                    let response = Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!(env_vars)),
                                                        error: None,
                                                    };

                                                    let response_json = serde_json::to_string(&response).unwrap();
                                                    let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                    stdout.write_all(header.as_bytes()).unwrap();
                                                    stdout.write_all(response_json.as_bytes()).unwrap();
                                                    stdout.flush().unwrap();
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
                                    "shutdown" => {
                                        // Just acknowledge shutdown
                                        let response = Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: request.id.unwrap_or(json!(null)),
                                            result: Some(json!({})),
                                            error: None,
                                        };

                                        let response_json = serde_json::to_string(&response).unwrap();
                                        let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                        stdout.write_all(header.as_bytes()).unwrap();
                                        stdout.write_all(response_json.as_bytes()).unwrap();
                                        stdout.flush().unwrap();
                                    },
                                    "exit" => {
                                        // Exit the process
                                        return Ok(());
                                    },
                                    _ => {
                                        // Unknown method
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
                                // JSON-RPC parse error
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