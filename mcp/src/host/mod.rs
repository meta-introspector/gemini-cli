mod active_server;
mod io;
mod message_handler;
pub(crate) mod types;

// Use types from the module
use self::types::ActiveServer;

// Main host implementation
use crate::config::{McpServerConfig, McpTransport};
// Import specific types from gemini_core
use crate::rpc::{self, create_log_notification};
use async_trait::async_trait; // Needed for trait implementation
use gemini_core::{JsonRpcError, Request, Response, ServerCapabilities}; // Removed RpcTool, Resource - not directly used here?
use gemini_memory::broker::{self as memory_broker, McpHostInterface};
use log::{debug, error, info, warn};
use serde_json::{self, json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

// Need Clone for task spawning
#[derive(Debug, Clone)]
pub struct McpHost {
    servers: Arc<Mutex<HashMap<String, ActiveServer>>>, // Keyed by server name
    next_request_id: Arc<AtomicU64>,                    // Use atomic for thread-safe incrementing
}

impl McpHost {
    pub async fn new(configs: Vec<McpServerConfig>) -> Result<Self, String> {
        if std::env::var("DEBUG").is_ok() {
            println!(
                "Initializing MCP Host with {} server configs",
                configs.len()
            );
        }

        let host = McpHost {
            servers: Arc::new(Mutex::new(HashMap::new())),
            next_request_id: Arc::new(AtomicU64::new(1)), // Start IDs from 1
        };

        let mut init_tasks = Vec::new();
        let mut servers_map = HashMap::new(); // Temp map to build servers before locking

        let mut failed_count = 0;
        for config_orig in configs {
            // Clone the config and immediately destructure it to avoid partial moves
            let McpServerConfig {
                name: server_name,
                enabled: _enabled, // Not directly used in this loop, but needed for destructuring
                transport,
                command,
                args,
                env,
                auto_execute,
            } = config_orig.clone();

            // Create a new config instance from the cloned parts for launching servers
            // This is needed because ActiveServer::launch_* expects a McpServerConfig
            let launch_config = McpServerConfig {
                name: server_name.clone(),
                enabled: _enabled,            // Must match the original
                transport: transport.clone(), // Clone transport for the new instance
                command: command.clone(),
                args: args.clone(),
                env: env.clone(),
                auto_execute: auto_execute.clone(),
            };

            if std::env::var("DEBUG").is_ok() {
                println!(
                    "Starting MCP server '{}' with transport {:?}",
                    server_name, transport
                );
            }

            let init_future = match transport {
                McpTransport::Stdio => {
                    match ActiveServer::launch_stdio(&host.next_request_id, launch_config.clone())
                        .await
                    {
                        Ok((server, init_future)) => {
                            servers_map.insert(server_name.clone(), server);
                            init_future
                        }
                        Err(e) => {
                            eprintln!("MCP Server '{}' initialization failed: {}", server_name, e);
                            failed_count += 1;
                            continue;
                        }
                    }
                }
                McpTransport::SSE { url, headers } => {
                    match ActiveServer::launch_sse(
                        &host.next_request_id,
                        launch_config.clone(),
                        url.clone(),
                        headers.clone(),
                    )
                    .await
                    {
                        Ok((server, init_future)) => {
                            servers_map.insert(server_name.clone(), server);
                            init_future
                        }
                        Err(e) => {
                            eprintln!("MCP Server '{}' initialization failed: {}", server_name, e);
                            failed_count += 1;
                            continue;
                        }
                    }
                }
                McpTransport::WebSocket { url, headers } => {
                    match ActiveServer::launch_websocket(
                        &host.next_request_id,
                        launch_config.clone(),
                        url.clone(),
                        headers.clone(),
                    )
                    .await
                    {
                        Ok((server, init_future)) => {
                            servers_map.insert(server_name.clone(), server);
                            init_future
                        }
                        Err(e) => {
                            eprintln!("MCP Server '{}' initialization failed: {}", server_name, e);
                            failed_count += 1;
                            continue;
                        }
                    }
                }
            };

            init_tasks.push((server_name, init_future));
        }

        // Wait for all servers to initialize
        let mut init_errors = Vec::new();
        for (server_name, init_future) in init_tasks {
            match init_future.await {
                Ok(result) => {
                    if let Err(_e) = result {
                        eprintln!("Initialization error: Server '{}' init failed", server_name);
                        init_errors.push(format!("Server '{}' init failed", server_name));
                        failed_count += 1;
                    }
                }
                Err(_) => {
                    eprintln!(
                        "Initialization error: Server '{}' init timed out",
                        server_name
                    );
                    init_errors.push(format!("Server '{}' init timed out", server_name));
                    failed_count += 1;
                }
            }
        }

        // Move servers to the host's map
        {
            let mut host_servers = host.servers.lock().await;
            *host_servers = servers_map;
        }

        if failed_count > 0 {
            eprintln!(
                "Warning: {} MCP servers failed to initialize and will be unavailable.",
                failed_count
            );
        }

        Ok(host)
    }

    // Gets combined capabilities from all *initialized* servers
    pub async fn get_all_capabilities(&self) -> ServerCapabilities {
        let servers = self.servers.lock().await;
        let mut combined_caps = ServerCapabilities::default();
        debug!(
            "[DEBUG get_all_capabilities] Checking capabilities for {} servers",
            servers.len()
        );

        for (server_name, server) in servers.iter() {
            debug!(
                "[DEBUG get_all_capabilities] Checking server: {}",
                server_name
            );
            let server_capabilities_lock = server.capabilities.lock().await;
            let maybe_caps = server_capabilities_lock.as_ref();

            if let Some(caps) = maybe_caps {
                debug!(
                    "[DEBUG get_all_capabilities] Found capabilities for server: {}",
                    server_name
                );
                // Prefix tool and resource names with server name to avoid collisions
                for mut tool in caps.tools.iter().cloned() {
                    // Inject server name into tool schema references if present
                    if let Some(_schema) = &mut tool.parameters {
                        // We could preprocess schema to inject namespaces, but that's complex
                        // Just pass the raw schema through for now
                    }

                    // Fully qualify the tool name with server name
                    tool.name = format!("{}/{}", server_name, tool.name);
                    combined_caps.tools.push(tool);
                }

                // Add resources (if any)
                for mut resource in caps.resources.iter().cloned() {
                    // Fully qualify the resource name with server name
                    resource.name = format!("{}/{}", server_name, resource.name);
                    combined_caps.resources.push(resource);
                }
            } else {
                debug!(
                    "[DEBUG get_all_capabilities] No capabilities found for server: {}",
                    server_name
                );
            }
        }

        debug!(
            "[DEBUG get_all_capabilities] Combined: {} tools, {} resources",
            combined_caps.tools.len(),
            combined_caps.resources.len()
        );

        combined_caps
    }

    // Helper to find a server that's initialized
    async fn find_ready_server(
        servers: &Mutex<HashMap<String, ActiveServer>>,
        server_name: &str,
    ) -> Result<ActiveServer, String> {
        let servers_lock = servers.lock().await;

        match servers_lock.get(server_name) {
            Some(server) => {
                let caps = server.capabilities.lock().await;
                if caps.is_some() {
                    // Clone the ActiveServer for out-of-lock use
                    Ok(server.clone())
                } else {
                    Err(format!("Server '{}' is not fully initialized", server_name))
                }
            }
            None => Err(format!("Server '{}' not found", server_name)),
        }
    }

    // Helper method to determine the appropriate timeout for different servers/tools
    fn get_tool_timeout(&self, server_name: &str, tool_name: &str) -> Duration {
        // Check if there's an environment variable for this specific server/tool
        let env_var_name = format!(
            "GEMINI_MCP_TIMEOUT_{}_{}",
            server_name.to_uppercase(),
            tool_name.to_uppercase()
        );
        if let Ok(timeout_str) = std::env::var(&env_var_name) {
            if let Ok(timeout_secs) = timeout_str.parse::<u64>() {
                debug!(
                    "Using custom timeout of {}s for {}/{} from env var {}",
                    timeout_secs, server_name, tool_name, env_var_name
                );
                return Duration::from_secs(timeout_secs);
            }
        }

        // Check if there's an environment variable for this server
        let env_var_name = format!("GEMINI_MCP_TIMEOUT_{}", server_name.to_uppercase());
        if let Ok(timeout_str) = std::env::var(&env_var_name) {
            if let Ok(timeout_secs) = timeout_str.parse::<u64>() {
                debug!(
                    "Using custom timeout of {}s for {} server from env var {}",
                    timeout_secs, server_name, env_var_name
                );
                return Duration::from_secs(timeout_secs);
            }
        }

        // Server-specific defaults
        match server_name {
            "embedding" => {
                // Embedding operations can take longer, especially for large texts
                debug!("Using extended timeout of 120s for embedding server");
                Duration::from_secs(120) // 2 minutes for embedding operations
            }
            _ => {
                // Check global timeout env var
                if let Ok(timeout_str) = std::env::var("GEMINI_MCP_TOOL_TIMEOUT") {
                    if let Ok(timeout_secs) = timeout_str.parse::<u64>() {
                        return Duration::from_secs(timeout_secs);
                    }
                }
                // Default timeout for other servers
                Duration::from_secs(30)
            }
        }
    }

    pub async fn execute_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, String> {
        // First, find the server
        let server = Self::find_ready_server(&self.servers, server_name).await?;

        if std::env::var("DEBUG").is_ok() {
            // Get current time with milliseconds for logging
            let now = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
            println!("[{now}] Executing tool {server_name}/{tool_name}");
        }

        // Then, execute the tool
        let params = rpc::ExecuteToolParams {
            tool_name: tool_name.to_string(),
            arguments: args,
        };

        // Get next request ID
        let request_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);

        let request = Request::new(
            Some(serde_json::to_value(request_id).unwrap()),
            "mcp/tool/execute".into(),
            Some(serde_json::to_value(params).unwrap()),
        );

        // Get the appropriate timeout for this server/tool
        let timeout = self.get_tool_timeout(server_name, tool_name);
        info!(
            "Using {}s timeout for {}/{}",
            timeout.as_secs(),
            server_name,
            tool_name
        );

        // Send request, with timeout for response
        let response = tokio::time::timeout(timeout, server.send_request(request))
            .await
            .map_err(|_| format!("Timeout waiting for response from server '{}'", server_name))?
            .map_err(|e| format!("Error from server '{}': {:?}", server_name, e))?;

        // Parse response
        match response.result() {
            Ok(result_value) => {
                if let Some(result) = result_value.get("result") {
                    Ok(result.clone())
                } else {
                    Err(format!(
                        "Server '{}' returned invalid result format. Expected JSON with 'result' field.",
                        server_name
                    ))
                }
            }
            Err(error) => Err(format!(
                "Error from server '{}' executing tool '{}': {}",
                server_name, tool_name, error.message
            )),
        }
    }

    // Get a resource from a specific server
    pub async fn get_resource(
        &self,
        server_name: &str,
        resource_name: &str,
        params: Option<Value>, // Add params if needed by spec/servers
    ) -> Result<Value, String> {
        // First, find the server
        let server = Self::find_ready_server(&self.servers, server_name).await?;

        // Then, get the resource
        let params = rpc::GetResourceParams {
            name: resource_name.to_string(),
            params,
        };

        // Get next request ID
        let request_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);

        let request = Request::new(
            Some(serde_json::to_value(request_id).unwrap()),
            "resource/get".into(),
            Some(serde_json::to_value(params).unwrap()),
        );

        // Get the appropriate timeout for this server/resource
        let timeout = self.get_tool_timeout(server_name, &format!("resource_{}", resource_name));
        info!(
            "Using {}s timeout for {}/{} resource",
            timeout.as_secs(),
            server_name,
            resource_name
        );

        // Send request, with timeout for response
        let response = tokio::time::timeout(timeout, server.send_request(request))
            .await
            .map_err(|_| format!("Timeout waiting for response from server '{}'", server_name))?
            .map_err(|e| format!("Error from server '{}': {:?}", server_name, e))?;

        // Parse response
        match response.result() {
            Ok(result_value) => Ok(result_value),
            Err(error) => Err(format!(
                "Error from server '{}' getting resource '{}': {}",
                server_name, resource_name, error.message
            )),
        }
    }

    // Shutdown all servers
    pub async fn shutdown(&self) {
        let mut servers_lock = self.servers.lock().await;

        if servers_lock.is_empty() {
            info!("No MCP servers to shutdown");
            return;
        }

        info!("Shutting down {} MCP servers...", servers_lock.len());

        // Take the servers out of the map (to appease the borrow checker)
        let mut servers = std::mem::take(&mut *servers_lock);

        for (server_name, server) in servers.iter_mut() {
            // Don't hold the capabilities lock during shutdown
            let should_shutdown = {
                let caps = server.capabilities.lock().await;
                caps.is_some() // Only shutdown if initialized
            };

            if should_shutdown {
                info!("Shutting down MCP server '{}'", server_name);

                // Send shutdown request if possible
                let request = Request::new(
                    Some(json!(self.next_request_id.fetch_add(1, Ordering::SeqCst))),
                    "shutdown".into(),
                    None,
                );

                // Try to send shutdown, with a longer timeout (5 seconds)
                match tokio::time::timeout(Duration::from_secs(5), server.send_request(request))
                    .await
                {
                    Ok(send_result) => match send_result {
                        Ok(response) => {
                            info!(
                                "Received shutdown response from MCP server '{}': {:?}",
                                server_name, response
                            );

                            // Wait a moment for graceful shutdown
                            tokio::time::sleep(Duration::from_secs(1)).await;

                            // Send exit notification
                            let exit_notification =
                                crate::rpc::Notification::new("exit".into(), None);

                            match server.send_notification(exit_notification).await {
                                Ok(_) => {
                                    info!("Sent exit notification to MCP server '{}'", server_name)
                                }
                                Err(e) => warn!(
                                    "Failed to send exit notification to MCP server '{}': {}",
                                    server_name, e
                                ),
                            }

                            // Additional grace period
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Err(e) => {
                            error!(
                                "Failed to send shutdown request to MCP server '{}': {:?}",
                                server_name, e
                            );
                        }
                    },
                    Err(_) => {
                        error!(
                            "Timeout sending shutdown request to MCP server '{}'",
                            server_name
                        );
                    }
                }
            }

            // Interrupt any blocked receiving/dispatching tasks by setting shutdown flag
            server.set_shutdown().await;

            // Kill the process if using Stdio
            if let Some(mut process) = server.take_process().await {
                Self::kill_process(&mut process, server_name).await;
            }
        }

        // Clean up any remaining resources
        info!("MCP host shutdown complete");
    }

    // Helper to kill child processes
    async fn kill_process(process: &mut tokio::process::Child, server_name: &str) {
        // Try to kill the process
        if let Err(e) = process.kill().await {
            error!("Failed to kill MCP server '{}': {}", server_name, e);
        } else {
            info!("Killed MCP server '{}'", server_name);
        }
    }

    // Get system information from all servers (for status)
    pub async fn get_system_info(&self) -> Result<String, String> {
        let servers = self.servers.lock().await;
        let mut output = String::new();
        output.push_str(&format!("{} MCP servers connected:\n", servers.len()));

        for (name, server) in servers.iter() {
            let caps = server.capabilities.lock().await;
            if let Some(caps) = caps.as_ref() {
                output.push_str(&format!(
                    "- {} [{}]: {} tools, {} resources\n",
                    name,
                    "READY",
                    caps.tools.len(),
                    caps.resources.len()
                ));
            } else {
                output.push_str(&format!("- {} [{}]\n", name, "INITIALIZING"));
            }
        }

        Ok(output)
    }

    // Log a message to all servers
    pub async fn log_to_servers(&self, message: &str, level: i32) {
        let servers = self.servers.lock().await;

        for (_name, server) in servers.iter() {
            // Create a notification
            let notification = create_log_notification(message, level);

            // Send the notification, ignore errors - it's just logging
            let _ = server.send_notification(notification).await;
        }
    }

    // Check if a tool is configured to auto-execute
    pub async fn is_auto_execute(&self, server_name: &str, tool_name: &str) -> bool {
        let servers = self.servers.lock().await;

        if let Some(server) = servers.get(server_name) {
            server.config.auto_execute.contains(&tool_name.to_string())
        } else {
            false
        }
    }

    // Add a tool to the auto-execute list
    pub async fn add_to_auto_execute(
        &self,
        server_name: &str,
        tool_name: &str,
    ) -> Result<(), String> {
        let mut servers = self.servers.lock().await;

        if let Some(server) = servers.get_mut(server_name) {
            // Check if it's not already there
            if !server.config.auto_execute.contains(&tool_name.to_string()) {
                // Add the tool to the auto-execute list
                server.config.auto_execute.push(tool_name.to_string());

                // Update the config file
                // Create a closure to avoid holding the lock during file I/O
                let config_clone = server.config.clone();
                drop(servers); // Release the lock

                // Now write the updated config to disk
                let config_path = crate::config::get_mcp_config_path()
                    .map_err(|e| format!("Error finding MCP config path: {}", e))?;

                let mut all_configs = if config_path.exists() {
                    let content = std::fs::read_to_string(&config_path)
                        .map_err(|e| format!("Error reading MCP config file: {}", e))?;

                    if content.trim().is_empty() {
                        Vec::new()
                    } else {
                        serde_json::from_str::<Vec<McpServerConfig>>(&content)
                            .map_err(|e| format!("Error parsing MCP config file: {}", e))?
                    }
                } else {
                    Vec::new()
                };

                // Update the server config in the list
                let mut found = false;
                for config in all_configs.iter_mut() {
                    if config.name == config_clone.name {
                        *config = config_clone.clone();
                        found = true;
                        break;
                    }
                }

                if !found {
                    all_configs.push(config_clone);
                }

                // Write the updated configs
                let content = serde_json::to_string_pretty(&all_configs)
                    .map_err(|e| format!("Error serializing MCP configs: {}", e))?;

                std::fs::write(&config_path, content)
                    .map_err(|e| format!("Error writing MCP config file: {}", e))?;

                Ok(())
            } else {
                // Already in the list, no action needed
                Ok(())
            }
        } else {
            Err(format!("Server '{}' not found", server_name))
        }
    }
}

