use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::process::Command;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::env;
use std::error::Error;
use colored::*;
use reqwest::Client;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json;

use crate::logging::{log_debug, log_error};

// --- Constants --- //
pub const TOKEN_THRESHOLD: usize = 700000; // 700k tokens 
const CHARS_PER_TOKEN: usize = 4; // Estimate 4 chars per token as a heuristic

// --- Chat History Types --- //
pub type Role = String; // User-defined type for clarity

// Role constants
pub mod roles {
    pub const USER: &str = "user";
    pub const ASSISTANT: &str = "assistant";
    pub const SYSTEM: &str = "system";
}

// History structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: Role, // "user" or "assistant"
    pub content: String,
    pub timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ChatHistory {
    pub messages: Vec<ChatMessage>,
    pub session_id: String,
}

/// Generate a session ID unique to the current terminal session
pub fn generate_session_id() -> String {
    // Try using process information to identify the terminal session
    let mut session_components = Vec::new();
    
    // Try to get terminal session ID - we'll use the parent process ID
    // as a proxy for terminal session (terminal's PID)
    if let Ok(output) = Command::new("sh")
        .arg("-c")
        .arg("ps -o ppid= -p $$")
        .output()
    {
        if let Ok(ppid) = String::from_utf8(output.stdout) {
            let ppid = ppid.trim();
            if !ppid.is_empty() {
                session_components.push(format!("term_{}", ppid));
            }
        }
    }
    
    // Add the shell's PID as another component
    if let Ok(output) = Command::new("sh").arg("-c").arg("echo $$").output() {
        if let Ok(pid) = String::from_utf8(output.stdout) {
            let pid = pid.trim();
            if !pid.is_empty() {
                session_components.push(format!("sh_{}", pid));
            }
        }
    }
    
    // Join all components
    if session_components.is_empty() {
        // Fallback to just timestamp if we couldn't get process info
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        format!("session_{}", timestamp)
    } else {
        session_components.join("_")
    }
}

/// Get the path for a session's history file
pub fn get_history_file_path(config_dir: &Path, session_id: &str) -> PathBuf {
    // Sanitize session_id for filename
    let sanitized_session_id = session_id.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
    config_dir.join(format!("history_{}.json", sanitized_session_id))
}

/// Load chat history from JSON file
pub fn load_chat_history(config_dir: &Path, session_id: &str) -> ChatHistory {
    let history_file_path = get_history_file_path(config_dir, session_id);

    match fs::read_to_string(&history_file_path) {
        Ok(json_str) => {
            match serde_json::from_str::<ChatHistory>(&json_str) {
                Ok(mut history) => {
                    // Ensure the loaded history has the correct session ID
                    history.session_id = session_id.to_string();
                    if env::var("GEMINI_DEBUG").is_ok() {
                         log_debug(&format!("Loaded history from: {}", history_file_path.display()));
                    }
                    history
                },
                Err(e) => {
                    eprintln!("{}: {}. {} {}", "Warning: Failed to parse history file".yellow(), e, "Creating new history for session:".yellow(), session_id.cyan());
                    ChatHistory { messages: Vec::new(), session_id: session_id.to_string() }
                }
            }
        },
        Err(_) => { // File likely doesn't exist
             if env::var("GEMINI_DEBUG").is_ok() {
                 log_debug(&format!("No history file found at: {} Starting new history.", history_file_path.display()));
            }
            ChatHistory { messages: Vec::new(), session_id: session_id.to_string() }
        }
    }
}

/// Save chat history to JSON file
pub fn save_chat_history(config_dir: &Path, history: &ChatHistory) -> Result<(), Box<dyn Error>> {
    let history_file_path = get_history_file_path(config_dir, &history.session_id);
    let json_str = serde_json::to_string_pretty(history)?;
    fs::write(&history_file_path, json_str)?;
    if env::var("GEMINI_DEBUG").is_ok() {
         log_debug(&format!("Saved history to: {}", history_file_path.display()));
    }
    Ok(())
}

/// Start a new chat by removing existing history file
pub fn start_new_chat(config_dir: &Path, session_id: &str) {
    let history_file_path = get_history_file_path(config_dir, session_id);
    if history_file_path.exists() {
        if let Err(e) = fs::remove_file(&history_file_path) {
            eprintln!("{}: {}: {}", "Warning: Failed to delete existing history file".yellow(), history_file_path.display(), e);
        } else {
            println!("{} {}", "Started new chat (removed previous history file for session:".yellow(), session_id.cyan());
        }
    }
}

