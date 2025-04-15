mod active_server;
mod message_handler;
mod io;
mod types;

// Re-export main types
pub use self::types::ActiveServer;

// Main host implementation
use crate::mcp::config::{McpServerConfig, McpTransport};
use crate::mcp::rpc::{self, ServerCapabilities, Request, create_log_notification};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task;
use serde_json::{self, json, Value};
use log::{debug, error, info, warn};

// Need Clone for task spawning
#[derive(Debug, Clone)]
pub struct McpHost {
    servers: Arc<Mutex<HashMap<String, ActiveServer>>>, // Keyed by server name
    next_request_id: Arc<AtomicU64>, // Use atomic for thread-safe incrementing
}

impl McpHost {
    pub async fn new(configs: Vec<McpServerConfig>) -> Result<Self, String> {
        let host = McpHost {
            servers: Arc::new(Mutex::new(HashMap::new())),
            next_request_id: Arc::new(AtomicU64::new(1)), // Start IDs from 1
        };

        let mut init_tasks = Vec::new();
        let mut servers_map = HashMap::new(); // Temp map to build servers before locking

        for config in configs {
            if config.transport == McpTransport::Stdio {
                match ActiveServer::launch_stdio(
                    &host.next_request_id, 
                    config.clone()
                ).await {
                    Ok((active_server, init_future)) => {
                        let server_name = active_server.config.name.clone();
                        servers_map.insert(server_name.clone(), active_server);
                        // Spawn a task to wait for the initialization result
                        init_tasks.push(task::spawn(async move {
                            match init_future.await {
                                Ok(Ok(_)) => {
                                    info!("MCP Server '{}' initialized successfully.", server_name);
                                    Ok(server_name)
                                }
                                Ok(Err(rpc_error)) => {
                                    error!(
                                        "MCP Server '{}' initialization failed: {:?}",
                                        server_name, rpc_error
                                    );
                                    Err(format!(
                                        "Server '{}' init failed",
                                        server_name
                                    ))
                                }
                                Err(timeout_error) => {
                                    error!(
                                        "MCP Server '{}' initialization timed out: {}",
                                        server_name, timeout_error
                                    );
                                    Err(format!(
                                        "Server '{}' init timed out",
                                        server_name
                                    ))
                                }
                            }
                        }));
                    }
                    Err(e) => {
                        error!("Failed to launch MCP server '{}': {}", config.name, e);
                        // Optionally collect launch errors to return later
                    }
                }
            }
            // TODO: Handle other transports
        }

        // Move successfully launched servers into the main map
        host.servers.lock().await.extend(servers_map);

        // Wait for initialization results (with a timeout)
        let results = futures::future::join_all(init_tasks).await;

        let mut failed_servers = Vec::new();
        for result in results {
            match result {
                Ok(Ok(_server_name)) => { /* Server initialized ok */ }
                Ok(Err(e)) => {
                    error!("Initialization error: {}", e);
                    failed_servers.push(e); // Collect errors
                }
                Err(join_error) => {
                    error!("Join error waiting for server init: {}", join_error);
                    failed_servers.push(format!("Join error: {}", join_error));
                }
            }
        }

        // Optionally remove failed servers from the host's map or mark them as errored
        if !failed_servers.is_empty() {
            warn!(
                "Some MCP servers failed to initialize: {:?}",
                failed_servers
            );
            // Decide if this should be a hard error for McpHost::new()
            // return Err(format!("Failed to initialize servers: {:?}", failed_servers));
        }

        Ok(host)
    }

    // Gets combined capabilities from all *initialized* servers
    pub async fn get_all_capabilities(&self) -> ServerCapabilities {
        let servers = self.servers.lock().await;
        let mut combined_caps = ServerCapabilities::default();
        
        for (server_name, server) in servers.iter() {
            if let Some(caps) = server.capabilities.lock().await.as_ref() {
                // Prefix tool and resource names with server name to avoid collisions
                for mut tool in caps.tools.iter().cloned() {
                    // Inject server name into tool schema references if present
                    if let Some(_schema) = &mut tool.parameters {
                        // TODO: Recursively find $ref fields in the schema (JSON Value)
                        //       and prepend "server_name/" to the reference path.
                        //       Requires careful JSON manipulation.
                    }
                    
                    // Modify tool name to include server prefix
                    let original_name = tool.name.clone();
                    tool.name = format!("{}/{}", server_name, original_name);
                    
                    // Add server context to description if present
                    match tool.description.as_mut() {
                        Some(desc) => *desc = format!("[From {}] {}", server_name, desc),
                        None => {}
                    }
                    
                    combined_caps.tools.push(tool);
                }
                
                for mut resource in caps.resources.iter().cloned() {
                    // Prefix resource name
                    let original_name = resource.name.clone();
                    resource.name = format!("{}/{}", server_name, original_name);
                    
                    // Add server context to description if present
                    match resource.description.as_mut() {
                        Some(desc) => *desc = format!("[From {}] {}", server_name, desc),
                        None => {}
                    }
                    
                    combined_caps.resources.push(resource);
                }
            }
        }
        
        combined_caps
    }

