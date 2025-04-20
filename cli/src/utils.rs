// Utility functions for the CLI

use crate::McpProvider;
use crate::logging::log_error;
use crate::memory_broker::MemoryBroker;
use colored::*;
use serde_json::{Value, json};
use std::error::Error;
use std::io::{self, Write};
use tokio::time::{Duration, timeout};

// Represents the result of a tool execution
pub enum ToolExecutionResult {
    Success(Value),
    Failure(String),
    Timeout,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationLevel {
    None,     // No confirmation needed
    Standard, // Regular confirmation (y/n/a)
    Strict,   // Stricter confirmation (y/n only)
}

// Configuration for tool execution
pub struct ToolExecutionConfig {
    pub timeout_seconds: u64,
    pub confirmation_level: ConfirmationLevel,
    pub auto_approve_tools: Vec<String>, // List of tool names that don't need confirmation
}

impl Default for ToolExecutionConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            confirmation_level: ConfirmationLevel::Standard,
            auto_approve_tools: vec![
                "filesystem.read_file".to_string(),
                "filesystem.list_directory".to_string(),
            ],
        }
    }
}

/// Executes a tool with the appropriate confirmation logic and error handling
pub async fn execute_tool_with_confirmation(
    provider: &McpProvider<'_>,
    function_name: &str,
    arguments: Value,
    config: Option<ToolExecutionConfig>,
) -> ToolExecutionResult {
    // Use default config if none provided
    let config = config.unwrap_or_default();

    // Parse qualified name
    let qualified_name = function_name.replace(".", "/");
    let parts: Vec<&str> = qualified_name.splitn(2, "/").collect();

    if parts.len() != 2 {
        return ToolExecutionResult::Failure(format!(
            "Invalid function call name format: {}",
            function_name
        ));
    }

    let server_name = parts[0];
    let tool_name = parts[1];

    // Check if confirmation is needed
    let needs_confirmation = match config.confirmation_level {
        ConfirmationLevel::None => false,
        _ => !config
            .auto_approve_tools
            .contains(&function_name.to_string()),
    };

    // Get confirmation if needed
    let mut should_execute = true;
    if needs_confirmation {
        println!(
            "{} Requesting to call function {} on server {} with arguments:",
            "Confirmation:".yellow().bold(),
            tool_name.cyan(),
            server_name.cyan()
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&arguments).unwrap_or_default()
        );

        match config.confirmation_level {
            ConfirmationLevel::Standard => {
                print!(
                    "{} (y)es/(n)o/(a)lways allow this tool: ",
                    "Proceed?".yellow()
                );
            }
            ConfirmationLevel::Strict => {
                print!("{} (y)es/(n)o: ", "Proceed?".yellow());
            }
            _ => {}
        }

        io::stdout().flush().ok();
        let mut response = String::new();
        io::stdin().read_line(&mut response).ok();

        match response.trim().to_lowercase().as_str() {
            "y" | "yes" => {
                should_execute = true;
            }
            "a" | "always" if matches!(config.confirmation_level, ConfirmationLevel::Standard) => {
                should_execute = true;
                println!("{} Always allowing tool: {}", "Note:".blue(), function_name);
                // TODO: Add function_name to auto_approve_tools for this session
            }
            _ => {
                println!("{} Function call cancelled by user", "Cancelled:".red());
                return ToolExecutionResult::Cancelled;
            }
        }
    }

    if should_execute {
        println!(
            "{} Calling function {} on server {}",
            "Action:".blue().bold(),
            tool_name.cyan(),
            server_name.cyan()
        );

        // Match on the provider and handle timeout/execution within each arm
        match provider {
            McpProvider::Host(Some(host)) => {
                match timeout(
                    Duration::from_secs(config.timeout_seconds),
                    host.execute_tool(server_name, tool_name, arguments.clone()),
                )
                .await
                {
                    Ok(result) => match result {
                        Ok(result_value) => {
                            println!(
                                "{}: {}",
                                "Result".green(),
                                serde_json::to_string_pretty(&result_value)
                                    .unwrap_or_else(|_| result_value.to_string())
                            );
                            ToolExecutionResult::Success(result_value)
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Function call {} failed: {}", function_name, e);
                            log_error(&error_msg);
                            ToolExecutionResult::Failure(error_msg)
                        }
                    },
                    Err(_) => {
                        let timeout_msg = format!(
                            "Function call {} timed out after {} seconds",
                            function_name, config.timeout_seconds
                        );
                        log_error(&timeout_msg);
                        ToolExecutionResult::Timeout
                    }
                }
            }
            McpProvider::Client(client) => {
                match timeout(
                    Duration::from_secs(config.timeout_seconds),
                    client.execute_tool(server_name, tool_name, arguments.clone()),
                )
                .await
                {
                    Ok(result) => match result {
                        Ok(result_value) => {
                            println!(
                                "{}: {}",
                                "Result".green(),
                                serde_json::to_string_pretty(&result_value)
                                    .unwrap_or_else(|_| result_value.to_string())
                            );
                            ToolExecutionResult::Success(result_value)
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Function call {} failed: {}", function_name, e);
                            log_error(&error_msg);
                            ToolExecutionResult::Failure(error_msg)
                        }
                    },
                    Err(_) => {
                        let timeout_msg = format!(
                            "Function call {} timed out after {} seconds",
                            function_name, config.timeout_seconds
                        );
                        log_error(&timeout_msg);
                        ToolExecutionResult::Timeout
                    }
                }
            }
            McpProvider::Host(None) => {
                // If host is None, immediately return failure
                ToolExecutionResult::Failure("MCP host is not available".to_string())
            }
        }
    } else {
        ToolExecutionResult::Cancelled
    }
}

/// Converts tool execution result to a function response part
pub fn tool_result_to_function_response(
    function_name: &str,
    result: ToolExecutionResult,
) -> gemini_core::types::Part {
    use gemini_core::types::Part;

    match result {
        ToolExecutionResult::Success(value) => {
            Part::function_response(function_name.to_string(), value)
        }
        ToolExecutionResult::Failure(error) => {
            Part::function_response(function_name.to_string(), json!({ "error": error }))
        }
        ToolExecutionResult::Timeout => Part::function_response(
            function_name.to_string(),
            json!({ "error": "Tool execution timed out" }),
        ),
        ToolExecutionResult::Cancelled => Part::function_response(
            function_name.to_string(),
            json!({ "error": "Tool execution cancelled by user" }),
        ),
    }
}

/// Enhanced prompt using memory retrieval
pub async fn enhance_prompt(
    original_prompt: &str,
    memory_broker: &MemoryBroker,
    top_k: usize,
    min_relevance: f32,
) -> Result<String, Box<dyn Error>> {
    // For now, just return the original prompt
    // In the future, we can implement memory retrieval once the API is more stable
    Ok(original_prompt.to_string())
}
