use gemini_ipc::internal_messages::MemoryItem;
use gemini_memory::{MemoryStore, Memory};
use std::sync::Arc;
use tracing::{info, instrument, debug};
use anyhow::{Result, Context};

/// Error type for memory operations
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Memory storage error: {0}")]
    Storage(#[from] anyhow::Error),
    #[error("No memory store available")]
    NoStore,
}

/// Converts a Memory from the memory store to a MemoryItem for IPC transfer
fn memory_to_memory_item(memory: &Memory) -> MemoryItem {
    MemoryItem {
        content: memory.value.clone(),
    }
}

/// Retrieves memories relevant to the given query
#[instrument(skip(query, memory_store))]
pub async fn retrieve_memories(
    query: &str, 
    memory_store: Arc<MemoryStore>,
    max_results: usize,
) -> Result<Vec<MemoryItem>, MemoryError> {
    info!("Retrieving memories for query: {}", query);
    
    // Retrieve semantically similar memories with a minimum relevance threshold
    let similar_memories = memory_store
        .get_semantically_similar(query, max_results, 0.6)
        .await
        .context("Failed to retrieve semantically similar memories")?;
    
    info!("Found {} potentially relevant memories", similar_memories.len());
    
    // Convert to MemoryItems for IPC transfer, including only memories above threshold
    let memory_items: Vec<MemoryItem> = similar_memories
        .into_iter()
        .map(|(memory, score)| {
            debug!("Memory match (score={:.2}): {}", score, memory.value);
            memory_to_memory_item(&memory)
        })
        .collect();
    
    Ok(memory_items)
}

/// Format memories to enhance a prompt with context
/// This is a utility function that could be used directly by HAPPE
/// but is kept here for reference as part of the memory system
pub fn format_memories_for_prompt(memories: &[MemoryItem], max_token_estimate: usize) -> String {
    if memories.is_empty() {
        return String::new();
    }
    
    // Very simple token estimation (4 chars ~= 1 token)
    let estimate_tokens = |s: &str| -> usize {
        s.len() / 4
    };
    
    let mut result = String::from("Previous relevant interactions:\n");
    let mut total_tokens = estimate_tokens(&result);
    
    // Add memories until we hit the token limit
    for (i, memory) in memories.iter().enumerate() {
        let formatted = format!("{}: {}\n", i + 1, memory.content);
        let tokens = estimate_tokens(&formatted);
        
        if total_tokens + tokens > max_token_estimate {
            // Add an indication that more memories were available but truncated
            if i < memories.len() - 1 {
                result.push_str("(additional relevant context omitted due to length constraints)\n");
            }
            break;
        }
        
        result.push_str(&formatted);
        total_tokens += tokens;
    }
    
    result.push_str("\n");
    result
}

#[instrument(skip(turn_data))]
pub async fn check_duplicates(
    turn_data: &gemini_ipc::internal_messages::ConversationTurn,
) -> anyhow::Result<bool> {
    info!(
        "(Placeholder) Checking for duplicates for turn: {:?}",
        turn_data.user_query // Correct field name
    );
    // Simulate finding no duplicates
    Ok(false)
}

#[instrument(skip(turn_data))]
pub async fn store_memory(
    turn_data: &gemini_ipc::internal_messages::ConversationTurn,
) -> anyhow::Result<()> {
    info!(
        "(Placeholder) Storing memory for turn: {:?}",
        turn_data.user_query // Correct field name
    );
    // Simulate successful storage
    Ok(())
}

// TODO: Add chrono to Cargo.toml - No longer needed here as timestamp removed
// TODO: Add thiserror to Cargo.toml
