use std::io::{self, BufRead, Read, Write};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::fs::{self};
use std::path::PathBuf;
use std::error::Error;
use crate::mcp::rpc::{self, Request, Response, JsonRpcError, InitializeResult, ServerInfo, ServerCapabilities, Tool};
use dirs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use log::{info, error, debug, warn};

// Memory structure
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Memory {
    key: String,
    value: String,
    timestamp: u64,
    tags: Vec<String>,
}

// MemoryStore manages memories
#[derive(Debug, Serialize, Deserialize, Default)]
struct MemoryStore {
    memories: Vec<Memory>,
}

impl MemoryStore {
    fn new() -> Self {
        Self { memories: Vec::new() }
    }
    // Placeholder for future methods (add, get, delete, etc.)
}

// Get the path for the memory store file
fn get_memory_store_path() -> Result<PathBuf, String> {
    let mut config_dir = dirs::config_dir().ok_or_else(|| "Could not find config directory".to_string())?;
    config_dir.push("gemini-cli");
    config_dir.push("memory_store.json");
    Ok(config_dir)
}

// Ensure the memory directory exists
fn ensure_memory_dir() -> Result<PathBuf, io::Error> {
    let memory_path = get_memory_store_path().map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
    if let Some(parent) = memory_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(memory_path)
}

// Load memory store from disk
fn load_memory_store() -> Result<MemoryStore, String> {
    let path = ensure_memory_dir().map_err(|e| format!("Failed to ensure memory directory: {}", e))?;
    
    if !path.exists() {
        debug!("Memory store file not found at: {}. Creating new store.", path.display());
        return Ok(MemoryStore::new());
    }
    
    match fs::read_to_string(&path) {
        Ok(json_str) => {
            if json_str.trim().is_empty() {
                 debug!("Memory store file is empty at: {}. Creating new store.", path.display());
                 return Ok(MemoryStore::new());
            }
            match serde_json::from_str::<MemoryStore>(&json_str) {
                Ok(store) => {
                    debug!("Loaded memory store from: {} with {} memories", 
                            path.display(), store.memories.len());
                    Ok(store)
                },
                Err(e) => {
                    warn!("Failed to parse memory store file: {}. Returning empty store.", e);
                    Ok(MemoryStore::new()) // Return empty store on parse error
                }
            }
        },
        Err(e) => {
            // Handle cases like permission errors gracefully
            warn!("Failed to read memory store file '{}': {}. Returning empty store.", path.display(), e);
             Ok(MemoryStore::new()) // Return empty store on read error
        }
    }
}

// Save memory store to disk
fn save_memory_store(store: &MemoryStore) -> Result<(), String> {
     let path = ensure_memory_dir().map_err(|e| format!("Failed to ensure memory directory: {}", e))?;
    let json_str = serde_json::to_string_pretty(store)
                       .map_err(|e| format!("Failed to serialize memory store: {}", e))?;
    
    fs::write(&path, json_str)
        .map_err(|e| format!("Failed to write memory store to {}: {}", path.display(), e))?;
        
    debug!("Saved memory store to: {}", path.display());
    Ok(())
}


