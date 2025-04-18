// Utility functions for the CLI

use crate::logging::{log_error, log_warning};
use colored::*;
use gemini_mcp::McpHost;
use serde_json::{json, Value};
use std::io::{self, Write};
use tokio::time::{timeout, Duration};

// Represents the result of a tool execution
pub enum ToolExecutionResult {
    Success(Value),
    Failure(String),
    Timeout,
    Cancelled,
}

// Confirmation levels for tool execution
pub enum ConfirmationLevel {
    None,      // No confirmation needed
    Standard,  // Regular confirmation (y/n/a)
    Strict,    // Always require confirmation (y/n only, no "always" option)
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
    host: &McpHost,
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
        _ => !config.auto_approve_tools.contains(&function_name.to_string()),
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
        println!("{}", serde_json::to_string_pretty(&arguments).unwrap_or_default());
        
        match config.confirmation_level {
            ConfirmationLevel::Standard => {
                print!("{} (y)es/(n)o/(a)lways allow this tool: ", "Proceed?".yellow());
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
        
        // Execute with timeout
        match timeout(
            Duration::from_secs(config.timeout_seconds),
            host.execute_tool(server_name, tool_name, arguments.clone())
        ).await {
            Ok(result) => {
                match result {
                    Ok(result) => {
                        println!(
                            "{}: {}",
                            "Result".green(),
                            serde_json::to_string_pretty(&result)
                                .unwrap_or_else(|_| result.to_string())
                        );
                        ToolExecutionResult::Success(result)
                    }
                    Err(e) => {
                        let error_msg = format!("Function call {} failed: {}", function_name, e);
                        log_error(&error_msg);
                        ToolExecutionResult::Failure(error_msg)
                    }
                }
            }
            Err(_) => {
                let timeout_msg = format!(
                    "Function call {} timed out after {} seconds",
                    function_name, config.timeout_seconds
                );
                log_error(&timeout_msg);
                ToolExecutionResult::Timeout
            }
        }
    } else {
        ToolExecutionResult::Cancelled
    }
}

/// Converts tool execution result to a function response part
pub fn tool_result_to_function_response(
    function_name: &str, 
    result: ToolExecutionResult
) -> gemini_core::types::Part {
    use gemini_core::types::Part;
    
    match result {
        ToolExecutionResult::Success(value) => {
            Part::function_response(function_name.to_string(), value)
        }
        ToolExecutionResult::Failure(error) => {
            Part::function_response(
                function_name.to_string(),
                json!({ "error": error })
            )
        }
        ToolExecutionResult::Timeout => {
            Part::function_response(
                function_name.to_string(),
                json!({ "error": "Tool execution timed out" })
            )
        }
        ToolExecutionResult::Cancelled => {
            Part::function_response(
                function_name.to_string(),
                json!({ "error": "Tool execution cancelled by user" })
            )
        }
    }
}
