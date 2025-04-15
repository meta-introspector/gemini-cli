use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Memory structure for storing key-value memories
#[derive(Debug, Serialize, Deserialize, Clone)]
struct Memory {
    key: String,
    value: String,
    timestamp: u64,
    tags: Vec<String>,
}

/// MemoryStore manages a collection of memories
#[derive(Debug, Serialize, Deserialize, Default)]
struct MemoryStore {
    memories: Vec<Memory>,
}

impl MemoryStore {
    /// Create a new empty memory store
    fn new() -> Self {
        Self {
            memories: Vec::new(),
        }
    }

    /// Add a new memory with the given key and value
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

    /// Get memories by key (exact match)
    fn get_by_key(&self, key: &str) -> Vec<Memory> {
        self.memories
            .iter()
            .filter(|m| m.key == key)
            .cloned()
            .collect()
    }

    /// Get memories by tag
    fn get_by_tag(&self, tag: &str) -> Vec<Memory> {
        self.memories
            .iter()
            .filter(|m| m.tags.contains(&tag.to_string()))
            .cloned()
            .collect()
    }

    /// Get all memories
    fn get_all_memories(&self) -> Vec<Memory> {
        self.memories.clone()
    }

    /// Delete memory by key
    fn delete_by_key(&mut self, key: &str) -> usize {
        let initial_len = self.memories.len();
        self.memories.retain(|m| m.key != key);
        initial_len - self.memories.len()
    }
}

/// Get the path for the memory store file
fn get_memory_store_path() -> PathBuf {
    let mut config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    config_dir.push("gemini-cli");
    config_dir.push("memory_store.json");
    config_dir
}

/// Ensure the memory directory exists
fn ensure_memory_dir() -> Result<PathBuf, io::Error> {
    let memory_path = get_memory_store_path();
    if let Some(parent) = memory_path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(memory_path)
}

/// Load memory store from disk
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

/// Save memory store to disk
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

/// Extract relevant memories for a user query using Gemini-2.0-flash
async fn get_relevant_memories(
    query: &str,
    api_key: &str,
    memory_store: &MemoryStore,
) -> Result<Vec<Memory>, String> {
    // Get all memories
    let memories = memory_store.get_all_memories();
    
    if memories.is_empty() {
        return Ok(Vec::new());
    }
    
    // Format memories for the model
    let memories_text = memories
        .iter()
        .map(|m| format!("- Key: \"{}\", Value: \"{}\", Tags: [{}]", 
                         m.key, 
                         m.value, 
                         m.tags.join(", ")))
        .collect::<Vec<_>>()
        .join("\n");
    
    // Create prompt for Gemini-2.0-flash
    let prompt = format!(
        "You are a memory retrieval system that determines what stored information is relevant to a user's query.
        
        STORED MEMORIES:
        {}
        
        USER QUERY:
        \"{}\"
        
        Task: Analyze the user's query and return ONLY the keys of memories that are directly relevant to answering this query.
        Return the keys as a JSON array of strings, e.g., [\"key1\", \"key2\"]
        If no memories are relevant, return an empty array: []
        DO NOT include any explanation, just the JSON array.",
        memories_text,
        query
    );
    
    // Call Gemini-2.0-flash model
    let client = reqwest::Client::new();
    let url = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent";
    
    let request_body = json!({
        "contents": [{
            "role": "user",
            "parts": [{
                "text": prompt
            }]
        }]
    });
    
    let response = match client
        .post(&format!("{}?key={}", url, api_key))
        .json(&request_body)
        .send()
        .await {
            Ok(r) => r,
            Err(e) => return Err(format!("Failed to call Gemini API: {}", e)),
        };
    
    if !response.status().is_success() {
        let error_text = match response.text().await {
            Ok(t) => t,
            Err(e) => return Err(format!("Failed to read response: {}", e)),
        };
        return Err(format!("Failed to query Gemini model: {}", error_text));
    }
    
    let response_json: Value = match response.json().await {
        Ok(j) => j,
        Err(e) => return Err(format!("Failed to parse JSON response: {}", e)),
    };
    
    // Extract the generated content and parse the JSON array
    let text = match response_json["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        Some(t) => t,
        None => return Err("Failed to extract text from response".to_string()),
    };
    
    // Try to parse the response as a JSON array of strings
    let keys: Vec<String> = match serde_json::from_str(text) {
        Ok(keys) => keys,
        Err(e) => {
            // If parsing fails, try to extract JSON array from text (in case model outputs extra text)
            if let Some(start) = text.find('[') {
                if let Some(end) = text.rfind(']') {
                    let json_str = &text[start..=end];
                    match serde_json::from_str(json_str) {
                        Ok(keys) => keys,
                        Err(_) => return Err(format!("Failed to parse memory keys from response: {}", e)),
                    }
                } else {
                    return Err(format!("Failed to parse memory keys from response: {}", e));
                }
            } else {
                return Err(format!("Failed to parse memory keys from response: {}", e));
            }
        }
    };
    
    // Get the actual memories for the keys
    let relevant_memories: Vec<Memory> = memories
        .into_iter()
        .filter(|m| keys.contains(&m.key))
        .collect();
    
    Ok(relevant_memories)
}

/// Helper function to call get_relevant_memories with a Vec<Memory> instead of MemoryStore
async fn get_relevant_memories_from_vec(
    query: &str,
    api_key: &str,
    memories: &Vec<Memory>,
) -> Result<Vec<Memory>, String> {
    // Convert Vec<Memory> to a temporary MemoryStore
    let temp_store = MemoryStore {
        memories: memories.clone(),
    };
    
    // Call the original function
    get_relevant_memories(query, api_key, &temp_store).await
}

fn main() {
    // Initialize the runtime for async functions
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create runtime: {}", e);
            process::exit(1);
        }
    };
    
    // Run the server
    if let Err(e) = rt.block_on(process_stdin()) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}