/// Run the application as a memory store MCP server
pub async fn run() -> Result<(), Box<dyn Error>> {
    info!("Starting memory-store MCP server...");

    // Load memory store (handle error gracefully)
    let mut memory_store = match load_memory_store() {
        Ok(store) => store,
        Err(e) => {
            error!("Critical error loading memory store: {}. Exiting.", e);
            // Consider if we should panic or exit differently
             return Err(Box::new(io::Error::new(io::ErrorKind::Other, e)));
        }
    };
    
    // Deduplicate existing memories on startup
    let deduped_count = memory_store.deduplicate();
    if deduped_count > 0 {
        info!("Removed {} duplicate memories during startup", deduped_count);
        // Save the deduplicated store
        if let Err(e) = save_memory_store(&memory_store) {
            warn!("Failed to save deduplicated memory store: {}", e);
        }
    }
    
    // Set up termination signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    // Set up Ctrl+C handler
    ctrlc::set_handler(move || {
        info!("Received termination signal. Shutting down memory-store MCP server...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");
    
    // Process standard input
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();
    
    let mut buffer = Vec::new();
    let mut content_length: Option<usize> = None;
    let mut line_buffer = String::new(); // Reusable buffer for reading lines

    info!("Memory store server ready. Waiting for messages...");

    // Main processing loop
    'outer: loop {
        // Check termination signal before blocking read
        if !running.load(Ordering::SeqCst) {
            break 'outer;
        }

        line_buffer.clear();
        match stdin_lock.read_line(&mut line_buffer) {
            Ok(0) => { // EOF
                info!("Stdin closed. Exiting.");
                break 'outer;
            }
            Ok(_) => {
                let line_trimmed = line_buffer.trim();
                // debug!("Received header line: {}", line_trimmed); // Verbose

                if line_trimmed.is_empty() {
                    // End of headers, read the content if Content-Length was found
                    if let Some(length) = content_length.take() { // Use take() to reset
                       // debug!("Reading message body with length: {}", length);
                        buffer.resize(length, 0);
                        
                        // Check termination signal again before blocking read
                        if !running.load(Ordering::SeqCst) {
                            break 'outer;
                        }

                        if let Err(e) = stdin_lock.read_exact(&mut buffer) {
                            error!("Failed to read message content: {}", e);
                            break 'outer; // Exit on read error
                        }

                        // Process the message
                        let json_str = String::from_utf8_lossy(&buffer);
                        // debug!("Received JSON: {}", json_str); // Very Verbose
                        
                        match serde_json::from_str::<Request>(&json_str) {
                            Ok(request) => {
                                handle_request(request, &mut memory_store, &mut stdout).await;
                            }
                            Err(e) => {
                                error!("Failed to parse JSON-RPC request: {}", e);
                                let error_response = Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: json!(null), // No ID available for parse errors
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32700, // Parse error
                                        message: format!("Parse error: {}", e),
                                        data: None,
                                    }),
                                };
                                send_response(error_response, &mut stdout);
                            }
                        }
                    } else {
                       // warn!("Received empty line separator without Content-Length header.");
                       // Reset in case of malformed headers
                       content_length = None; 
                    }
                } else if line_trimmed.starts_with("Content-Length:") {
                    if let Some(len_str) = line_trimmed.strip_prefix("Content-Length:") {
                        match len_str.trim().parse::<usize>() {
                            Ok(len) => content_length = Some(len),
                            Err(_) => {
                                error!("Invalid Content-Length value: {}", len_str.trim());
                                content_length = None; // Reset if invalid
                            }
                        }
                    }
                } else {
                    // Ignore other headers for now
                   // debug!("Ignoring header: {}", line_trimmed);
                }
            }
            Err(e) => {
                 // Check if the error is because reading would block (for non-blocking setup)
                 if e.kind() == io::ErrorKind::WouldBlock {
                     // Yield control briefly if non-blocking, or handle appropriately
                     tokio::time::sleep(Duration::from_millis(10)).await; 
                     continue; // Try reading again
                 } else {
                    error!("Error reading from stdin: {}", e);
                    break 'outer; // Exit on other read errors
                 }
            }
        }
    }
    
    // Save final state before exiting
    if let Err(e) = save_memory_store(&memory_store) {
        error!("Failed to save memory store during shutdown: {}", e);
    }
    
    info!("Memory-store MCP server gracefully shut down");
    Ok(())
}