    // Find the server and check if it's ready and supports the capability
    async fn find_ready_server(
        servers: &Mutex<HashMap<String, ActiveServer>>,
        server_name: &str,
    ) -> Result<ActiveServer, String> {
        let servers_guard = servers.lock().await;
        let server = servers_guard
            .get(server_name)
            .ok_or_else(|| format!("Server '{}' not found.", server_name))?;
        
        // Check if capabilities are loaded (i.e., initialized)
        let has_capabilities = {
            let capabilities = server.capabilities.lock().await;
            capabilities.is_some()
        };
        
        if has_capabilities {
            // TODO: Add more explicit state check (e.g., `server.status == Status::Ready`)
            Ok(server.clone()) // Clone the necessary parts (Arc pointers)
        } else {
            Err(format!("Server '{}' is not initialized or ready.", server_name))
        }
    }

    // Executes a tool on a specific server
    pub async fn execute_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: Value,
    ) -> Result<Value, String> {
        if std::env::var("GEMINI_DEBUG").is_ok() {
            println!("[DEBUG] McpHost execute_tool: server='{}', tool='{}'", server_name, tool_name);
        }
        let server = Self::find_ready_server(&self.servers, server_name).await?;

        // --- 1. Check Capability ---
        let has_tool = { // Scoped lock
            let caps_guard = server.capabilities.lock().await;
            let empty_vec = Vec::new();
            let available_tools = match caps_guard.as_ref() {
                Some(caps) => {
                    if std::env::var("GEMINI_DEBUG").is_ok() {
                        println!("[DEBUG] Available tools:");
                        for tool in &caps.tools {
                            println!("[DEBUG]   - '{}'", tool.name);
                        }
                    }
                    &caps.tools
                },
                None => &empty_vec
            };
            
            let found = available_tools.iter().any(|t| t.name == tool_name);
            if std::env::var("GEMINI_DEBUG").is_ok() {
                println!("[DEBUG] Tool '{}' found in capabilities: {}", tool_name, found);
            }
            found
        };

        if !has_tool {
            return Err(format!(
                "Server '{}' does not support tool '{}' or is not initialized.",
                server_name, tool_name
            ));
        }

        // --- 2. Prepare Request ---
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let params = rpc::ExecuteToolParams {
            tool_name: tool_name.to_string(),
            arguments: args,
        };

        // --- 3. Call ActiveServer Method --- 
        server.execute_tool(request_id, params).await.map_err(|rpc_error| {
            // Convert JsonRpcError back to String for the McpHost method signature
            format!("Tool execution error [{}]: {}", rpc_error.code, rpc_error.message)
        })
    }

    // Gets a resource from a specific server
    pub async fn get_resource(
        &self,
        server_name: &str,
        resource_name: &str,
        params: Option<Value>, // Add params if needed by spec/servers
    ) -> Result<Value, String> {
        info!("Requesting resource get: server='{}', resource='{}', params={:?}", server_name, resource_name, params);
        let server = Self::find_ready_server(&self.servers, server_name).await?;

        // --- 1. Check Capability ---
        let has_resource = { // Scoped lock
            let caps_guard = server.capabilities.lock().await;
            caps_guard.as_ref().map_or(false, |caps| {
                caps.resources.iter().any(|r| r.name == resource_name)
            })
        };

        if !has_resource {
            return Err(format!(
                "Server '{}' does not support resource '{}' or is not initialized.",
                server_name, resource_name
            ));
        }

        // --- 2. Prepare Request ---
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let params = rpc::GetResourceParams {
            name: resource_name.to_string(),
            params, // Pass provided params
        };

        // --- 3. Call ActiveServer Method --- 
        server.get_resource(request_id, params).await.map_err(|rpc_error| {
            // Convert JsonRpcError back to String for the McpHost method signature
            format!("Resource retrieval error [{}]: {}", rpc_error.code, rpc_error.message)
        })
    }

    // Graceful shutdown of all managed servers
    pub async fn shutdown(&self) {
        info!("Starting MCP Host shutdown sequence");
        let mut servers = self.servers.lock().await;
        
        for (server_name, server) in servers.iter_mut() {
            info!("Shutting down MCP server: {}", server_name);
            
            // 1. Send shutdown signal to tasks
            if let Some(shutdown_tx) = server.shutdown_signal.lock().await.take() {
                let _ = shutdown_tx.send(());
            }
            
            // 2. Try to gracefully shut down each server with the 'shutdown' method
            // (per LSP spec, followed by 'exit' notification)
            let req_id = self.next_request_id.fetch_add(1, Ordering::SeqCst);
            let shutdown_req = Request {
                jsonrpc: "2.0".to_string(),
                id: Some(json!(req_id)),
                method: "shutdown".to_string(),
                params: None,
            };
            
            // Serialize and send shutdown request
            if let Ok(req_str) = serde_json::to_string(&shutdown_req) {
                match server.stdin_tx.try_send(req_str) {
                    Ok(_) => {
                        // Don't wait for result - server might already be gone
                        debug!("Sent shutdown request to {}", server_name);
                    }
                    Err(e) => {
                        info!("Failed to send shutdown request to {}: {}", server_name, e);
                    }
                }
            }
            
            // Send 'exit' notification (no response expected)
            let exit_notification = Request {
                jsonrpc: "2.0".to_string(),
                id: None, // None = notification
                method: "exit".to_string(),
                params: None,
            };
            
            if let Ok(notif_str) = serde_json::to_string(&exit_notification) {
                let _ = server.stdin_tx.try_send(notif_str); // Ignore result
            }
            
            // 3. Wait a moment for the server to handle shutdown/exit
            tokio::time::sleep(Duration::from_millis(100)).await;
            
            // 4. Force kill process if still running
            let mut process = server.process.lock().await;
            Self::kill_process(&mut process, server_name).await;
            
            // 5. Wait for reader/writer tasks to finish
            // We could use either timeout or optionally join these
            let reader_task = server.reader_task.lock().await.take();
            let writer_task = server.writer_task.lock().await.take();
            let stderr_task = server.stderr_task.lock().await.take();
            
            if let Some(task) = reader_task {
                let _ = task.await; // Or timeout
            }
            if let Some(task) = writer_task {
                let _ = task.await; // Or timeout
            }
            if let Some(task) = stderr_task {
                let _ = task.await; // Or timeout
            }
        }

        servers.clear(); // Clear the servers map
        info!("MCP Host shutdown sequence complete");
    }

    async fn kill_process(process: &mut tokio::process::Child, server_name: &str) {
        match process.kill().await {
            Ok(_) => {
                debug!("Killed MCP server process: {}", server_name);
            }
            Err(e) => {
                error!("Failed to kill MCP server process {}: {}", server_name, e);
            }
        }
    }

    // Add an example that demonstrates get_resource usage
    pub async fn get_system_info(&self) -> Result<String, String> {
        // Get system info resource if available
        if let Ok(server_configs) = crate::mcp::config::load_mcp_servers() {
            // Find the filesystem server for resource access
            if let Some(fs_server) = server_configs.iter().find(|s| s.name == "filesystem") {
                match self.get_resource(&fs_server.name, "current_directory", None).await {
                    Ok(value) => {
                        return Ok(format!("Current directory: {}", 
                            value["path"].as_str().unwrap_or("unknown")));
                    },
                    Err(e) => {
                        return Err(format!("Failed to get current directory: {}", e));
                    }
                }
            }
        }
        
        Err("No filesystem server available for resource access".to_string())
    }

    // Send log message to all connected servers
    pub async fn log_to_servers(&self, message: &str, level: i32) {
        // Create a log notification
        let notification = create_log_notification(message, level);
        
        // Send to all servers
        let servers_guard = self.servers.lock().await;
        for (_, server) in servers_guard.iter() {
            if let Err(e) = server.send_notification(&notification).await {
                error!("Failed to send log notification to server {}: {}", 
                       server.config.name, e);
            }
        }
    }

    // Checks if a tool is in the auto-execute list for a server
    pub async fn is_auto_execute(&self, server_name: &str, tool_name: &str) -> bool {
        let servers = self.servers.lock().await;
        if let Some(server) = servers.get(server_name) {
            return server.config.auto_execute.iter().any(|t| t == tool_name);
        }
        false
    }
    
    // Adds a tool to the auto-execute list for a server
    pub async fn add_to_auto_execute(&self, server_name: &str, tool_name: &str) -> Result<(), String> {
        // Find the config file path
        let config_path = crate::mcp::config::get_mcp_config_path()
            .map_err(|e| format!("Error finding MCP config path: {}", e))?;
        
        // Read the current config
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("Error reading MCP config file: {}", e))?;
        
        let mut servers: Vec<crate::mcp::config::McpServerConfig> = serde_json::from_str(&content)
            .map_err(|e| format!("Error parsing MCP config: {}", e))?;
        
        // Find the server and update its auto_execute list
        let mut found = false;
        for server in &mut servers {
            if server.name == server_name {
                found = true;
                if !server.auto_execute.contains(&tool_name.to_string()) {
                    server.auto_execute.push(tool_name.to_string());
                    println!("Added '{}' to auto-execute list for server '{}'", tool_name, server_name);
                }
                break;
            }
        }
        
        if !found {
            return Err(format!("Server '{}' not found in config", server_name));
        }
        
        // Write the updated config back
        let updated_content = serde_json::to_string_pretty(&servers)
            .map_err(|e| format!("Error serializing updated config: {}", e))?;
        
        std::fs::write(&config_path, updated_content)
            .map_err(|e| format!("Error writing updated config: {}", e))?;
        
        // Also update the in-memory server config
        let mut servers_guard = self.servers.lock().await;
        if let Some(server) = servers_guard.get_mut(server_name) {
            if !server.config.auto_execute.contains(&tool_name.to_string()) {
                server.config.auto_execute.push(tool_name.to_string());
            }
        }
        
        Ok(())
    }
} // impl McpHost 