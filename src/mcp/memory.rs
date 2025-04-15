// DEPRECATED: This module is deprecated.
// The memory functionality has been moved to a standalone server at src/mcp/servers/memory
//
// This file is kept for backward compatibility but should not be used.
// The standalone memory server should be used instead.

#[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
pub struct Memory;

#[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
pub struct MemoryStore;

// Empty implementations to prevent compilation errors for code that might still be using these types
impl MemoryStore {
    #[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
    pub fn new() -> Self {
        Self
    }

    #[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
    pub fn add_memory(&mut self, _key: &str, _value: &str, _tags: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    #[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
    pub fn get_by_key(&self, _key: &str) -> Vec<Memory> {
        Vec::new()
    }

    #[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
    pub fn get_all_memories(&self) -> Vec<Memory> {
        Vec::new()
    }
}

#[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
pub fn get_memory_store() -> std::sync::Arc<std::sync::Mutex<MemoryStore>> {
    std::sync::Arc::new(std::sync::Mutex::new(MemoryStore))
}

#[deprecated(since = "0.1.0", note = "Use the standalone memory MCP server in src/mcp/servers/memory instead")]
pub async fn get_relevant_memories(
    _query: &str,
    _api_key: &str,
) -> Result<Vec<Memory>, Box<dyn std::error::Error>> {
    Ok(Vec::new())
} 