use serde::{Deserialize, Serialize};

pub mod types;
pub use types::*;

/// Enum defining messages passed between HAPPE and IDA.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum InternalMessage {
    /// HAPPE -> IDA: Request relevant memories for a given query.
    GetMemoriesRequest { 
        query: String,
        /// Optional recent conversation context to help with memory selection
        conversation_context: Option<String> 
    },
    /// IDA -> HAPPE: Response containing retrieved memories.
    GetMemoriesResponse { memories: Vec<MemoryItem> },
    /// HAPPE -> IDA: Asynchronous notification with data from a completed turn for storage.
    StoreTurnRequest { turn_data: ConversationTurn },
    // TODO: Add other messages as needed, e.g., health checks, configuration updates?
}
