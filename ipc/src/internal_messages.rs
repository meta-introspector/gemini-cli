use serde::{Deserialize, Serialize};

/// Represents a message sent between HAPPE and IDA daemons.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InternalMessage {
    /// HAPPE -> IDA: Request relevant memories for a raw query.
    GetMemoriesRequest {
        query: String,
        // Optional: Add conversation ID or other context if needed
    },
    /// IDA -> HAPPE: Response containing retrieved memories.
    GetMemoriesResponse {
        memories: Vec<MemoryItem>, // Or Option<Vec<MemoryItem>>
    },
    /// HAPPE -> IDA (Async): Notification of a completed turn for storage.
    StoreTurnRequest {
        raw_query: String,
        retrieved_memories: Vec<MemoryItem>,
        llm_response: String,
        // Optional: Add timestamp, conversation ID, etc.
    },
    // No explicit response needed for StoreTurnRequest as it's async
    // Add other message types as needed for future IDA capabilities
}

/// Represents a single retrieved memory item.
/// Adapt this structure based on what MemoryStore actually stores and returns.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryItem {
    pub content: String,
    pub source: String, // e.g., filename, URL, previous conversation
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub score: Option<f32>, // Similarity score if applicable
    // Add other relevant metadata
}

// Placeholder for chrono types if not already imported/aliased
// You might need to add chrono to ipc/Cargo.toml if using timestamps here
// mod chrono {
//     pub use chrono::{DateTime, Utc};
// } 