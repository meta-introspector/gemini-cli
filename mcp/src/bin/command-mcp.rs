use log::{debug, error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::error::Error;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::signal;
use tokio::process::Command as AsyncCommand;

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

// --- Functions for command execution ---
async fn execute_command(
    cmd: &str, 
    args: &[String], 
    working_dir: Option<&str>,
    timeout_secs: Option<u64>,
) -> Result<Value, Box<dyn Error + Send + Sync>> {
    info!("Executing command: {} {:?}", cmd, args);
    
    let mut command = AsyncCommand::new(cmd);
    command.args(args);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    
    if let Some(dir) = working_dir {
        command.current_dir(dir);
    }
    
    let mut child = command.spawn()?;
    
    let stdout_handle = child.stdout.take().expect("Child process stdout handle missing");
    let stderr_handle = child.stderr.take().expect("Child process stderr handle missing");
    
    let stdout_reader = BufReader::new(stdout_handle);
    let stderr_reader = BufReader::new(stderr_handle);
    
    let mut stdout_lines = Vec::new();
    let mut stderr_lines = Vec::new();
    
    // Create background tasks to read stdout and stderr
    let stdout_task = tokio::spawn(async move {
        let mut lines = Vec::new();
        let mut reader = stdout_reader;
        let mut line = String::new();
        
        while let Ok(bytes) = reader.read_line(&mut line).await {
            if bytes == 0 {
                break; // EOF
            }
            lines.push(line.clone());
            line.clear();
        }
        
        lines
    });
    
    let stderr_task = tokio::spawn(async move {
        let mut lines = Vec::new();
        let mut reader = stderr_reader;
        let mut line = String::new();
        
        while let Ok(bytes) = reader.read_line(&mut line).await {
            if bytes == 0 {
                break; // EOF
            }
            lines.push(line.clone());
            line.clear();
        }
        
        lines
    });
    
    // Set up timeout if specified
    let status = if let Some(timeout) = timeout_secs {
        match tokio::time::timeout(std::time::Duration::from_secs(timeout), child.wait()).await {
            Ok(result) => result?,
            Err(_) => {
                // Timeout occurred
                child.kill().await?;
                return Err("Command execution timed out".into());
            }
        }
    } else {
        child.wait().await?
    };
    
    // Collect output from the background tasks
    stdout_lines = stdout_task.await?;
    stderr_lines = stderr_task.await?;
    
    let stdout = stdout_lines.join("");
    let stderr = stderr_lines.join("");
    
    Ok(json!({
        "exit_code": status.code(),
        "stdout": stdout,
        "stderr": stderr,
        "success": status.success()
    }))
}

// --- Handler for tool execute requests ---
async fn handle_tool_execute(params: Value) -> Result<Value, Box<dyn Error + Send + Sync>> {
    let tool_name = params["name"]
        .as_str()
        .ok_or("Missing 'name' field in params")?;
    let arguments = params["arguments"].clone();

    match tool_name {
        "execute_command" => {
            let cmd = arguments["command"]
                .as_str()
                .ok_or("Missing 'command' field in arguments")?;
            
            let args = arguments["args"].as_array()
                .map(|arr| arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<String>>())
                .unwrap_or_default();
            
            let working_dir = arguments["working_dir"].as_str();
            
            let timeout_secs = arguments["timeout_secs"].as_u64();
            
            execute_command(cmd, &args, working_dir, timeout_secs).await
        }
        "get_environment_variable" => {
            let var_name = arguments["name"]
                .as_str()
                .ok_or("Missing 'name' field in arguments")?;
            
            match std::env::var(var_name) {
                Ok(value) => Ok(json!({
                    "value": value,
                    "exists": true
                })),
                Err(_) => Ok(json!({
                    "value": null,
                    "exists": false
                }))
            }
        }
        _ => {
            Err(format!("Unknown tool: {}", tool_name).into())
        }
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
                name: "command-mcp".to_string(),
                version: "1.0.0".to_string(),
            };

            let tools = vec![
                Tool {
                    name: "execute_command".to_string(),
                    description: "Executes a shell command and returns its output".to_string(),
                    schema: None,
                },
                Tool {
                    name: "get_environment_variable".to_string(),
                    description: "Gets the value of an environment variable".to_string(),
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
    
    info!("Starting command MCP server...");

    let mut stdout = BufWriter::new(tokio::io::stdout());
    let mut stdin = BufReader::new(tokio::io::stdin());

    // Send early init response to avoid timeout
    let server_info = ServerInfo {
        name: "command-mcp".to_string(),
        version: "1.0.0".to_string(),
    };

    let tools = vec![
        Tool {
            name: "execute_command".to_string(),
            description: "Executes a shell command and returns its output".to_string(),
            schema: None,
        },
        Tool {
            name: "get_environment_variable".to_string(),
            description: "Gets the value of an environment variable".to_string(),
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

    info!("Command MCP server shutting down.");
    Ok(())
} 