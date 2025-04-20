use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents a memory entry stored in the memory store
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Unique identifier for the memory
    pub key: String,
    
    /// The actual content of the memory
    pub value: String,
    
    /// Tags associated with this memory for filtering and categorization
    pub tags: Vec<String>,
    
    /// Optional namespace for organizing memories
    pub namespace: Option<String>,
    
    /// Optional source identifier (e.g., "cli", "webapp")
    pub source: Option<String>,
    
    /// Optional timestamp when the memory was created
    pub created_at: Option<u64>,
    
    /// Optional metadata for additional context
    pub metadata: Option<Value>,
}

impl Memory {
    /// Create a new memory
    pub fn new(
        key: String,
        value: String,
        tags: Vec<String>,
        namespace: Option<String>,
        source: Option<String>,
        metadata: Option<Value>,
    ) -> Self {
        Self {
            key,
            value,
            tags,
            namespace,
            source,
            created_at: Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
            metadata,
        }
    }
}

/// Configuration options for the AsyncMemoryStore
#[derive(Debug, Clone)]
pub struct AsyncMemoryConfig {
    /// Whether to use async embedding operations
    pub enable_async: bool,
    
    /// Size of the embedding job queue
    pub queue_size: Option<usize>,
    
    /// Number of worker tasks for processing embedding jobs
    pub worker_count: Option<usize>,
}

impl Default for AsyncMemoryConfig {
    fn default() -> Self {
        Self {
            enable_async: true,
            queue_size: None,
            worker_count: None,
        }
    }
} 