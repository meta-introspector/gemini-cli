// Model Control Protocol - Command Module
// This implements the command-executing MCP server that runs shell commands

use log::{debug, error};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Read, Write};
use std::process::{Command, Stdio};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::io::{AsyncRead, AsyncWrite};

// JSON-RPC 2.0 structures
#[derive(Serialize, Deserialize, Debug, Clone)]
struct Request {
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Response {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Notification {
    jsonrpc: String,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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

// Command execution result
#[derive(Serialize, Deserialize, Debug)]
struct CommandResult {
    exit_code: i32,
    stdout: String,
    stderr: String,
    success: bool,
}

// Restore local definitions
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

/// Server for handling commands from stdin and writing responses to stdout.
/// This is used by the communication with the web UI.
pub struct CommandServer {}

impl Default for CommandServer {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandServer {
    pub fn new() -> Self {
        CommandServer {}
    }

    pub async fn run(
        self,
        stdin: impl AsyncRead + Unpin,
        mut stdout: impl AsyncWrite + Unpin,
    ) -> Result<(), String> {
        let mut buffer = Vec::new();
        let mut reader = tokio::io::BufReader::new(stdin);
        let mut line_buffer = String::new();
        let mut shutdown_requested = false;

        loop {
            if shutdown_requested {
                debug!("Shutdown requested, exiting loop.");
                break;
            }

            // Step 1: Read headers until empty line
            let mut content_length: Option<usize> = None;

            loop {
                line_buffer.clear();
                match reader.read_line(&mut line_buffer).await {
                    Ok(0) => return Ok(()), // End of input
                    Ok(_) => {
                        let line_trimmed = line_buffer.trim();

                        if line_trimmed.is_empty() {
                            // End of headers
                            break;
                        } else if line_trimmed.starts_with("Content-Length:") {
                            // Parse Content-Length header
                            let parts: Vec<&str> = line_trimmed.split(':').collect();
                            if parts.len() == 2 {
                                let len_str = parts[1].trim();
                                debug!("Parsing Content-Length value: '{}'", len_str);
                                match len_str.parse::<usize>() {
                                    Ok(len) => {
                                        debug!("Successfully parsed Content-Length: {}", len);
                                        content_length = Some(len);
                                    }
                                    Err(e) => {
                                        error!("Invalid Content-Length value '{}': {}", len_str, e);
                                        let error_resp = format_error_response(
                                            0,
                                            format!("Invalid Content-Length: {}", e),
                                        );
                                        // Use self.write_response which is async
                                        self.write_response(&mut stdout, &error_resp).await?;
                                        continue; // Continue reading headers after error
                                    }
                                }
                            }
                        } else {
                            // Ignore other headers
                            debug!("Ignoring header: {}", line_trimmed);
                        }
                    }
                    Err(e) => return Err(format!("Error reading header from stdin: {}", e)),
                }
            }

            // Step 2: Read exact content bytes according to Content-Length
            if let Some(length) = content_length {
                debug!("Reading {} bytes of content", length);
                buffer.resize(length, 0);
                match reader.read_exact(&mut buffer).await {
                    Ok(_) => {
                        let json_str = String::from_utf8_lossy(&buffer);
                        debug!("Received JSON content: {}", json_str);

                        // Peek at the method to check for exit/shutdown before processing
                        let request_peek: Result<Request, _> = serde_json::from_str(&json_str);
                        let mut is_shutdown = false;

                        if let Ok(ref req) = request_peek {
                            if req.method == "exit" {
                                debug!("Received exit notification, exiting loop.");
                                break; // Exit immediately for notification
                            }
                            if req.method == "shutdown" {
                                is_shutdown = true;
                                debug!("Shutdown method detected, will exit after response.");
                            }
                        }

                        // Process the message and get response
                        let response_result = self.process_message(&json_str).await;

                        match response_result {
                            Ok(Some(resp)) => {
                                // Write response
                                self.write_response(&mut stdout, &resp).await?;

                                // If it was a shutdown request, set the flag to exit next iteration
                                if is_shutdown {
                                    shutdown_requested = true;
                                    debug!("Shutdown response sent, flag set.");
                                    continue; // Go to next loop iteration to check flag
                                }
                            }
                            Ok(None) => {
                                // This case should now only be triggered by internal logic, not exit/shutdown directly
                                debug!("process_message requested exit, breaking loop.");
                                break;
                            }
                            Err(e) => {
                                // Error during processing, format and send error response
                                let error_resp = format_error_response(0, e); // Use default ID 0 for processing errors
                                self.write_response(&mut stdout, &error_resp).await?;
                            }
                        }
                    }
                    Err(e) => return Err(format!("Error reading message content: {}", e)),
                }
            } else {
                // No Content-Length header found
                error!("No Content-Length header found");
                let error_resp =
                    format_error_response(0, "No Content-Length header found".to_string());
                self.write_response(&mut stdout, &error_resp).await?;
            }
        }

        Ok(())
    }

    // Helper to write response with proper Content-Length framing
    async fn write_response(
        &self,
        stdout: &mut (impl AsyncWrite + Unpin),
        response: &str,
    ) -> Result<(), String> {
        let message = format!("Content-Length: {}\r\n\r\n{}", response.len(), response);
        debug!("Writing response: {} bytes", message.len());

        // Write in steps with detailed error handling
        match stdout.write_all(message.as_bytes()).await {
            Ok(_) => debug!("Response bytes written"),
            Err(e) => {
                error!("Failed to write response: {}", e);
                return Err(format!("Failed to write response: {}", e));
            }
        }

        match stdout.flush().await {
            Ok(_) => {
                debug!("Response flushed successfully");
                Ok(())
            }
            Err(e) => {
                error!("Failed to flush response: {}", e);
                Err(format!("Failed to flush response: {}", e))
            }
        }
    }

    async fn process_message(&self, message: &str) -> Result<Option<String>, String> {
        // Parse the JSON-RPC request
        let request: Request = match serde_json::from_str(message) {
            Ok(req) => req,
            Err(e) => {
                return Ok(Some(format_error_response(
                    0, // Use default ID 0 for parsing errors
                    format!("Invalid JSON-RPC request: {}", e),
                )));
            }
        };

        // Store the id value for later use
        let id_value = match &request.id {
            Some(val) => val.clone(),
            None => json!(null),
        };

        // Create a buffer for the response
        let mut buffer = Vec::new();

        // Handle the request method
        // Shutdown and Exit are now handled in the main loop
        let result = match request.method.as_str() {
            "initialize" => handle_initialize(request, &mut buffer).await,
            "shutdown" => {
                // Prepare shutdown response in buffer
                handle_shutdown(request, &mut buffer)
            }
            "tool/execute" => handle_execute_tool(request, &mut buffer).await,
            "resource/get" => handle_get_resource(request, &mut buffer).await,
            "exit" => {
                // Should not be reached if main loop handles it, but return error if it does
                error!("'exit' method reached process_message unexpectedly.");
                Err("Exit notification should be handled by main loop".to_string())
            }
            _ => send_error(
                &mut buffer,
                id_value.clone(),
                -32601,
                &format!("Method not found: {}", request.method),
            ),
        };

        // Convert the buffer to a string if successful
        match result {
            Ok(()) => {
                let response_str = String::from_utf8(buffer)
                    .map_err(|e| format!("Failed to convert response to string: {}", e))?;
                Ok(Some(response_str))
            }
            Err(e) => {
                // Don't format an error response here, just propagate the error string
                Err(e)
            }
        }
    }
}

// Helper function to format error responses
fn format_error_response(id: u64, message: String) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32000,
            "message": message
        }
    })
    .to_string()
}