// Helper function to send a JSON-RPC response
fn send_response(response: Response, stdout: &mut io::Stdout) {
     match serde_json::to_string(&response) {
        Ok(response_json) => {
           // debug!("Sending response: {}", response_json); // Verbose
            let header = format!("Content-Length: {}

", response_json.len());
            if let Err(e) = stdout.write_all(header.as_bytes()) {
                error!("Failed to write response header: {}", e);
                return; // Don't try to write body if header failed
            }
            if let Err(e) = stdout.write_all(response_json.as_bytes()) {
                 error!("Failed to write response body: {}", e);
            }
            if let Err(e) = stdout.flush() {
                error!("Failed to flush stdout: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to serialize response: {}", e);
            // Potentially send a generic error response if serialization fails
        }
    }
}

// Handle incoming requests
async fn handle_request(request: Request, memory_store: &mut MemoryStore, stdout: &mut io::Stdout) {
    let request_id = request.id.clone().unwrap_or(json!(null)); // Get ID for responses

    match request.method.as_str() {
        "initialize" => {
            // Optional: Parse InitializeParams if needed, though not strictly required by MCP spec
            // let params: InitializeParams = match request.params {
            //     Some(p) => serde_json::from_value(p).unwrap_or_else(|e| /* handle error */),
            //     None => /* handle error */ ,
            // };

             debug!("Handling initialize request");

            let server_info = ServerInfo {
                name: "memory-store-mcp".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(), // Use crate version
            };
            
            // Define tools
            let tools = vec![
                Tool {
                    name: "store_memory".to_string(),
                    description: Some("Store a key-value pair with optional tags.".to_string()),
                    parameters: Some(json!({
                        "type": "object",
                        "properties": {
                            "key": {"type": "string", "description": "Unique identifier for the memory"},
                            "value": {"type": "string", "description": "Content of the memory"},
                            "tags": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Optional tags for categorization"
                            }
                        },
                        "required": ["key", "value"]
                    })),
                },
                Tool {
                    name: "retrieve_memory_by_key".to_string(),
                    description: Some("Retrieve memories matching a specific key.".to_string()),
                    parameters: Some(json!({
                        "type": "object",
                        "properties": {"key": {"type": "string", "description": "The key to search for"}},
                        "required": ["key"]
                    })),
                },
                Tool {
                    name: "retrieve_memory_by_tag".to_string(),
                    description: Some("Retrieve memories matching a specific tag.".to_string()),
                    parameters: Some(json!({
                        "type": "object",
                        "properties": {"tag": {"type": "string", "description": "The tag to search for"}},
                        "required": ["tag"]
                    })),
                },
                Tool {
                    name: "list_all_memories".to_string(),
                    description: Some("List all stored memories.".to_string()),
                    parameters: Some(json!({"type": "object", "properties": {}})),
                },
                Tool {
                    name: "delete_memory_by_key".to_string(),
                    description: Some("Delete memories matching a specific key.".to_string()),
                    parameters: Some(json!({
                        "type": "object",
                        "properties": {"key": {"type": "string", "description": "The key of the memory to delete"}},
                        "required": ["key"]
                    })),
                },
                Tool {
                    name: "deduplicate_memories".to_string(),
                    description: Some("Remove duplicate memories, keeping only the most recent version of each key.".to_string()),
                    parameters: Some(json!({"type": "object", "properties": {}})),
                }
            ];
            
            // Define resources (optional for this server)
            let resources = vec![]; // No resources defined for now
            
            let result = InitializeResult {
                server_info,
                capabilities: ServerCapabilities { tools, resources },
            };
            
            let response = Response {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: Some(serde_json::to_value(result).unwrap()), // Handle potential error
                error: None,
            };
            send_response(response, stdout);
        }

        "shutdown" => {
            debug!("Handling shutdown request");
            // Send null result as per JSON-RPC spec for shutdown
            let response = Response {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: Some(json!(null)),
                error: None,
            };
            send_response(response, stdout);
            
            // Signal shutdown (use the Arc<AtomicBool> pattern if running means checking it)
             info!("Shutdown requested via MCP. Exiting loop soon.");
             // Consider if immediate exit is needed or just signal the outer loop
        }
        
        "mcp/tool/execute" => {
            debug!("Handling tool/execute request");
            if let Some(params_value) = request.params {
                // Parse the ExecuteToolParams structure
                match serde_json::from_value::<rpc::ExecuteToolParams>(params_value.clone()) {
                    Ok(params) => {
                        debug!("Executing tool: {} with args: {:?}", params.tool_name, params.arguments);
                        
                        match params.tool_name.as_str() {
                            "store_memory" => {
                                let key = params.arguments.get("key").and_then(Value::as_str);
                                let value = params.arguments.get("value").and_then(Value::as_str);
                                let tags: Vec<String> = params.arguments.get("tags")
                                    .and_then(Value::as_array)
                                    .map(|arr| arr.iter().filter_map(Value::as_str).map(String::from).collect())
                                    .unwrap_or_default();

                                if let (Some(k), Some(v)) = (key, value) {
                                    match memory_store.add_memory(k, v, tags) {
                                        Ok(_) => {
                                            if let Err(e) = save_memory_store(memory_store) {
                                                error!("Failed to save memory store after add: {}", e);
                                                // Decide if we should still report success to client?
                                            }
                                            let response = Response {
                                                jsonrpc: "2.0".to_string(),
                                                id: request_id,
                                                result: Some(json!({ "success": true, "key": k })),
                                                error: None,
                                            };
                                            send_response(response, stdout);
                                        }
                                        Err(e) => {
                                             let error = JsonRpcError { code: -32001, message: e, data: None };
                                             let response = Response { jsonrpc: "2.0".to_string(), id: request_id, result: None, error: Some(error) };
                                             send_response(response, stdout);
                                        }
                                    }
                                } else {
                                    let error = JsonRpcError { code: -32602, message: "Invalid store_memory parameters".to_string(), data: None };
                                    let response = Response { jsonrpc: "2.0".to_string(), id: request_id, result: None, error: Some(error) };
                                    send_response(response, stdout);
                                }
                            }
                            "retrieve_memory_by_key" => {
                                if let Some(key) = params.arguments.get("key").and_then(Value::as_str) {
                                    let memories = memory_store.get_by_key(key);
                                    let response = Response {
                                        jsonrpc: "2.0".to_string(),
                                        id: request_id,
                                        result: Some(json!({ "memories": memories, "count": memories.len() })),
                                        error: None,
                                    };
                                     send_response(response, stdout);
                                } else {
                                    let error = JsonRpcError { code: -32602, message: "Invalid parameters: missing key".to_string(), data: None };
                                    let response = Response { jsonrpc: "2.0".to_string(), id: request_id, result: None, error: Some(error) };
                                     send_response(response, stdout);
                                }
                            }
                            "retrieve_memory_by_tag" => {
                                if let Some(tag) = params.arguments.get("tag").and_then(Value::as_str) {
                                     let memories = memory_store.get_by_tag(tag);
                                     let response = Response {
                                        jsonrpc: "2.0".to_string(),
                                        id: request_id,
                                        result: Some(json!({ "memories": memories, "count": memories.len(), "tag": tag })),
                                        error: None,
                                    };
                                     send_response(response, stdout);
                                } else {
                                     let error = JsonRpcError { code: -32602, message: "Invalid parameters: missing tag".to_string(), data: None };
                                     let response = Response { jsonrpc: "2.0".to_string(), id: request_id, result: None, error: Some(error) };
                                     send_response(response, stdout);
                                }
                            }
                            "list_all_memories" => {
                                 let memories = memory_store.get_all_memories();
                                 let response = Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: request_id,
                                    result: Some(json!({ "memories": memories, "count": memories.len() })),
                                    error: None,
                                };
                                 send_response(response, stdout);
                            }
                            "delete_memory_by_key" => {
                                if let Some(key) = params.arguments.get("key").and_then(Value::as_str) {
                                    let deleted_count = memory_store.delete_by_key(key);
                                    if deleted_count > 0 {
                                        if let Err(e) = save_memory_store(memory_store) {
                                             error!("Failed to save memory store after delete: {}", e);
                                            // Decide if we should still report success to client?
                                        }
                                    }
                                     let response = Response {
                                        jsonrpc: "2.0".to_string(),
                                        id: request_id,
                                        result: Some(json!({ "deleted": deleted_count > 0, "count": deleted_count, "key": key })),
                                        error: None,
                                    };
                                     send_response(response, stdout);
                                } else {
                                    let error = JsonRpcError { code: -32602, message: "Invalid parameters: missing key".to_string(), data: None };
                                     let response = Response { jsonrpc: "2.0".to_string(), id: request_id, result: None, error: Some(error) };
                                     send_response(response, stdout);
                                }
                            }
                            "deduplicate_memories" => {
                                let removed_count = memory_store.deduplicate();
                                
                                // Save the deduplicated store if changes were made
                                if removed_count > 0 {
                                    if let Err(e) = save_memory_store(memory_store) {
                                        error!("Failed to save memory store after deduplication: {}", e);
                                    }
                                }
                                
                                let response = Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: request_id,
                                    result: Some(json!({ 
                                        "success": true, 
                                        "removed_count": removed_count,
                                        "remaining_count": memory_store.memories.len()
                                    })),
                                    error: None,
                                };
                                send_response(response, stdout);
                            }
                            _ => {
                                // Unknown tool
                                warn!("Received request for unknown tool: {}", params.tool_name);
                                let error = JsonRpcError { code: -32601, message: format!("Unknown tool: {}", params.tool_name), data: None };
                                let response = Response { jsonrpc: "2.0".to_string(), id: request_id, result: None, error: Some(error) };
                                send_response(response, stdout);
                            }
                        }
                    }
                    Err(e) => {
                        // Failed to parse the parameters structure
                        error!("Failed to parse tool/execute parameters: {}. Params: {}", e, params_value);
                        let error = JsonRpcError {
                            code: -32602, // Invalid params
                            message: format!("Invalid parameters for tool/execute: {}", e),
                            data: Some(params_value), // Echo back the invalid params
                        };
                         let response = Response {
                            jsonrpc: "2.0".to_string(),
                            id: request_id,
                            result: None,
                            error: Some(error),
                        };
                        send_response(response, stdout);
                    }
                }
            } else {
                 // Params field was missing
                 error!("Missing params for tool/execute request");
                  let error = JsonRpcError {
                    code: -32602, // Invalid params
                    message: "Missing parameters for mcp/tool/execute".to_string(),
                    data: None,
                };
                 let response = Response {
                    jsonrpc: "2.0".to_string(),
                    id: request_id,
                    result: None,
                    error: Some(error),
                };
                 send_response(response, stdout);
            }
        }
        
        // Optional: Handle mcp/resource/get if resources were defined
        // "mcp/resource/get" => { ... }

        _ => {
            // Unknown method
            warn!("Received unknown method: {}", request.method);
            let error = JsonRpcError {
                code: -32601, // Method not found
                message: format!("Method not found: {}", request.method),
                data: None,
            };
            let response = Response {
                jsonrpc: "2.0".to_string(),
                id: request_id,
                result: None,
                error: Some(error),
            };
            send_response(response, stdout);
        }
    }
}

