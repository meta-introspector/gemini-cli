use crate::config::AppConfig;
use crate::ida_client::IdaClient;
use crate::llm_client;
use gemini_ipc::internal_messages::{ConversationTurn, MemoryItem};
use std::io::{self, Write};
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error, info, warn};

pub async fn run_coordinator(config: AppConfig) -> anyhow::Result<()> {
    info!("Coordinator starting...");

    // Create IDA client
    let mut ida_client = IdaClient::connect(&config.ida_socket_path).await?;
    info!("Connected to IDA daemon at {}", config.ida_socket_path);

    // Placeholder: Simple loop reading from stdin
    let mut stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = io::stdout();

    loop {
        print!("> ");
        stdout.flush()?;

        let mut line = String::new();
        match stdin.read_line(&mut line).await {
            Ok(0) => {
                info!("Input stream closed, exiting.");
                break; // EOF
            }
            Ok(_) => {
                let query = line.trim();
                if query.is_empty() {
                    continue;
                }
                if query == "exit" || query == "quit" {
                    break;
                }

                info!(query, "Received user query");

                // 1. Get memories from IDA
                let memories = match ida_client.get_memories(query).await {
                    Ok(m) => {
                        info!(count = m.len(), "Retrieved memories from IDA");
                        m
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to retrieve memories from IDA");
                        // Decide how to proceed - maybe continue without memories?
                        vec![]
                    }
                };

                // 2. Construct prompt (Placeholder)
                let prompt = construct_prompt(query, &memories);
                debug!(prompt, "Constructed prompt");

                // 3. Call LLM (Placeholder)
                let llm_response = match llm_client::generate_response(&prompt).await {
                    Ok(resp) => {
                        info!("Received response from LLM");
                        resp
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to get response from LLM");
                        // Provide error message to user
                        "Sorry, I encountered an error talking to the language model.".to_string()
                    }
                };

                // 4. Handle tool calls (Placeholder - none for now)

                // 5. Send response to user
                println!("LLM: {}", llm_response);

                // 6. Store conversation turn asynchronously
                let turn_data = ConversationTurn {
                    user_query: query.to_string(),
                    retrieved_memories: memories.clone(), // Clone memories for storage
                    llm_response: llm_response.clone(),
                    // Add other relevant fields like timestamps, context, etc.
                };
                if let Err(e) = ida_client.store_turn_async(turn_data).await {
                    warn!(error = %e, "Failed to send turn data to IDA for storage");
                    // Log the error, but don't block the user flow
                }
            }
            Err(e) => {
                error!(error = %e, "Error reading from stdin");
                break; // Exit on stdin error
            }
        }
    }

    info!("Coordinator finished.");
    Ok(())
}

// Placeholder prompt construction
fn construct_prompt(query: &str, memories: &[MemoryItem]) -> String {
    let mut prompt = String::new();
    if !memories.is_empty() {
        prompt.push_str("Relevant previous interactions:\n");
        for mem in memories {
            // Basic formatting, could be more sophisticated
            prompt.push_str(&format!("- {}\n", mem.content));
        }
        prompt.push_str("\n---\n\n");
    }
    prompt.push_str(&format!("User query: {}\n", query));
    prompt.push_str("Response:"); // Let the LLM complete
    prompt
}