async fn handle_initialize(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    // Define server capabilities
    let server_info = ServerInfo {
        name: "command-mcp".to_string(),
        version: "1.0.0".to_string(),
    };

    // Define tools with updated schemas
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
                        "description": "Directory to run the command in (optional)",
                        "default": null
                    },
                    "environment_variables": {
                        "type": "object",
                        "additionalProperties": {
                            "type": "string"
                        },
                        "description": "Environment variables to set for the command",
                        "default": {}
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
            description: "Executes a command within a shell (like bash or sh)".to_string(),
            schema: Some(json!({
                "type": "object",
                "properties": {
                    "command_line": {
                        "type": "string",
                        "description": "The full command line to execute in the shell"
                    },
                    "shell_path": {
                        "type": "string",
                        "description": "Path to the shell executable (e.g., /bin/bash)",
                        "default": "/bin/sh"
                    },
                    "working_directory": {
                        "type": "string",
                        "description": "Directory to run the command in (optional)",
                        "default": null
                    },
                    "environment_variables": {
                        "type": "object",
                        "additionalProperties": {
                            "type": "string"
                        },
                        "description": "Environment variables to set for the command",
                        "default": {}
                    },
                    "timeout_ms": {
                        "type": "integer",
                        "description": "Maximum execution time in milliseconds (0 for no timeout)",
                        "default": 0
                    }
                },
                "required": ["command_line"]
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
            description: "Gets all environment variables".to_string(),
            schema: None,
        },
    ];

    let capabilities = ServerCapabilities { tools, resources };

    let result = InitializeResult {
        server_info,
        capabilities,
    };

    send_response(
        stdout,
        request.id.unwrap_or(json!(null)),
        Some(json!(result)),
        None,
    )
}

// Function for handling shutdown request
fn handle_shutdown(request: Request, stdout: &mut impl Write) -> Result<(), String> {
    debug!("Handling shutdown request with ID: {:?}", request.id);

    // Ensure we write a proper response with null result
    let response = Response {
        jsonrpc: "2.0".to_string(),
        id: request.id.unwrap_or(json!(null)),
        result: Some(json!(null)),
        error: None,
    };

    let json_str = serde_json::to_string(&response)
        .map_err(|e| format!("Failed to serialize shutdown response: {}", e))?;

    let message = format!("Content-Length: {}\r\n\r\n{}", json_str.len(), json_str);

    // More verbose logging to debug issues
    debug!("Sending shutdown response: {}", json_str);

    // Write and flush in separate steps for better error handling
    match stdout.write_all(message.as_bytes()) {
        Ok(_) => debug!("Shutdown response bytes written successfully"),
        Err(e) => return Err(format!("Failed to write shutdown response: {}", e)),
    }

    match stdout.flush() {
        Ok(_) => debug!("Shutdown response flushed successfully"),
        Err(e) => return Err(format!("Failed to flush shutdown response: {}", e)),
    }

    debug!("Shutdown response sent successfully");
    Ok(())
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
        }
    };

    match params.tool_name.as_str() {
        "execute_command" => {
            execute_command(params.arguments, request.id.unwrap_or(json!(null)), stdout).await
        }
        "execute_shell" => {
            execute_shell(params.arguments, request.id.unwrap_or(json!(null)), stdout).await
        }
        _ => send_error(
            stdout,
            request.id.unwrap_or(json!(null)),
            -32601,
            &format!("Tool not found: {}", params.tool_name),
        ),
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
        }
    };

    match params.name.as_str() {
        "os_info" => get_os_info(params.params, request.id.unwrap_or(json!(null)), stdout).await,
        "environment_variables" => {
            get_environment_variables(params.params, request.id.unwrap_or(json!(null)), stdout)
                .await
        }
        _ => send_error(
            stdout,
            request.id.unwrap_or(json!(null)),
            -32601,
            &format!("Resource not found: {}", params.name),
        ),
    }
}