// --- MemoryStore Method Implementations ---

impl MemoryStore {
    // Add a new memory with the given key and value
    fn add_memory(&mut self, key: &str, value: &str, tags: Vec<String>) -> Result<(), String> {
        if key.trim().is_empty() {
            return Err("Memory key cannot be empty".to_string());
        }
         if value.trim().is_empty() {
            return Err("Memory value cannot be empty".to_string());
        }
        
        // Check for exact duplicates (same key and value)
        if self.memories.iter().any(|m| m.key == key && m.value == value) {
            debug!("Skipping duplicate memory with key '{}' and identical value", key);
            return Ok(());
        }
        
        // Check for similar entries with same key and normalize tags
        let existing_entries: Vec<_> = self.memories.iter()
            .filter(|m| m.key == key)
            .collect();
        
        if !existing_entries.is_empty() {
            // If we have existing entries with same key but different values,
            // update the most recent one instead of adding a new entry
            if existing_entries.iter().any(|m| m.value != value) {
                debug!("Updating existing memory with key '{}'", key);
                
                // Find the most recent entry with this key
                let mut idx_to_update = None;
                let mut latest_timestamp = 0;
                
                for (idx, memory) in self.memories.iter().enumerate() {
                    if memory.key == key && memory.timestamp > latest_timestamp {
                        latest_timestamp = memory.timestamp;
                        idx_to_update = Some(idx);
                    }
                }
                
                if let Some(idx) = idx_to_update {
                    // Update the existing entry instead of adding a new one
                    let timestamp = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    
                    // Merge tags to avoid duplicates
                    let mut updated_tags = self.memories[idx].tags.clone();
                    for tag in &tags {
                        if !updated_tags.contains(tag) {
                            updated_tags.push(tag.clone());
                        }
                    }
                    
                    self.memories[idx].value = value.to_string();
                    self.memories[idx].timestamp = timestamp;
                    self.memories[idx].tags = updated_tags;
                    
                    return Ok(());
                }
            }
        }
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        // Add new memory if we haven't found a duplicate or updatable entry
        self.memories.push(Memory {
            key: key.to_string(),
            value: value.to_string(),
            timestamp,
            tags, // Use provided tags
        });
        
        Ok(())
    }

