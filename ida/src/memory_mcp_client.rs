use anyhow::{Context, Result};
use gemini_ipc::internal_messages::MemoryItem;
use gemini_memory::{Memory, MemoryStore};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, error, info, instrument, warn};

use crate::llm_clients::LLMClient;

/// Error type for memory operations
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("Memory storage error: {0}")]
    Storage(#[from] anyhow::Error),
    #[error("No memory store available")]
    NoStore,
    #[error("Broker LLM error: {0}")]
    BrokerError(String),
}

/// Converts a Memory from the memory store to a MemoryItem for IPC transfer
fn memory_to_memory_item(memory: &Memory) -> MemoryItem {
    MemoryItem {
        content: memory.value.clone(),
    }
}

/// Retrieves memories relevant to the given query, optionally using a broker LLM to refine results
#[instrument(skip(query, memory_store, broker_llm_client, conversation_context))]
pub async fn retrieve_memories(
    query: &str,
    memory_store: Arc<MemoryStore>,
    max_results: usize,
    broker_llm_client: &Option<Arc<dyn LLMClient + Send + Sync>>,
    conversation_context: Option<String>,
) -> Result<Vec<MemoryItem>, MemoryError> {
    info!("Retrieving memories for query: {}", query);

    // Step 1: Retrieve semantically similar memories with a minimum relevance threshold
    let similar_memories = memory_store
        .get_semantically_similar(query, max_results, 0.6)
        .await
        .context("Failed to retrieve semantically similar memories")?;

    info!(
        "Found {} potentially relevant memories via semantic search",
        similar_memories.len()
    );

    // If we found no memories or have no broker LLM, just return the raw semantic search results
    if similar_memories.is_empty() || broker_llm_client.is_none() {
        debug!("No broker LLM available or no memories found, returning raw semantic search results");
        // Convert to MemoryItems for IPC transfer
        let memory_items = similar_memories
            .into_iter()
            .map(|(memory, score)| {
                debug!("Memory match (score={:.2}): {}", score, memory.value);
                memory_to_memory_item(&memory)
            })
            .collect();
        return Ok(memory_items);
    }

    // Step 2: If we have a broker LLM available, use it to filter/refine the results
    let broker_client = broker_llm_client.as_ref().unwrap();
    info!(
        "Using broker LLM ({}) to refine {} memory results",
        broker_client.provider_name(),
        similar_memories.len()
    );

    // Construct a prompt for the broker LLM to help select the most relevant memories
    let mut prompt = format!(
        "You are a memory broker responsible for selecting the most relevant context for answering a user query.\n\n"
    );

    prompt.push_str(&format!("USER QUERY: {}\n\n", query));
    
    if let Some(context) = conversation_context {
        prompt.push_str(&format!("RECENT CONVERSATION CONTEXT:\n{}\n\n", context));
    }

    prompt.push_str("CANDIDATE MEMORIES:\n");
    
    // Create a map to lookup memories by key later
    let mut memory_map = std::collections::HashMap::new();
    
    for (i, (memory, score)) in similar_memories.iter().enumerate() {
        let memory_entry = format!(
            "[Memory {}] key: {}, relevance: {:.2}, content: {}\n",
            i + 1, memory.key, score, memory.value
        );
        prompt.push_str(&memory_entry);
        memory_map.insert(memory.key.clone(), memory.clone());
    }
    
    prompt.push_str("\nINSTRUCTIONS:\n");
    prompt.push_str("1. Analyze the user query and review the candidate memories.\n");
    prompt.push_str("2. Select ONLY the memories that provide directly useful information for answering the query.\n");
    prompt.push_str("3. Focus on factual relevance, not semantic similarity.\n");
    prompt.push_str("4. Respond ONLY with a comma-separated list of the memory keys you've selected (e.g., \"memory_key1,memory_key2\").\n");
    prompt.push_str("5. If no memories are relevant, respond with \"NONE\".\n");
    prompt.push_str("\nSelected memory keys: ");

    debug!("Sending prompt to broker LLM:\n{}", prompt);
    
    // Call the broker LLM to get filtered keys
    let broker_response = match broker_client.generate(&prompt).await {
        Ok(response) => response,
        Err(e) => {
            error!("Broker LLM error: {}", e);
            // Fall back to raw semantic search on broker failure
            warn!("Falling back to raw semantic search results due to broker error");
            return Ok(similar_memories
                .into_iter()
                .map(|(memory, score)| {
                    debug!("Fallback memory match (score={:.2}): {}", score, memory.value);
                    memory_to_memory_item(&memory)
                })
                .collect());
        }
    };
    
    debug!("Broker LLM response: {}", broker_response);
    
    // Parse the broker response to get the list of selected memory keys
    let selected_keys: HashSet<String> = if broker_response.trim().to_uppercase() == "NONE" {
        info!("Broker LLM determined no memories are relevant");
        HashSet::new()
    } else {
        broker_response
            .split(',')
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
            .collect()
    };
    
    // Filter memories based on broker selection
    let mut filtered_memories = Vec::new();
    
    if selected_keys.is_empty() {
        info!("No memories selected by broker or invalid response format");
    } else {
        info!("Broker selected {} memories", selected_keys.len());
        
        for key in &selected_keys {
            if let Some(memory) = memory_map.get(key) {
                debug!("Broker selected memory: {}", memory.value);
                filtered_memories.push(memory_to_memory_item(memory));
            } else {
                // This happens if the broker invents a key not in the original list
                warn!("Broker selected nonexistent memory key: {}", key);
            }
        }
    }
    
    // If broker selection yielded valid results, return those
    if !filtered_memories.is_empty() {
        info!("Returning {} broker-filtered memories", filtered_memories.len());
        return Ok(filtered_memories);
    }
    
    // Fallback to semantic search results if broker filtering produced no results
    info!("Broker filtering produced no results, falling back to semantic search");
    Ok(similar_memories
        .into_iter()
        .map(|(memory, score)| {
            debug!("Fallback memory match (score={:.2}): {}", score, memory.value);
            memory_to_memory_item(&memory)
        })
        .collect())
}

/// Format memories to enhance a prompt with context
/// This is a utility function that could be used directly by HAPPE
/// but is kept here for reference as part of the memory system
pub fn format_memories_for_prompt(memories: &[MemoryItem], max_token_estimate: usize) -> String {
    if memories.is_empty() {
        return String::new();
    }

    // Very simple token estimation (4 chars ~= 1 token)
    let estimate_tokens = |s: &str| -> usize { s.len() / 4 };

    let mut result = String::from("Previous relevant interactions:\n");
    let mut total_tokens = estimate_tokens(&result);

    // Add memories until we hit the token limit
    for (i, memory) in memories.iter().enumerate() {
        let formatted = format!("{}: {}\n", i + 1, memory.content);
        let tokens = estimate_tokens(&formatted);

        if total_tokens + tokens > max_token_estimate {
            // Add an indication that more memories were available but truncated
            if i < memories.len() - 1 {
                result
                    .push_str("(additional relevant context omitted due to length constraints)\n");
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
