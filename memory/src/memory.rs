use serde::{Deserialize, Serialize};

/// Memory structure for storing individual memory items
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Memory {
    /// The key/identifier for this memory
    pub key: String,
    /// The content/value of the memory
    pub value: String,
    /// Timestamp when the memory was created (Unix timestamp)
    pub timestamp: u64,
    /// Optional tags for categorizing and filtering memories
    pub tags: Vec<String>,
    /// Optional token count for the memory
    pub token_count: Option<usize>,
    /// ID of the originating session/chat
    pub session_id: Option<String>,
    /// Origin of the memory (e.g., "user", "ai", "tool_result:<tool_name>")
    pub source: Option<String>,
    /// Keys of related memories
    pub related_keys: Option<Vec<String>>,
    /// Relevance/confidence score (e.g., from search)
    pub confidence_score: Option<f32>,
}

impl Memory {
    // ... existing code ...
}