async fn process_stdin() -> Result<(), String> {
    // Load memory store
    let memory_store = Arc::new(Mutex::new(match load_memory_store() {
        Ok(store) => store,
        Err(e) => {
            eprintln!("Failed to load memory store: {}", e);
            MemoryStore::new()
        }
    }));
    
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();
    
    let mut buffer = String::new();
    
    loop {
        buffer.clear();
        match stdin_lock.read_line(&mut buffer) {
            Ok(0) => break, // End of stream
            Ok(_) => {
                match serde_json::from_str::<Request>(&buffer) {
                    Ok(request) => {
                        let response = match request.method.as_str() {
                            "initialize" => {
                                // Return server capabilities
                                let server_info = ServerInfo {
                                    name: "memory-mcp".to_string(),
                                    version: "1.0.0".to_string(),
                                };
                                
                                let capabilities = ServerCapabilities {
                                    tools: vec![
                                        Tool {
                                            name: "store_memory".to_string(),
                                            description: "Store a new memory in the persistent memory store".to_string(),
                                            schema: Some(json!({
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
                                            description: "Retrieve memories by key".to_string(),
                                            schema: Some(json!({
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
                                            description: "Retrieve memories by tag".to_string(),
                                            schema: Some(json!({
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
                                            description: "List all stored memories".to_string(),
                                            schema: Some(json!({
                                                "type": "object",
                                                "properties": {},
                                                "required": []
                                            })),
                                        },
                                        Tool {
                                            name: "delete_memory".to_string(),
                                            description: "Delete a memory by key".to_string(),
                                            schema: Some(json!({
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
                                        Tool {
                                            name: "get_relevant_memories".to_string(),
                                            description: "Get memories relevant to a query using Gemini-2.0-flash".to_string(),
                                            schema: Some(json!({
                                                "type": "object",
                                                "properties": {
                                                    "query": {
                                                        "type": "string",
                                                        "description": "Query to find relevant memories for"
                                                    },
                                                    "api_key": {
                                                        "type": "string",
                                                        "description": "Gemini API key for model access"
                                                    }
                                                },
                                                "required": ["query", "api_key"]
                                            })),
                                        },
                                    ],
                                    resources: vec![
                                        Resource {
                                            name: "memory_stats".to_string(),
                                            description: "Get statistics about the memory store".to_string(),
                                            schema: None,
                                        },
                                        Resource {
                                            name: "memory_schema".to_string(),
                                            description: "Get the schema of memory objects".to_string(),
                                            schema: None,
                                        },
                                    ],
                                };
                                
                                let result = InitializeResult {
                                    server_info,
                                    capabilities,
                                };
                                
                                Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: request.id.unwrap_or(json!(null)),
                                    result: Some(json!(result)),
                                    error: None,
                                }
                            },
                            "mcp/tool/execute" => {
                                if let Some(params) = request.params {
                                    if let Some(tool_name) = params.get("tool_name").and_then(Value::as_str) {
                                        if let Some(arguments) = params.get("arguments") {
                                            match tool_name {
                                                "store_memory" => {
                                                    let key = match arguments.get("key").and_then(Value::as_str) {
                                                        Some(k) => k,
                                                        None => {
                                                            let response = send_error_response(request.id, -32602, "Missing key parameter");
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err("Missing key parameter".to_string());
                                                        },
                                                    };
                                                    let value = match arguments.get("value").and_then(Value::as_str) {
                                                        Some(v) => v,
                                                        None => {
                                                            let response = send_error_response(request.id, -32602, "Missing value parameter");
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err("Missing value parameter".to_string());
                                                        },
                                                    };
                                                        
                                                    let tags = match arguments.get("tags") {
                                                        Some(tag_arr) if tag_arr.is_array() => {
                                                            tag_arr.as_array()
                                                                .unwrap()
                                                                .iter()
                                                                .filter_map(|t| t.as_str().map(String::from))
                                                                .collect()
                                                        },
                                                        _ => Vec::new(),
                                                    };
                                                    
                                                    let mut store = match memory_store.lock() {
                                                        Ok(store) => store,
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to lock memory store: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to lock memory store: {}", e));
                                                        },
                                                    };
                                                    
                                                    match store.add_memory(key, value, tags) {
                                                        Ok(_) => (),
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to add memory: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to add memory: {}", e));
                                                        },
                                                    }
                                                    
                                                    match save_memory_store(&store) {
                                                        Ok(_) => (),
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to save memory store: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to save memory store: {}", e));
                                                        },
                                                    }
                                                    
                                                    Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!({
                                                            "success": true,
                                                            "message": format!("Memory stored with key: {}", key)
                                                        })),
                                                        error: None,
                                                    }
                                                },
                                                "retrieve_memory" => {
                                                    let key = match arguments.get("key").and_then(Value::as_str) {
                                                        Some(k) => k,
                                                        None => {
                                                            let response = send_error_response(request.id, -32602, "Missing key parameter");
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err("Missing key parameter".to_string());
                                                        },
                                                    };
                                                    
                                                    let store = match memory_store.lock() {
                                                        Ok(store) => store,
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to lock memory store: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to lock memory store: {}", e));
                                                        },
                                                    };
                                                    
                                                    let memories = store.get_by_key(key);
                                                    
                                                    Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!({
                                                            "memories": memories,
                                                            "count": memories.len()
                                                        })),
                                                        error: None,
                                                    }
                                                },
                                                "retrieve_by_tag" => {
                                                    let tag = match arguments.get("tag").and_then(Value::as_str) {
                                                        Some(t) => t,
                                                        None => {
                                                            let response = send_error_response(request.id, -32602, "Missing tag parameter");
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err("Missing tag parameter".to_string());
                                                        },
                                                    };
                                                    
                                                    let store = match memory_store.lock() {
                                                        Ok(store) => store,
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to lock memory store: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to lock memory store: {}", e));
                                                        },
                                                    };
                                                    
                                                    let memories = store.get_by_tag(tag);
                                                    
                                                    Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!({
                                                            "memories": memories,
                                                            "count": memories.len()
                                                        })),
                                                        error: None,
                                                    }
                                                },
                                                "list_all_memories" => {
                                                    let store = match memory_store.lock() {
                                                        Ok(store) => store,
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to lock memory store: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to lock memory store: {}", e));
                                                        },
                                                    };
                                                    
                                                    let memories = store.get_all_memories();
                                                    
                                                    Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!({
                                                            "memories": memories,
                                                            "count": memories.len()
                                                        })),
                                                        error: None,
                                                    }
                                                },
                                                "delete_memory" => {
                                                    let key = match arguments.get("key").and_then(Value::as_str) {
                                                        Some(k) => k,
                                                        None => {
                                                            let response = send_error_response(request.id, -32602, "Missing key parameter");
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err("Missing key parameter".to_string());
                                                        },
                                                    };
                                                    
                                                    let mut store = match memory_store.lock() {
                                                        Ok(store) => store,
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to lock memory store: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to lock memory store: {}", e));
                                                        },
                                                    };
                                                    
                                                    let count = store.delete_by_key(key);
                                                    
                                                    match save_memory_store(&store) {
                                                        Ok(_) => (),
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to save memory store: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to save memory store: {}", e));
                                                        },
                                                    }
                                                    
                                                    Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!({
                                                            "success": true,
                                                            "deleted_count": count,
                                                            "message": format!("Deleted {} memories with key: {}", count, key)
                                                        })),
                                                        error: None,
                                                    }
                                                },
                                                "get_relevant_memories" => {
                                                    let query = match arguments.get("query").and_then(Value::as_str) {
                                                        Some(q) => q,
                                                        None => {
                                                            let response = send_error_response(request.id, -32602, "Missing query parameter");
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err("Missing query parameter".to_string());
                                                        },
                                                    };
                                                    let api_key = match arguments.get("api_key").and_then(Value::as_str) {
                                                        Some(k) => k,
                                                        None => {
                                                            let response = send_error_response(request.id, -32602, "Missing API key parameter");
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err("Missing API key parameter".to_string());
                                                        },
                                                    };
                                                    
                                                    let store_clone = {
                                                        let store = match memory_store.lock() {
                                                            Ok(store) => store,
                                                            Err(e) => {
                                                                let response = send_error_response(request.id, -32603, &format!("Failed to lock memory store: {}", e));
                                                                serde_json::to_string(&response)
                                                                    .map(|s| writeln!(stdout_lock, "{}", s))
                                                                    .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                                return Err(format!("Failed to lock memory store: {}", e));
                                                            },
                                                        };
                                                        store.get_all_memories()
                                                    };
                                                    
                                                    // Use the memory broker to filter relevant memories
                                                    let relevant_memories = match get_relevant_memories_from_vec(query, api_key, &store_clone).await {
                                                        Ok(memories) => memories,
                                                        Err(e) => {
                                                            let response = send_error_response(request.id, -32603, &format!("Failed to get relevant memories: {}", e));
                                                            serde_json::to_string(&response)
                                                                .map(|s| writeln!(stdout_lock, "{}", s))
                                                                .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                            return Err(format!("Failed to get relevant memories: {}", e));
                                                        },
                                                    };
                                                    
                                                    Response {
                                                        jsonrpc: "2.0".to_string(),
                                                        id: request.id.unwrap_or(json!(null)),
                                                        result: Some(json!({
                                                            "memories": relevant_memories,
                                                            "count": relevant_memories.len()
                                                        })),
                                                        error: None,
                                                    }
                                                },
                                                _ => send_error_response(request.id, -32601, &format!("Tool not found: {}", tool_name)),
                                            }
                                        } else {
                                            send_error_response(request.id, -32602, "Missing arguments parameter")
                                        }
                                    } else {
                                        send_error_response(request.id, -32602, "Missing tool_name parameter")
                                    }
                                } else {
                                    send_error_response(request.id, -32602, "Missing params")
                                }
                            },
                            "mcp/resource/get" => {
                                if let Some(params) = request.params {
                                    if let Some(resource_name) = params.get("resource_name").and_then(Value::as_str) {
                                        match resource_name {
                                            "memory_stats" => {
                                                let store = match memory_store.lock() {
                                                    Ok(store) => store,
                                                    Err(e) => {
                                                        let response = send_error_response(request.id, -32603, &format!("Failed to lock memory store: {}", e));
                                                        serde_json::to_string(&response)
                                                            .map(|s| writeln!(stdout_lock, "{}", s))
                                                            .map_err(|e| format!("Failed to serialize response: {}", e))?;
                                                        return Err(format!("Failed to lock memory store: {}", e));
                                                    },
                                                };
                                                
                                                let memories = store.get_all_memories();
                                                let total_memories = memories.len();
                                                
                                                // Collect unique tags
                                                let mut tags = HashSet::new();
                                                for memory in &memories {
                                                    for tag in &memory.tags {
                                                        tags.insert(tag.clone());
                                                    }
                                                }
                                                
                                                Response {
                                                    jsonrpc: "2.0".to_string(),
                                                    id: request.id.unwrap_or(json!(null)),
                                                    result: Some(json!({
                                                        "total_memories": total_memories,
                                                        "unique_tags": tags.len(),
                                                        "tags": tags
                                                    })),
                                                    error: None,
                                                }
                                            },
                                            "memory_schema" => {
                                                Response {
                                                    jsonrpc: "2.0".to_string(),
                                                    id: request.id.unwrap_or(json!(null)),
                                                    result: Some(json!({
                                                        "type": "object",
                                                        "properties": {
                                                            "key": {
                                                                "type": "string",
                                                                "description": "Unique identifier for the memory"
                                                            },
                                                            "value": {
                                                                "type": "string",
                                                                "description": "Content of the memory"
                                                            },
                                                            "timestamp": {
                                                                "type": "integer",
                                                                "description": "Unix timestamp when the memory was created"
                                                            },
                                                            "tags": {
                                                                "type": "array",
                                                                "items": {
                                                                    "type": "string"
                                                                },
                                                                "description": "Tags for categorizing the memory"
                                                            }
                                                        }
                                                    })),
                                                    error: None,
                                                }
                                            },
                                            _ => send_error_response(request.id, -32601, &format!("Resource not found: {}", resource_name)),
                                        }
                                    } else {
                                        send_error_response(request.id, -32602, "Missing resource_name parameter")
                                    }
                                } else {
                                    send_error_response(request.id, -32602, "Missing params")
                                }
                            },
                            _ => send_error_response(request.id, -32601, &format!("Method not found: {}", request.method)),
                        };
                        
                        let response_json = match serde_json::to_string(&response) {
                            Ok(json) => json,
                            Err(e) => {
                                eprintln!("Error serializing response: {}", e);
                                continue;
                            }
                        };
                        
                        if let Err(e) = writeln!(stdout_lock, "{}", response_json) {
                            eprintln!("Error writing response: {}", e);
                        }
                        
                        if let Err(e) = stdout_lock.flush() {
                            eprintln!("Error flushing output: {}", e);
                        }
                    },
                    Err(e) => {
                        eprintln!("Error parsing request: {}", e);
                    }
                }
            },
            Err(e) => {
                eprintln!("Error reading line: {}", e);
                return Err(format!("Error reading from stdin: {}", e));
            }
        }
    }
    
    Ok(())
}

fn send_error_response(id: Option<Value>, code: i64, message: &str) -> Response {
    Response {
        jsonrpc: "2.0".to_string(),
        id: id.unwrap_or(json!(null)),
        result: None,
        error: Some(JsonRpcError {
            code,
            message: message.to_string(),
            data: None,
        }),
    }
} 