// Tool implementations
async fn execute_command(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let command_str = args["command"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'command' argument".to_string())?;
    let arguments: Vec<String> = args["arguments"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let working_dir = args["working_directory"].as_str();
    let env_vars: HashMap<String, String> = args["environment_variables"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let mut cmd = Command::new(command_str);
    cmd.args(&arguments);
    cmd.envs(env_vars);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(child) => {
            match child.wait_with_output() {
                Ok(output) => {
                    let result = CommandResult {
                        exit_code: output.status.code().unwrap_or(-1), // Handle cases where status code is not available
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                        success: output.status.success(),
                    };
                    send_response(stdout, id, Some(json!(result)), None)
                }
                Err(e) => send_error(
                    stdout,
                    id,
                    -32000,
                    &format!("Failed to read command output: {}", e),
                ),
            }
        }
        Err(e) => send_error(
            stdout,
            id,
            -32001,
            &format!("Failed to execute command '{}': {}", command_str, e),
        ),
    }
}

async fn execute_shell(args: Value, id: Value, stdout: &mut impl Write) -> Result<(), String> {
    let command_line = args["command_line"]
        .as_str()
        .ok_or_else(|| "Missing or invalid 'command_line' argument".to_string())?;
    let shell_path = args["shell_path"].as_str().unwrap_or("/bin/sh");
    let working_dir = args["working_directory"].as_str();
    let env_vars: HashMap<String, String> = args["environment_variables"]
        .as_object()
        .map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let mut cmd = Command::new(shell_path);
    cmd.arg("-c"); // Tell the shell to execute the following string
    cmd.arg(command_line);
    cmd.envs(env_vars);
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    match cmd.spawn() {
        Ok(child) => match child.wait_with_output() {
            Ok(output) => {
                let result = CommandResult {
                    exit_code: output.status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    success: output.status.success(),
                };
                send_response(stdout, id, Some(json!(result)), None)
            }
            Err(e) => send_error(
                stdout,
                id,
                -32000,
                &format!("Failed to read shell output: {}", e),
            ),
        },
        Err(e) => send_error(
            stdout,
            id,
            -32001,
            &format!("Failed to execute shell command: {}", e),
        ),
    }
}

// Resource implementations
async fn get_os_info(
    _params: Option<Value>,
    id: Value,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let info = os_info::get();

    let result = json!({
        "os_type": info.os_type().to_string(),
        "version": info.version().to_string(),
        "bitness": info.bitness().to_string(),
        "architecture": info.architecture().map(|a| a.to_string()),
        "hostname": hostname::get().ok().map(|h| h.to_string_lossy().to_string()).unwrap_or_else(|| "unknown".to_string()),
    });

    send_response(stdout, id, Some(result), None)
}

async fn get_environment_variables(
    _params: Option<Value>,
    id: Value,
    stdout: &mut impl Write,
) -> Result<(), String> {
    let env_map: HashMap<String, String> = std::env::vars().collect();
    send_response(stdout, id, Some(json!(env_map)), None)
}

// Non-async version of send_response since it doesn't have any async operations
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
    let json_str = serde_json::to_string(&response)
        .map_err(|e| format!("Failed to serialize response: {}", e))?;
    let message = format!("Content-Length: {}\r\n\r\n{}", json_str.len(), json_str);
    stdout
        .write_all(message.as_bytes())
        .map_err(|e| format!("Failed to write response: {}", e))?;
    stdout
        .flush()
        .map_err(|e| format!("Failed to flush stdout: {}", e))?;
    Ok(())
}

// Non-async version of send_error
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

// Add a parameterless run function to match the filesystem and memory_store implementation
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    debug!("Command MCP server starting...");

    // Send immediate initialization response
    {
        let mut stdout = io::stdout();
        let server_info = ServerInfo {
            name: "command-mcp".to_string(),
            version: "1.0.0".to_string(),
        };
        let tools = vec![
            Tool {
                name: "execute_command".to_string(),
                description: "Executes a system command and returns the result".to_string(),
                schema: None, // Simplified for early response
            },
            Tool {
                name: "execute_shell".to_string(),
                description: "Executes a command within a shell".to_string(),
                schema: None, // Simplified for early response
            },
        ];
        let resources = vec![
            Resource {
                name: "os_info".to_string(),
                description: "Gets OS info".to_string(),
                schema: None,
            },
            Resource {
                name: "environment_variables".to_string(),
                description: "Gets env vars".to_string(),
                schema: None,
            },
        ];
        let capabilities = ServerCapabilities { tools, resources };

        let response = Response {
            jsonrpc: "2.0".to_string(),
            id: json!(2), // Use the typical ID for command server if known
            result: Some(json!({
                "serverInfo": server_info,
                "capabilities": capabilities,
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
                debug!(
                    "Sending early init response ({} bytes)",
                    response_json.len()
                );
                if let Err(e) = stdout.write_all(message.as_bytes()) {
                    error!("Failed to write early init response: {}", e);
                }
                if let Err(e) = stdout.flush() {
                    error!("Failed to flush early init response: {}", e);
                }
                debug!("Early init response sent.");
            }
            Err(e) => {
                error!("Failed to serialize early init response: {}", e);
            }
        }
    }

    // Apply a synchronous I/O approach for simpler implementation
    process_jsonrpc().await
}

async fn process_jsonrpc() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize executor
    let cmd_executor = CommandExecutor::new();

    let stdin = io::stdin();
    let mut stdin_reader = io::BufReader::new(stdin);
    let mut line = String::new();
    let mut stdout = io::stdout();

    debug!("Command MCP server started, waiting for messages...");

    // Main message processing loop
    'outer: loop {
        // Read headers
        let mut content_length: Option<usize> = None;
        'headers: loop {
            line.clear();
            match stdin_reader.read_line(&mut line) {
                Ok(0) => {
                    // EOF - exit gracefully
                    debug!("Reached EOF on stdin");
                    break 'outer;
                }
                Ok(_) => {
                    let line_trimmed = line.trim();

                    // Empty line indicates end of headers
                    if line_trimmed.is_empty() {
                        debug!("End of headers");
                        break 'headers;
                    } else if line_trimmed.starts_with("Content-Length:") {
                        // Parse Content-Length header
                        if let Some(len_str) = line_trimmed.strip_prefix("Content-Length:") {
                            let len_str = len_str.trim();

                            debug!("Parsing Content-Length value: '{}'", len_str);

                            // Extract only the numeric part
                            let numeric_part = len_str
                                .chars()
                                .take_while(|c| c.is_ascii_digit())
                                .collect::<String>();

                            match numeric_part.parse::<usize>() {
                                Ok(len) => {
                                    debug!("Successfully parsed Content-Length: {}", len);
                                    content_length = Some(len);
                                }
                                Err(e) => {
                                    error!("Invalid Content-Length value '{}': {}", len_str, e);
                                    let error_resp = format_error_response(
                                        0,
                                        format!("Invalid Content-Length: {}", e),
                                    );
                                    write_response(&mut stdout, error_resp).await?;
                                    // Continue processing other headers
                                }
                            }
                        }
                    }
                    // Possibly handle other headers here
                }
                Err(e) => {
                    error!("Failed to read from stdin: {}", e);
                    return Err(e.into());
                }
            }
        }

        // Step 2: Read exact content bytes according to Content-Length
        if let Some(length) = content_length {
            debug!("Reading {} bytes of content", length);

            // Allocate a buffer of the right size
            let mut buffer = vec![0; length];
            let mut total_read = 0;

            // Read exactly length bytes
            while total_read < length {
                match stdin_reader.read(&mut buffer[total_read..]) {
                    Ok(0) => {
                        error!("Unexpected EOF while reading content");
                        break 'outer;
                    }
                    Ok(n) => {
                        total_read += n;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => {
                        error!("Error reading from stdin: {}", e);
                        return Err(e.into());
                    }
                }
            }

            // Convert to string and process
            match String::from_utf8(buffer) {
                Ok(content) => {
                    debug!("Received message: {}", content);

                    // Process the JSON-RPC message
                    match process_message(&content, &cmd_executor, &mut stdout).await {
                        Ok(_) => {}
                        Err(e) => {
                            error!("Error processing message: {}", e);
                            let error_resp = format_error_response(
                                0,
                                format!("Message processing error: {}", e),
                            );
                            write_response(&mut stdout, error_resp).await?;
                        }
                    }
                }
                Err(e) => {
                    error!("Invalid UTF-8 in message content: {}", e);
                    let error_resp =
                        format_error_response(0, format!("Invalid UTF-8 content: {}", e));
                    write_response(&mut stdout, error_resp).await?;
                }
            }
        } else {
            // No Content-Length header found
            error!("No Content-Length header found");
            let error_resp = format_error_response(0, "No Content-Length header found".to_string());
            write_response(&mut stdout, error_resp).await?;
        }
    }

    debug!("Command MCP server shutting down");
    Ok(())
}

