# Gemini Memory Crate (`gemini-memory`)

This crate provides persistent memory storage and retrieval capabilities, primarily designed to give context awareness to LLM interactions within the Gemini Suite.

It uses LanceDB as a vector database for efficient semantic search and relies on an external embedding service (accessed via MCP) to generate vector representations of memory content.

## Features

*   **Persistent Memory Storage**: Stores `Memory` objects (key-value pairs with metadata) in a LanceDB database.
*   **Vector Embeddings**: Automatically generates vector embeddings for memory values using an external embedding tool accessed via the `McpHostInterface`. Supports different embedding model variants (`Small`, `Base`, `Large`).
*   **Semantic Search**: Provides `get_semantically_similar` function to retrieve memories based on vector similarity to a query, enabling context retrieval based on meaning rather than just keywords.
*   **Metadata & Filtering**: Stores and allows filtering by metadata such as tags, timestamps, source, session ID, and related keys.
*   **Standard CRUD & Retrieval**: Offers functions for adding (`add_memory`), updating (`update_memory`), deleting (`delete_by_key`), and retrieving memories by key, tag, or time range.
*   **Prompt Enhancement**: Includes `enhance_prompt` function to automatically retrieve relevant memories based on a user prompt and prepend them as context.
*   **MCP Integration**: Defines the `McpHostInterface` trait, specifying the required functions (like `execute_tool` for embeddings) that the MCP host (`gemini-mcp`) must provide.
*   **Data Schema**: Uses Apache Arrow for its data schema, including fields for metadata and the vector embedding.

## Core Concepts

1.  **`MemoryStore`**: The main struct that manages the LanceDB connection and table, providing methods for interacting with memories.
2.  **`Memory`**: A struct representing a single piece of information stored, containing the key, value, timestamp, tags, and other metadata.
3.  **Embeddings**: Numerical vector representations of the memory `value` text, generated externally.
4.  **LanceDB**: The underlying vector database used for storage and efficient similarity search.
5.  **`McpHostInterface`**: A trait defining the dependency on an MCP host for generating embeddings. The `MemoryStore` requires an implementation of this trait during initialization.
6.  **Prompt Enhancement**: The process of retrieving relevant past memories based on a current query and adding them to the prompt sent to the LLM.

## Modules

*   `store`: Contains the `MemoryStore` implementation, handling LanceDB interaction, embedding generation calls, and CRUD/search operations.
*   `broker`: Defines the `McpHostInterface` trait and provides the `enhance_prompt` functionality.
*   `memory`: Defines the `Memory` struct.
*   `schema`: Defines the Apache Arrow schema for LanceDB and the `EmbeddingModelVariant` enum.
*   `arrow_conversion`: Contains utility functions for converting `Memory` data to/from Arrow `RecordBatch` format.
*   `config`: Provides helper functions to determine the default path for the memory database (`~/.local/share/gemini-cli/memory.db`).
*   `errors`: Defines error types specific to the memory crate.

## Usage

This crate is typically used internally by other components of the Gemini Suite (like a CLI or application layer) that need to provide memory capabilities to the LLM. It requires an implementation of the `McpHostInterface` (usually provided by `gemini-mcp`).

```rust
use gemini_memory::{MemoryStore, broker::McpHostInterface, schema::EmbeddingModelVariant};
use gemini_core::{/* ... needed core types ... */ Request, Response, JsonRpcError, ServerCapabilities};
// Assume you have a concrete implementation of McpHostInterface, e.g., from gemini-mcp
// use gemini_mcp::McpHost;
use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::error::Error;

// --- Dummy McpHost for demonstration --- 
#[derive(Debug, Clone)]
struct DummyMcpHost;

#[async_trait]
impl McpHostInterface for DummyMcpHost {
    async fn execute_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        params: Value,
    ) -> Result<Value, Box<dyn Error>> {
        println!("[DummyMcpHost] execute_tool called: {}/{}, params: {}", server_name, tool_name, params);
        if server_name == "embedding" && tool_name == "embed" {
            // Simulate returning a dummy embedding vector
            let dim = EmbeddingModelVariant::default().dimension(); // Get default dimension
            let dummy_embedding = vec![0.1; dim];
            Ok(serde_json::json!({ "embedding": dummy_embedding }))
        } else {
            Err("Dummy tool not implemented".into())
        }
    }

    async fn get_all_capabilities(&self) -> gemini_memory::broker::Capabilities {
        println!("[DummyMcpHost] get_all_capabilities called");
        // Simulate returning capabilities including the embedding tool
        gemini_memory::broker::Capabilities { 
            tools: vec![gemini_memory::broker::ToolDefinition { name: "embedding/embed".to_string() }] 
        }
    }
    
    // Implement other required methods (send_request, get_capabilities) minimally
    async fn send_request(&self, _request: Request) -> Result<Response, JsonRpcError> {
        Err(JsonRpcError { code: -32601, message: "Not implemented".to_string(), data: None })
    }
    async fn get_capabilities(&self) -> Result<ServerCapabilities, String> {
        Err("Not implemented".to_string())
    }
}
// --- End Dummy McpHost --- 

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Create an instance of the McpHostInterface (using the Dummy for this example)
    let mcp_host: Arc<dyn McpHostInterface + Send + Sync> = Arc::new(DummyMcpHost);

    // 2. Initialize the MemoryStore (uses default path and embedding model)
    let memory_store = MemoryStore::new(None, None, Some(mcp_host.clone())).await?;
    let store_arc = Arc::new(memory_store);

    // 3. Add some memories
    store_arc.add_memory(
        "project_alpha_goal",
        "The goal of Project Alpha is to develop a new AI assistant.",
        vec!["project:alpha".to_string(), "planning".to_string()],
        None, None, None
    ).await?;
    
    store_arc.add_memory(
        "project_beta_status",
        "Project Beta is currently blocked by resource allocation.",
        vec!["project:beta".to_string(), "status".to_string()],
        None, None, None
    ).await?;

    println!("Memories added.");

    // 4. Enhance a user prompt
    let user_prompt = "What is Project Alpha about?";
    let enhanced_prompt = gemini_memory::broker::enhance_prompt(
        user_prompt,
        &store_arc,
        3,    // Retrieve top 3 relevant memories
        0.7,  // Minimum relevance score
    ).await?;

    println!("\nOriginal Prompt: {}", user_prompt);
    println!("\nEnhanced Prompt:\n{}", enhanced_prompt);

    // 5. Perform a semantic search
    let search_query = "information about AI projects";
    let similar_memories = store_arc.get_semantically_similar(
        search_query,
        5,    // Retrieve top 5
        0.5,  // Minimum relevance
    ).await?;

    println!("\nMemories similar to '{}':", search_query);
    for (memory, score) in similar_memories {
        println!(" - Key: {}, Value: \"{}\", Score: {:.2}", memory.key, memory.value, score);
    }

    Ok(())
}
```

**Note**: This example uses a `DummyMcpHost` for demonstration. In a real application, you would provide an actual implementation, likely obtained from initializing `gemini-mcp::McpHost`. 