use std::io::{self, BufRead, Read, Write};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs;
use std::path::PathBuf;
use std::error::Error;
use std::collections::HashSet;
use crate::mcp::rpc::{Request, Response, JsonRpcError, InitializeResult, ServerInfo, ServerCapabilities, Tool, Resource};
use dirs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Memory structure for storing key-value memories
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Memory {
    key: String,
    value: String,
    timestamp: u64,
    tags: Vec<String>,
}

// MemoryStore manages a collection of memories
#[derive(Debug, Serialize, Deserialize, Default)]
struct MemoryStore {
    memories: Vec<Memory>,
}

impl MemoryStore {
    // Create a new empty memory store
    fn new() -> Self {
        Self { memories: Vec::new() }
    }

    // Add a new memory with the given key and value
    fn add_memory(&mut self, key: &str, value: &str, tags: Vec<String>) -> Result<(), String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        self.memories.push(Memory {
            key: key.to_string(),
            value: value.to_string(),
            timestamp,
            tags,
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
        self.memories
            .iter()
            .filter(|m| m.tags.contains(&tag.to_string()))
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
        initial_len - self.memories.len()
    }
}

// Get the path for the memory store file
fn get_memory_store_path() -> PathBuf {
    let mut config_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    config_dir.push("gemini-cli");
    config_dir.push("memory_store.json");
    config_dir
}

// Ensure the memory directory exists
fn ensure_memory_dir() -> Result<PathBuf, io::Error> {
    let memory_path = get_memory_store_path();
    if let Some(parent) = memory_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(memory_path)
}

// Load memory store from disk
fn load_memory_store() -> Result<MemoryStore, String> {
    let path = match ensure_memory_dir() {
        Ok(p) => p,
        Err(e) => return Err(format!("Failed to create memory directory: {}", e)),
    };
    
    if !path.exists() {
        eprintln!("Memory store file not found at: {}. Creating new store.", path.display());
        return Ok(MemoryStore::new());
    }
    
    match fs::read_to_string(&path) {
        Ok(json_str) => {
            match serde_json::from_str::<MemoryStore>(&json_str) {
                Ok(store) => {
                    eprintln!("Loaded memory store from: {} with {} memories", 
                            path.display(), store.memories.len());
                    Ok(store)
                },
                Err(e) => {
                    eprintln!("Failed to parse memory store file: {}", e);
                    Ok(MemoryStore::new())
                }
            }
        },
        Err(e) => {
            eprintln!("Failed to read memory store file: {}", e);
            Ok(MemoryStore::new())
        }
    }
}

// Save memory store to disk
fn save_memory_store(store: &MemoryStore) -> Result<(), String> {
    let path = match ensure_memory_dir() {
        Ok(p) => p,
        Err(e) => return Err(format!("Failed to create memory directory: {}", e)),
    };
    let json_str = match serde_json::to_string_pretty(store) {
        Ok(s) => s,
        Err(e) => return Err(format!("Failed to serialize memory store: {}", e)),
    };
    
    match fs::write(&path, json_str) {
        Ok(_) => {
            eprintln!("Saved memory store to: {}", path.display());
            Ok(())
        },
        Err(e) => Err(format!("Failed to write memory store: {}", e)),
    }
}

/// Run the application as a memory MCP server
pub async fn run() -> Result<(), Box<dyn Error>> {
    println!("Starting memory MCP server...");

    // Load memory store
    let mut memory_store = match load_memory_store() {
        Ok(store) => store,
        Err(e) => {
            eprintln!("Failed to load memory store: {}", e);
            MemoryStore::new()
        }
    };
    
    // Set up termination signal handling
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    
    // Set up Ctrl+C handler
    ctrlc::set_handler(move || {
        println!("Received termination signal. Shutting down memory MCP server...");
        r.store(false, Ordering::SeqCst);
    }).expect("Error setting Ctrl-C handler");
    
    // Process standard input
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut stdout = io::stdout();
    
    let mut buffer = Vec::new();
    let mut content_length: Option<usize> = None;
    
    // Use tokio to poll stdin with a timeout so we can check for termination signal
    let mut line = String::new();
    
    // Main processing loop
    'outer: while running.load(Ordering::SeqCst) {
        // Clear the line for new input
        line.clear();
        
        // Use a non-blocking approach to reading stdin
        match stdin_lock.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let line_trimmed = line.trim();
                if line_trimmed.starts_with("Content-Length:") {
                    if let Some(len_str) = line_trimmed.strip_prefix("Content-Length:") {
                        if let Ok(len) = len_str.trim().parse::<usize>() {
                            content_length = Some(len);
                        }
                    }
                } else if line_trimmed.is_empty() {
                    // End of headers, read the content
                    if let Some(length) = content_length {
                        buffer.resize(length, 0);
                        
                        // Check if we should terminate before potentially blocking on read
                        if !running.load(Ordering::SeqCst) {
                            break 'outer;
                        }
                        
                        match stdin_lock.read_exact(&mut buffer) {
                            Ok(_) => {
                                // Process the message
                                let json_str = String::from_utf8_lossy(&buffer);
                                
                                // Try to parse as a Request
                                match serde_json::from_str::<Request>(&json_str) {
                                    Ok(request) => {
                                        match request.method.as_str() {
                                            "initialize" => {
                                                // Define server capabilities
                                                let server_info = ServerInfo {
                                                    name: "memory-mcp".to_string(),
                                                    version: "1.0.0".to_string(),
                                                };
                                                
                                                // Define tools
                                                let tools = vec![
                                                    Tool {
                                                        name: "store_memory".to_string(),
                                                        description: Some("Store a new memory in the persistent memory store".to_string()),
                                                        parameters: Some(json!({
                                                            "type": "object",
                                                            "properties": {
                                                                "key": {
                                                                    "type": "string",
                                                                    "description": "Unique identifier for the memory"
                                                                },
                                                                "value": {
                                                                    "type": "string",
                                                                    "description": "Content of the memory to store"
                                                                },
                                                                "tags": {
                                                                    "type": "array",
                                                                    "items": {
                                                                        "type": "string"
                                                                    },
                                                                    "description": "Optional tags to categorize the memory"
                                                                }
                                                            },
                                                            "required": ["key", "value"]
                                                        })),
                                                    },
                                                    Tool {
                                                        name: "retrieve_memory".to_string(),
                                                        description: Some("Retrieve memories by key".to_string()),
                                                        parameters: Some(json!({
                                                            "type": "object",
                                                            "properties": {
                                                                "key": {
                                                                    "type": "string",
                                                                    "description": "Key to search for"
                                                                }
                                                            },
                                                            "required": ["key"]
                                                        })),
                                                    },
                                                    Tool {
                                                        name: "retrieve_by_tag".to_string(),
                                                        description: Some("Retrieve memories by tag".to_string()),
                                                        parameters: Some(json!({
                                                            "type": "object",
                                                            "properties": {
                                                                "tag": {
                                                                    "type": "string",
                                                                    "description": "Tag to search for"
                                                                }
                                                            },
                                                            "required": ["tag"]
                                                        })),
                                                    },
                                                    Tool {
                                                        name: "list_all_memories".to_string(),
                                                        description: Some("List all stored memories".to_string()),
                                                        parameters: Some(json!({
                                                            "type": "object",
                                                            "properties": {}
                                                        })),
                                                    },
                                                    Tool {
                                                        name: "delete_memory".to_string(),
                                                        description: Some("Delete a memory by key".to_string()),
                                                        parameters: Some(json!({
                                                            "type": "object",
                                                            "properties": {
                                                                "key": {
                                                                    "type": "string",
                                                                    "description": "Key of the memory to delete"
                                                                }
                                                            },
                                                            "required": ["key"]
                                                        })),
                                                    },
                                                ];
                                                
                                                let resources = vec![
                                                    Resource {
                                                        name: "memory_stats".to_string(),
                                                        description: Some("Get statistics about the memory store".to_string()),
                                                    }
                                                ];
                                                
                                                // Prepare the response
                                                let result = InitializeResult {
                                                    server_info,
                                                    capabilities: ServerCapabilities {
                                                        tools,
                                                        resources,
                                                    },
                                                };
                                                
                                                let response = Response {
                                                    jsonrpc: "2.0".to_string(),
                                                    id: request.id.unwrap_or(json!(null)),
                                                    result: Some(serde_json::to_value(result).unwrap()),
                                                    error: None,
                                                };
                                                
                                                let response_json = serde_json::to_string(&response).unwrap();
                                                
                                                let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                stdout.write_all(header.as_bytes()).unwrap();
                                                stdout.write_all(response_json.as_bytes()).unwrap();
                                                stdout.flush().unwrap();
                                            },
                                            
                                            "shutdown" => {
                                                // Handle shutdown request
                                                println!("Received shutdown request");
                                                
                                                // Send success response
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
                                                
                                                // Signal shutdown
                                                running.store(false, Ordering::SeqCst);
                                                break 'outer;
                                            },
                                            
                                            "mcp/tool/execute" => {
                                                // Handle tool execution request
                                                if let Some(params) = request.params {
                                                    // Expected params format: {name: string, args: object}
                                                    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                                    // Store arguments in a variable to avoid temporary value being dropped
                                                    let arguments = params.get("args").cloned().unwrap_or(json!({}));
                                                    
                                                    eprintln!("Executing memory tool: '{}' with args: {:?}", tool_name, arguments);
                                                    
                                                    match tool_name {
                                                        "store_memory" => {
                                                            let key = arguments.get("key").and_then(|v| v.as_str()).unwrap_or("");
                                                            let value = arguments.get("value").and_then(|v| v.as_str()).unwrap_or("");
                                                            
                                                            // Extract tags if present
                                                            let tags: Vec<String> = match arguments.get("tags") {
                                                                Some(tags_val) if tags_val.is_array() => {
                                                                    tags_val.as_array()
                                                                        .unwrap()
                                                                        .iter()
                                                                        .filter_map(|t| t.as_str().map(String::from))
                                                                        .collect()
                                                                },
                                                                _ => Vec::new(),
                                                            };
                                                            
                                                            // Store memory
                                                            match memory_store.add_memory(key, value, tags) {
                                                                Ok(_) => {
                                                                    // Save to disk
                                                                    if let Err(e) = save_memory_store(&memory_store) {
                                                                        eprintln!("Failed to save memory store: {}", e);
                                                                    }
                                                                    
                                                                    // Send success response
                                                                    let response = Response {
                                                                        jsonrpc: "2.0".to_string(),
                                                                        id: request.id.unwrap_or(json!(null)),
                                                                        result: Some(json!({
                                                                            "success": true,
                                                                            "key": key,
                                                                        })),
                                                                        error: None,
                                                                    };
                                                                    
                                                                    let response_json = serde_json::to_string(&response).unwrap();
                                                                    
                                                                    let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                                    stdout.write_all(header.as_bytes()).unwrap();
                                                                    stdout.write_all(response_json.as_bytes()).unwrap();
                                                                    stdout.flush().unwrap();
                                                                },
                                                                Err(e) => {
                                                                    // Send error response
                                                                    let error = JsonRpcError {
                                                                        code: -32000,
                                                                        message: format!("Failed to store memory: {}", e),
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
                                                        
                                                        "retrieve_memory" => {
                                                            let key = arguments.get("key").and_then(|v| v.as_str()).unwrap_or("");
                                                            
                                                            let memories = memory_store.get_by_key(key);
                                                            
                                                            // Convert memories to JSON
                                                            let memories_json: Vec<Value> = memories.iter().map(|memory| {
                                                                json!({
                                                                    "key": memory.key,
                                                                    "value": memory.value,
                                                                    "timestamp": memory.timestamp,
                                                                    "tags": memory.tags
                                                                })
                                                            }).collect();
                                                            
                                                            // Send response
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "memories": memories_json,
                                                                    "count": memories.len()
                                                                })),
                                                                error: None,
                                                            };
                                                            
                                                            let response_json = serde_json::to_string(&response).unwrap();
                                                            
                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        
                                                        "retrieve_by_tag" => {
                                                            let tag = arguments.get("tag").and_then(|v| v.as_str()).unwrap_or("");
                                                            
                                                            let memories = memory_store.get_by_tag(tag);
                                                            
                                                            // Convert memories to JSON
                                                            let memories_json: Vec<Value> = memories.iter().map(|memory| {
                                                                json!({
                                                                    "key": memory.key,
                                                                    "value": memory.value,
                                                                    "timestamp": memory.timestamp,
                                                                    "tags": memory.tags
                                                                })
                                                            }).collect();
                                                            
                                                            // Send response
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "memories": memories_json,
                                                                    "count": memories.len(),
                                                                    "tag": tag
                                                                })),
                                                                error: None,
                                                            };
                                                            
                                                            let response_json = serde_json::to_string(&response).unwrap();
                                                            
                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        
                                                        "list_all_memories" => {
                                                            let memories = memory_store.get_all_memories();
                                                            
                                                            // Convert memories to JSON
                                                            let memories_json: Vec<Value> = memories.iter().map(|memory| {
                                                                json!({
                                                                    "key": memory.key,
                                                                    "value": memory.value,
                                                                    "timestamp": memory.timestamp,
                                                                    "tags": memory.tags
                                                                })
                                                            }).collect();
                                                            
                                                            // Send response
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "memories": memories_json,
                                                                    "count": memories.len()
                                                                })),
                                                                error: None,
                                                            };
                                                            
                                                            let response_json = serde_json::to_string(&response).unwrap();
                                                            
                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        
                                                        "delete_memory" => {
                                                            let key = arguments.get("key").and_then(|v| v.as_str()).unwrap_or("");
                                                            
                                                            let deleted_count = memory_store.delete_by_key(key);
                                                            
                                                            // Save to disk
                                                            if deleted_count > 0 {
                                                                if let Err(e) = save_memory_store(&memory_store) {
                                                                    eprintln!("Failed to save memory store after deletion: {}", e);
                                                                }
                                                            }
                                                            
                                                            // Send response
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "deleted": deleted_count > 0,
                                                                    "count": deleted_count,
                                                                    "key": key
                                                                })),
                                                                error: None,
                                                            };
                                                            
                                                            let response_json = serde_json::to_string(&response).unwrap();
                                                            
                                                            let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                            stdout.write_all(header.as_bytes()).unwrap();
                                                            stdout.write_all(response_json.as_bytes()).unwrap();
                                                            stdout.flush().unwrap();
                                                        },
                                                        
                                                        _ => {
                                                            // Unknown tool
                                                            let error = JsonRpcError {
                                                                code: -32601,
                                                                message: format!("Unknown tool: {}", tool_name),
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
                                                }
                                            },
                                            
                                            "mcp/resource/get" => {
                                                // Handle resource get request
                                                if let Some(params) = request.params {
                                                    let resource_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                                    
                                                    match resource_name {
                                                        "memory_stats" => {
                                                            let memories = memory_store.get_all_memories();
                                                            
                                                            // Collect unique tags
                                                            let mut tags = std::collections::HashSet::new();
                                                            for memory in &memories {
                                                                for tag in &memory.tags {
                                                                    tags.insert(tag.clone());
                                                                }
                                                            }
                                                            
                                                            // Build response
                                                            let response = Response {
                                                                jsonrpc: "2.0".to_string(),
                                                                id: request.id.unwrap_or(json!(null)),
                                                                result: Some(json!({
                                                                    "total_memories": memories.len(),
                                                                    "unique_tags": tags.len(),
                                                                    "tags": tags,
                                                                })),
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
                                                }
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
                                                
                                                let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                                stdout.write_all(header.as_bytes()).unwrap();
                                                stdout.write_all(response_json.as_bytes()).unwrap();
                                                stdout.flush().unwrap();
                                            }
                                        }
                                    },
                                    Err(e) => {
                                        // Parse error
                                        eprintln!("Failed to parse JSON-RPC request: {}", e);
                                        
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
                                        
                                        let header = format!("Content-Length: {}\r\n\r\n", response_json.len());
                                        stdout.write_all(header.as_bytes()).unwrap();
                                        stdout.write_all(response_json.as_bytes()).unwrap();
                                        stdout.flush().unwrap();
                                    }
                                }
                                
                                // Reset buffer and content length for next message
                                content_length = None;
                            },
                            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                                eprintln!("Connection closed");
                                break 'outer;
                            },
                            Err(e) => {
                                eprintln!("Failed to read message content: {}", e);
                                break 'outer;
                            }
                        }
                    }
                }
            },
            Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => {
                // No data available, check if we should terminate
                if !running.load(Ordering::SeqCst) {
                    break 'outer;
                }
                // Short sleep to avoid spinning CPU
                std::thread::sleep(std::time::Duration::from_millis(100));
                continue;
            },
            Err(e) => {
                eprintln!("Error reading from stdin: {}", e);
                break 'outer;
            },
        }
    }
    
    // Save any pending changes before exit
    if let Err(e) = save_memory_store(&memory_store) {
        eprintln!("Failed to save memory store during shutdown: {}", e);
    }
    
    println!("Memory MCP server gracefully shut down");
    Ok(())
} 