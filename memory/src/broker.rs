use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use crate::store::MemoryStore;
use anyhow::{Result, anyhow};
use tracing::debug;

// Use shared RPC types from gemini_core (specific exports)
use gemini_core::{JsonRpcError, Request, Response, ServerCapabilities};

// Assuming McpHostInterface needs types from gemini_mcp::rpc
// We need to figure out the right dependency structure or type sharing strategy.
// For now, let's assume these types might be duplicated or need to come from core/mcp.
// Placeholder types to satisfy the compiler for now:
// use gemini_core::types::{FunctionCall, FunctionResponse}; // Example: Assuming these might be relevant somehow or need replacement
// use gemini_mcp::rpc::{...}; // Removed

// Use async_trait for the trait
#[async_trait]
/// Interface for MCP host functions needed by the memory broker
pub trait McpHostInterface: Send + Sync + 'static {
    /// Execute a tool on an MCP server, returning the result
    async fn execute_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        params: Value,
    ) -> Result<Value, Box<dyn Error>>;

    /// Get all capabilities (tools) from connected MCP servers
    async fn get_all_capabilities(&self) -> Capabilities;

    async fn send_request(&self, request: Request) -> Result<Response, JsonRpcError>;
    async fn get_capabilities(&self) -> Result<ServerCapabilities, String>;
}

/// Simplified capabilities structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Capabilities {
    /// Available MCP tools
    #[serde(rename = "capabilities")]
    pub tools: Vec<ToolDefinition>,
}

/// Simplified tool definition
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolDefinition {
    /// Tool name in format "server_name/tool_name"
    #[serde(rename = "toolName")]
    pub name: String,
}

// Remove unused pub(crate) functions
// pub(crate) async fn retrieve_all_memories(...) { ... }
// pub(crate) async fn filter_relevant_memories(...) { ... }
// pub(crate) async fn enhance_query(...) { ... }
// pub(crate) async fn deduplicate_memories(...) { ... }

/// Retrieves relevant memories and formats them into a context string.
async fn get_memory_context(
    store: &MemoryStore,
    query: &str,
    top_k: usize,
    min_relevance: f32,
) -> Result<String> {
    debug!(
        "Retrieving top {} memories similar to query (min relevance: {})",
        top_k,
        min_relevance
    );
    let similar_memories = store
        .get_semantically_similar(query, top_k, min_relevance)
        .await
        .map_err(|e| anyhow!("Failed to retrieve similar memories: {}", e))?;

    if similar_memories.is_empty() {
        debug!("No relevant memories found for the query.");
        return Ok(String::new());
    }

    let context_header = "Relevant information from past interactions:";
    let mut context_parts = vec![context_header.to_string()];

    for (memory, score) in similar_memories {
        // Simple formatting: Key, Value, Score
        let entry = format!(
            "- Key: {}\n  Value: {}\n  (Relevance: {:.2})",
            memory.key,
            memory.value.trim(), // Trim whitespace from value
            score
        );
        context_parts.push(entry);
    }

    Ok(context_parts.join("\n\n"))
}

/// Enhances a user prompt with relevant context from the MemoryStore.
pub async fn enhance_prompt(
    prompt: &str,
    store: &MemoryStore,
    top_k: usize,
    min_relevance: f32,
) -> Result<String> {
    let memory_context = get_memory_context(store, prompt, top_k, min_relevance).await?;

    if memory_context.is_empty() {
        Ok(prompt.to_string()) // No enhancement needed
    } else {
        // Prepend the context to the user's prompt
        Ok(format!(
            "{}

---

User Query:
{}",
            memory_context,
            prompt
        ))
    }
}
