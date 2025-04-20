use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum LlmClientError {
    #[error("Placeholder LLM error: {0}")]
    Placeholder(String),
    // Add real errors later (e.g., network, API errors)
}

type Result<T> = std::result::Result<T, LlmClientError>;

// Placeholder function
pub async fn generate_response(prompt: &str) -> Result<String> {
    info!(prompt_len = prompt.len(), "Simulating LLM call...");

    // Simulate some processing time
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    // Return a canned response
    Ok(format!(
        "Placeholder response to prompt ending with: ...{}",
        &prompt
            .chars()
            .rev()
            .take(50)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    ))
}
