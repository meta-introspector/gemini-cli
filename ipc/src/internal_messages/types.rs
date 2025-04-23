use serde::{Deserialize, Serialize};
use gemini_core::types::Content;

/// Represents a single piece of memory retrieved or stored.
/// TODO: Finalize fields - consider adding source, timestamp, embedding ID etc.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MemoryItem {
    pub content: String,
    // pub source: String,
    // pub timestamp: chrono::DateTime<chrono::Utc>,
    // pub embedding_id: Option<String>,
}

/// Represents the data from a completed conversation turn.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ConversationTurn {
    pub user_query: String,
    pub retrieved_memories: Vec<MemoryItem>,
    pub llm_response: String,
    pub turn_parts: Vec<Content>,
}