/// Function to get the last N commands from shell history
pub fn get_recent_commands(count: usize) -> Vec<String> {
    let mut commands = Vec::new();
    
    // Try to determine shell type
    let shell = env::var("SHELL").unwrap_or_else(|_| String::from("unknown"));
    
    // Preferred: try to use the fc command which works across shells
    if let Ok(output) = Command::new("sh")
        .arg("-c")
        .arg(format!("fc -ln -{}..1", count))
        .output()
    {
        if output.status.success() {
            if let Ok(history_output) = String::from_utf8(output.stdout) {
                // Split lines, trim, filter out empty lines, and collect
                commands = history_output
                    .lines()
                    .map(|line| line.trim())
                    .filter(|line| !line.is_empty())
                    .map(String::from)
                    .collect();

                // fc lists oldest first, so we reverse to get most recent first
                commands.reverse();

                // Remove the command that ran this tool itself if present, check different forms
                commands.retain(|cmd|
                    !cmd.starts_with("gemini ") &&
                    !cmd.starts_with("./gemini ") && // If run locally
                    !cmd.contains("fc -ln") && // Exclude the fc command itself
                    !cmd.contains("gemini-cli-bin") // Exclude the direct binary call
                );

                // Take only the required count after filtering
                commands.truncate(count);
                return commands;
            }
        } else {
            // Optionally log if fc failed, but proceed to file fallback
            if env::var("GEMINI_DEBUG").is_ok() {
                log_debug("Warning: `fc` command failed, falling back to history file.");
                if let Ok(stderr) = String::from_utf8(output.stderr) {
                    log_error(&stderr);
                }
            }
        }
    } else if env::var("GEMINI_DEBUG").is_ok() {
        log_debug("Warning: Could not execute `fc` command.");
    }

    // Fallback: Read history file if fc failed or wasn't available
    let history_path = if shell.contains("zsh") {
        env::var("HISTFILE").unwrap_or_else(|_|
            format!("{}/.zsh_history", env::var("HOME").unwrap_or_else(|_| String::from("~")))
        )
    } else {
        env::var("HISTFILE").unwrap_or_else(|_|
            format!("{}/.bash_history", env::var("HOME").unwrap_or_else(|_| String::from("~")))
        )
    };

    if let Ok(file) = File::open(&history_path) {
        let reader = BufReader::new(file);
        let is_zsh = shell.contains("zsh");

        let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

        for line in lines.iter().rev() {
            if commands.len() >= count {
                break; // Stop once we have enough commands
            }
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() {
                continue;
            }

            let cmd = if is_zsh {
                // Zsh extended history format: ': <timestamp>:<duration>;<command>'
                // Basic format: '<command>'
                // We just need the command part after the ';', if it exists
                trimmed_line.split_once(';').map_or(trimmed_line, |(_meta, command)| command).trim()
            } else {
                // Bash history is simpler
                trimmed_line
            };

            // Exclude the gemini command itself
            if !cmd.starts_with("gemini ") &&
               !cmd.starts_with("./gemini ") &&
               !cmd.contains("gemini-cli-bin")
            {
                commands.push(cmd.to_string());
            }
        }
    } else if env::var("GEMINI_DEBUG").is_ok() {
        log_debug(&format!("Warning: Could not open history file: {} .", history_path));
    }

    // Return collected commands (already reversed by reading file backwards)
    commands
}

/// Create system prompt with recent command history context
pub fn create_system_prompt_with_history(base_prompt: &str) -> String {
    let recent_commands = get_recent_commands(5);
    
    if recent_commands.is_empty() {
        return base_prompt.to_string();
    }
    
    let mut prompt = base_prompt.to_string();
    prompt.push_str("\n\nRecent command history:\n");
    
    for (i, cmd) in recent_commands.iter().enumerate() {
        prompt.push_str(&format!("{}. {}\n", i + 1, cmd));
    }
    
    prompt.push_str("\nPlease consider this command history when providing assistance.");
    prompt
}

/// Estimate the number of tokens in a string
/// This is a simple heuristic based on character count
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / CHARS_PER_TOKEN + 1 // +1 to avoid div by zero and round up
}

/// Estimate total tokens in a chat history
pub fn estimate_total_tokens(history: &ChatHistory, system_prompt: &str) -> usize {
    let mut total = estimate_tokens(system_prompt);
    
    for message in &history.messages {
        total += estimate_tokens(&message.content);
        // Add a small overhead for message formatting
        total += 4; // "role" field tokens
    }
    
    total
}

