use anyhow::{Context, Result};
use gemini_ipc::internal_messages::ConversationTurn;
use gemini_memory::MemoryStore;
use std::sync::Arc;
use tracing::{debug, info, instrument};

/// Summarizes a conversation turn into a memory item.
/// This is called by the storage handler to prepare data for persistent storage.
fn summarize_turn(turn: &ConversationTurn) -> String {
    // Simple format: User query followed by AI response
    format!("User: {}\nAI: {}", turn.user_query, turn.llm_response)
}

/// Handles the storage of conversation turns in the memory store.
/// This runs as a background task to avoid blocking the IPC server.
#[instrument(skip(turn_data, memory_store), name = "background_storage_task")]
pub async fn handle_storage(
    turn_data: ConversationTurn,
    memory_store: Arc<MemoryStore>,
) -> Result<()> {
    info!("Processing turn data for storage");

    // 1. Analyze and summarize the turn data
    let memory_content = summarize_turn(&turn_data);
    debug!("Summarized memory content: {}", memory_content);

    // 2. Generate a key for this memory
    // Use the user query as the base for the key, but clean it first
    let key = format!(
        "turn_{}",
        turn_data
            .user_query
            .chars()
            .take(40)
            .collect::<String>()
            .replace(' ', "_")
    );
    debug!("Generated memory key: {}", key);

    // 3. Assign tags
    // For now, we'll just use a generic tag, but this could be enhanced with topic extraction
    let tags = vec!["conversation_turn".to_string()];

    // 4. Check for duplicates (semantic similarity)
    // This uses the MemoryStore's search capabilities
    let similar_memories = memory_store
        .get_semantically_similar(&turn_data.user_query, 5, 0.8)
        .await
        .context("Failed to search for similar memories")?;

    if !similar_memories.is_empty() {
        for (memory, score) in &similar_memories {
            debug!(
                "Found similar memory: key={}, score={:.2}, content={}",
                memory.key, score, memory.value
            );
        }

        // If we have a very similar memory (high score), consider it a duplicate
        if similar_memories.iter().any(|(_, score)| *score > 0.95) {
            info!("Memory appears to be a duplicate, skipping storage");
            return Ok(());
        }
    }

    // 5. Store the memory
    info!("Storing new memory with key: {}", key);
    memory_store
        .add_memory(
            &key,
            &memory_content,
            tags,
            None,                             // session_id
            Some("conversation".to_string()), // source
            None,                             // related_keys
        )
        .await
        .context("Failed to add memory to store")?;

    info!("Successfully stored memory");
    Ok(())
}