// Implement the interface needed by MemoryStore
#[async_trait]
impl McpHostInterface for McpHost {
    // Use the existing execute_tool method, adjusting error type
    async fn execute_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
        self.execute_tool(server_name, tool_name, params)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(std::io::ErrorKind::Other, e))
                    as Box<dyn std::error::Error>
            })
    }

    // Implement get_all_capabilities using the existing method
    async fn get_all_capabilities(&self) -> memory_broker::Capabilities {
        let host_caps = self.get_all_capabilities().await;
        // Convert host_caps (Vec<Tool>) to memory_broker::Capabilities (BrokerCapabilities { tools: Vec<BrokerToolDefinition> })
        let broker_tools = host_caps
            .tools
            .into_iter()
            .map(|rpc_tool| {
                memory_broker::ToolDefinition {
                    name: rpc_tool.name, // Use the tool name directly from RpcTool
                                         // description: tool.description, // Assuming BrokerToolDefinition might need description
                                         // parameters: tool.parameters, // Assuming BrokerToolDefinition might need parameters
                }
            })
            .collect();

        memory_broker::Capabilities {
            tools: broker_tools,
        }
    }

    // Implement send_request using the internal helper
    async fn send_request(&self, request: Request) -> Result<Response, JsonRpcError> {
        let server_name = request
            .params
            .as_ref()
            .and_then(|p| p.get("server_name"))
            .and_then(|s| s.as_str())
            .ok_or_else(|| JsonRpcError {
                code: -32602,
                message: "Missing server_name in params".to_string(),
                data: None,
            })?;

        let active_server = McpHost::find_ready_server(&self.servers, server_name)
            .await
            .map_err(|e| JsonRpcError {
                code: -32603,
                message: e,
                data: None,
            })?;

        active_server.send_request(request).await
    }

    // Implement get_capabilities (assuming it gets caps for a *specific* server)
    // The trait definition seems generic, maybe it should take server_name?
    // For now, let's return combined caps, though this might not be right.
    async fn get_capabilities(&self) -> Result<ServerCapabilities, String> {
        // This implementation gets *combined* capabilities, which might not match
        // the intent if the trait method was meant for a specific server.
        // If it needs specific server caps, the trait or this impl needs adjustment.
        warn!("McpHostInterface::get_capabilities returning combined capabilities, not server-specific.");
        let combined = self.get_all_capabilities().await;
        Ok(combined)
    }
}