/// Request Gemini to summarize the conversation history
pub async fn summarize_conversation(
    client: &Client, 
    api_key: &str, 
    history: &ChatHistory
) -> Result<ChatHistory, Box<dyn Error>> {
    // --- Start Spinner ---
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&[
                "[    ]",
                "[=   ]",
                "[==  ]",
                "[=== ]",
                "[ ===]",
                "[  ==]",
                "[   =]",
                "[    ]",
                "[   =]",
                "[  ==]",
                "[ ===]",
                "[====]",
                "[=== ]",
                "[==  ]",
                "[=   ]",
            ])
            .template("{spinner:.blue} {msg}")? // Added color here
    );
    pb.set_message("Summarizing conversation with Gemini...".cyan().to_string());

    // Create a request directly using JSON structure
    // Rather than depending on model.rs, which would create a circular dependency
    let mut request_json = serde_json::json!({
        "contents": [],
        "system_instruction": {
            "parts": [{
                "text": "You are a helpful assistant tasked with summarizing a conversation. Create a concise summary that preserves the key points and context while reducing the token count significantly."
            }]
        }
    });
    
    // Add history to contents
    let mut contents = Vec::new();
    for msg in &history.messages {
        contents.push(serde_json::json!({
            "parts": [{"text": msg.content}],
            "role": msg.role
        }));
    }
    
    // Add the summarization request
    contents.push(serde_json::json!({
        "parts": [{"text": "Please summarize our conversation so far in a concise yet informative way. Keep the most important context and details. Format the summary as a conversation with clear USER and ASSISTANT roles. I'll use this summary to continue our conversation while reducing token usage."}],
        "role": "user"
    }));
    
    // Set the contents in the request
    request_json["contents"] = serde_json::json!(contents);
    
    // Prepare API URL
    let model = "gemini-2.5-pro-exp-03-25";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model,
        api_key
    );
    
    // Make the request
    let result = client
        .post(&url)
        .json(&request_json)
        .send()
        .await;

    // --- Finish Spinner ---
    pb.finish_and_clear(); // Clear the spinner

    let res = result?; // Handle potential request error after spinner finishes

    if !res.status().is_success() {
        let status = res.status();
        let error_text = res.text().await
            .unwrap_or_else(|_| "Could not read error body".to_string());
        return Err(format!("{} {}: {}", "Summarization API request failed:".red(), status, error_text).into());
    }
    
    let response_data: serde_json::Value = res.json().await?;
    let response_text = if let Some(text) = response_data["candidates"][0]["content"]["parts"][0]["text"].as_str() {
        text.to_owned()
    } else {
        return Err("No content found in Gemini response".into());
    };
    
    // Create a new ChatHistory with the summarized content
    let mut new_history = ChatHistory {
        messages: Vec::new(),
        session_id: history.session_id.clone(),
    };
    
    // Add a system note about summarization
    new_history.messages.push(ChatMessage {
        role: roles::SYSTEM.to_string(),
        content: "Note: The following is a summarized version of a longer conversation.".to_string(),
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    
    // Add the summary as a single message
    new_history.messages.push(ChatMessage {
        role: roles::ASSISTANT.to_string(),
        content: response_text,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    });
    
    Ok(new_history)
}

/// Log history debug information if GEMINI_DEBUG is set
pub fn log_history_debug(history: &ChatHistory, token_count: usize, was_summarized: bool, session_id: &str) {
    if !std::env::var("GEMINI_DEBUG").is_ok() {
        return;
    }

    println!("{}", "\n--- DEBUG INFO START ---".purple());
    
    println!("{} {}", "\nUsing session ID:".cyan(), session_id.yellow());
    
    if !history.messages.is_empty() {
        println!("{}", format!("\nConversation history (estimated tokens: {}):", token_count).cyan());
        if was_summarized {
            println!("{}", "(History was automatically summarized)".yellow());
        }
        println!("{}", "--------------------".purple());
        for (i, msg) in history.messages.iter().enumerate() {
            let role_colored = match msg.role.as_str() {
                "user" => msg.role.blue(),
                "assistant" => msg.role.green(),
                "system" => msg.role.yellow(),
                _ => msg.role.dimmed(),
            };
            println!("{}. [{}]: {}", i + 1, role_colored, msg.content);
        }
        println!("{}", "--------------------".purple());
    }
    println!("{}", "\n--- DEBUG INFO END ---".purple());
} 