    // Get memories by key (exact match)
    fn get_by_key(&self, key: &str) -> Vec<Memory> {
        self.memories
            .iter()
            .filter(|m| m.key == key)
            .cloned()
            .collect()
    }

    // Get memories by tag
    fn get_by_tag(&self, tag: &str) -> Vec<Memory> {
        if tag.trim().is_empty() { return Vec::new(); } // Don't search for empty tags
        self.memories
            .iter()
            .filter(|m| m.tags.iter().any(|t| t == tag))
            .cloned()
            .collect()
    }

    // Get all memories
    fn get_all_memories(&self) -> Vec<Memory> {
        self.memories.clone()
    }

    // Delete memory by key
    fn delete_by_key(&mut self, key: &str) -> usize {
        let initial_len = self.memories.len();
        self.memories.retain(|m| m.key != key);
        let deleted_count = initial_len - self.memories.len();
        if deleted_count > 0 {
             debug!("Deleted {} memories with key {}", deleted_count, key);
        }
        deleted_count
    }
    
    // Deduplicate the memory store
    fn deduplicate(&mut self) -> usize {
        let initial_len = self.memories.len();
        
        // Sort memories by timestamp (newest first) to keep the most recent entries
        self.memories.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        
        // Track keys we've already processed
        let mut seen_keys = std::collections::HashSet::new();
        let mut to_keep = Vec::new();
        
        // First pass: keep one entry per key (the most recent one)
        for memory in self.memories.drain(..) {
            if !seen_keys.contains(&memory.key) {
                seen_keys.insert(memory.key.clone());
                to_keep.push(memory);
            }
        }
        
        // Replace the memories with the deduplicated list
        self.memories = to_keep;
        
        let removed_count = initial_len - self.memories.len();
        if removed_count > 0 {
            debug!("Removed {} duplicate memories during deduplication", removed_count);
        }
        
        removed_count
    }
} 