// Helper to write response with proper Content-Length framing
async fn write_response(
    stdout: &mut impl Write,
    response: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let message = format!("Content-Length: {}\r\n\r\n{}", response.len(), response);
    debug!("Sending response (length: {})", response.len());
    stdout.write_all(message.as_bytes())?;
    stdout.flush()?;
    Ok(())
}

async fn process_message(
    content: &str,
    executor: &CommandExecutor,
    stdout: &mut impl Write,
) -> Result<(), String> {
    // Parse the JSON-RPC message
    let request: serde_json::Value =
        serde_json::from_str(content).map_err(|e| format!("Failed to parse JSON: {}", e))?;

    // Extract request ID and method
    let id = request.get("id").cloned().unwrap_or(json!(null));

    // Check for method field
    if let Some(method) = request.get("method").and_then(|m| m.as_str()) {
        debug!("Processing method: {}", method);

        match method {
            "initialize" => {
                debug!("Received initialize request");

                // Send immediate response for initialization with proper format
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "serverInfo": {
                            "name": "command-mcp",
                            "version": "1.0.0"
                        },
                        "capabilities": {
                            "tools": [
                                {
                                    "name": "execute_command",
                                    "description": "Executes a shell command and returns its output",
                                    "schema": {
                                        "type": "object",
                                        "properties": {
                                            "command": {
                                                "type": "string",
                                                "description": "The command to execute"
                                            },
                                            "cwd": {
                                                "type": "string",
                                                "description": "Working directory for command execution"
                                            },
                                            "timeout_ms": {
                                                "type": "integer",
                                                "description": "Timeout in milliseconds (0 for no timeout)"
                                            },
                                            "env": {
                                                "type": "object",
                                                "description": "Environment variables to set"
                                            }
                                        },
                                        "required": ["command"]
                                    }
                                }
                            ],
                            "resources": []
                        },
                        "status": "initialized"
                    }
                });

                let response_str = serde_json::to_string(&response)
                    .map_err(|e| format!("Failed to serialize response: {}", e))?;

                write_response(stdout, response_str)
                    .await
                    .map_err(|e| format!("Failed to write response: {}", e))?;

                return Ok(());
            }
            "shutdown" => {
                debug!("Received shutdown request");

                // Send acknowledgment for shutdown
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {}
                });

                let response_str = serde_json::to_string(&response)
                    .map_err(|e| format!("Failed to serialize response: {}", e))?;

                write_response(stdout, response_str)
                    .await
                    .map_err(|e| format!("Failed to write response: {}", e))?;

                return Ok(());
            }
            "exit" => {
                debug!("Received exit notification, terminating");
                // Just exit normally, no response needed for notifications
                return Ok(());
            }
            "tool/execute" => {
                debug!("Received tool/execute request");

                // Extract tool details
                if let Some(params) = request.get("params") {
                    if let Some(tool_name) = params.get("tool_name").and_then(|t| t.as_str()) {
                        if let Some(arguments) = params.get("arguments") {
                            match tool_name {
                                "execute_command" => {
                                    if let Some(cmd) =
                                        arguments.get("command").and_then(|c| c.as_str())
                                    {
                                        debug!("Executing command: {}", cmd);

                                        let cwd = arguments.get("cwd").and_then(|c| c.as_str());
                                        let timeout =
                                            arguments.get("timeout_ms").and_then(|t| t.as_u64());
                                        let env = arguments.get("env");

                                        match executor.execute_command(cmd, cwd, env, timeout).await
                                        {
                                            Ok(result) => {
                                                let response = json!({
                                                    "jsonrpc": "2.0",
                                                    "id": id,
                                                    "result": result
                                                });

                                                let response_str = serde_json::to_string(&response)
                                                    .map_err(|e| {
                                                        format!(
                                                            "Failed to serialize response: {}",
                                                            e
                                                        )
                                                    })?;

                                                write_response(stdout, response_str)
                                                    .await
                                                    .map_err(|e| {
                                                        format!("Failed to write response: {}", e)
                                                    })?;
                                            }
                                            Err(e) => {
                                                let error_resp = format_error_response(
                                                    id.as_u64().unwrap_or(0),
                                                    format!("Command execution failed: {}", e),
                                                );
                                                write_response(stdout, error_resp).await.map_err(
                                                    |e| {
                                                        format!(
                                                            "Failed to write error response: {}",
                                                            e
                                                        )
                                                    },
                                                )?;
                                            }
                                        }
                                    } else {
                                        let error_resp = format_error_response(
                                            id.as_u64().unwrap_or(0),
                                            "Missing or invalid 'command' parameter".to_string(),
                                        );
                                        write_response(stdout, error_resp).await.map_err(|e| {
                                            format!("Failed to write error response: {}", e)
                                        })?;
                                    }
                                }
                                _ => {
                                    let error_resp = format_error_response(
                                        id.as_u64().unwrap_or(0),
                                        format!("Unknown tool: {}", tool_name),
                                    );
                                    write_response(stdout, error_resp).await.map_err(|e| {
                                        format!("Failed to write error response: {}", e)
                                    })?;
                                }
                            }
                        } else {
                            let error_resp = format_error_response(
                                id.as_u64().unwrap_or(0),
                                "Missing 'arguments' in tool/execute params".to_string(),
                            );
                            write_response(stdout, error_resp)
                                .await
                                .map_err(|e| format!("Failed to write error response: {}", e))?;
                        }
                    } else {
                        let error_resp = format_error_response(
                            id.as_u64().unwrap_or(0),
                            "Missing 'tool_name' in tool/execute params".to_string(),
                        );
                        write_response(stdout, error_resp)
                            .await
                            .map_err(|e| format!("Failed to write error response: {}", e))?;
                    }
                } else {
                    let error_resp = format_error_response(
                        id.as_u64().unwrap_or(0),
                        "Missing params for tool/execute".to_string(),
                    );
                    write_response(stdout, error_resp)
                        .await
                        .map_err(|e| format!("Failed to write error response: {}", e))?;
                }

                return Ok(());
            }
            _ => {
                debug!("Unknown method: {}", method);

                let error_resp = format_error_response(
                    id.as_u64().unwrap_or(0),
                    format!("Method not found: {}", method),
                );
                write_response(stdout, error_resp)
                    .await
                    .map_err(|e| format!("Failed to write error response: {}", e))?;
            }
        }
    } else {
        // No method field found
        error!("No method field in JSON-RPC request");
        let error_resp = format_error_response(
            id.as_u64().unwrap_or(0),
            "No method field in JSON-RPC request".to_string(),
        );
        write_response(stdout, error_resp)
            .await
            .map_err(|e| format!("Failed to write error response: {}", e))?;
    }

    Ok(())
}

// Command executor handles actual system command execution
struct CommandExecutor {}

impl CommandExecutor {
    fn new() -> Self {
        CommandExecutor {}
    }

    // Execute a command and return its result
    async fn execute_command(
        &self,
        command: &str,
        cwd: Option<&str>,
        _env: Option<&Value>,
        _timeout_ms: Option<u64>,
    ) -> Result<CommandResult, String> {
        // Command execution logic
        // This is a simplified implementation
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);

        // Set working directory if provided
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        // Capture stdout and stderr
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Execute the command
        let output = cmd
            .output()
            .map_err(|e| format!("Failed to execute command: {}", e))?;

        // Convert output to strings
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(CommandResult {
            exit_code: output.status.code().unwrap_or(-1),
            stdout,
            stderr,
            success: output.status.success(),
        })
    }
}
