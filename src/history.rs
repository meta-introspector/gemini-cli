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
    // First, try to get session ID from environment variable
    // This allows for explicit session control and persistence across commands
    if let Ok(session_id) = env::var("GEMINI_SESSION_ID") {
        if !session_id.is_empty() {
            return session_id;
        }
    }
    
    // Next, try to get a consistent terminal identifier
    // Use TERM_SESSION_ID on macOS/iTerm
    if let Ok(term_session) = env::var("TERM_SESSION_ID") {
        if !term_session.is_empty() {
            return format!("term_{}", term_session);
        }
    }
    
    // For Linux/other terminals, try using TTY
    if let Ok(output) = Command::new("sh")
        .arg("-c")
        .arg("tty")
        .output()
    {
        if let Ok(tty) = String::from_utf8(output.stdout) {
            let tty = tty.trim();
            if !tty.is_empty() && tty != "not a tty" {
                // Normalize the TTY path to make it a valid filename component
                let normalized = tty.replace("/", "_").replace(" ", "_");
                return format!("tty_{}", normalized);
            }
        }
    }
    
    // Try using shell info as fallback
    let mut shell_info = String::new();
    
    // Try to get the shell's parent terminal PID
    if let Ok(output) = Command::new("sh")
        .arg("-c")
        .arg("ps -o ppid= -p $$")
        .output()
    {
        if let Ok(ppid) = String::from_utf8(output.stdout) {
            let ppid = ppid.trim();
            if !ppid.is_empty() {
                shell_info.push_str(&format!("term_{}", ppid));
            }
        }
    }
    
    // Add shell environment variables that should be consistent in a terminal session
    if let Ok(shlvl) = env::var("SHLVL") {
        if !shell_info.is_empty() {
            shell_info.push('_');
        }
        shell_info.push_str(&format!("lvl_{}", shlvl));
    }
    
    // Use the USER environment variable for additional stability
    if let Ok(user) = env::var("USER") {
        if !shell_info.is_empty() {
            shell_info.push('_');
        }
        shell_info.push_str(&format!("usr_{}", user));
    }
    
    // If we've gathered some shell info, use it
    if !shell_info.is_empty() {
        return shell_info;
    }
    
    // Ultimate fallback: use a stable identifier based on the day
    // This at least keeps conversations together within the same day
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    
    // Convert to day-based timestamp (seconds / (seconds per day))
    // 86400 = 60 * 60 * 24 = seconds in a day
    let day_timestamp = now / 86400;
    format!("day_{}", day_timestamp)
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
    let shell = match env::var("SHELL") {
        Ok(s) => s,
        Err(_) => {
            if env::var("GEMINI_DEBUG").is_ok() {
                log_debug("Could not determine shell type from SHELL env var");
            }
            String::from("unknown")
        }
    };
    
    let is_zsh = shell.contains("zsh");
    let is_bash = shell.contains("bash");
    
    if env::var("GEMINI_DEBUG").is_ok() {
        log_debug(&format!("Detected shell type: {}", shell));
    }
    
    // Read directly from history file - this provides access to the actual commands
    // rather than just command names from fc/history
    if env::var("GEMINI_DEBUG").is_ok() {
        log_debug("Reading command history directly from history file");
    }
    
    // Determine the history file path based on shell
    let history_path = if is_zsh {
        env::var("HISTFILE").unwrap_or_else(|_|
            format!("{}/.zsh_history", env::var("HOME").unwrap_or_else(|_| String::from("~")))
        )
    } else {
        env::var("HISTFILE").unwrap_or_else(|_|
            format!("{}/.bash_history", env::var("HOME").unwrap_or_else(|_| String::from("~")))
        )
    };
    
    if env::var("GEMINI_DEBUG").is_ok() {
        log_debug(&format!("Trying to read history from: {}", history_path));
    }

    if let Ok(file) = File::open(&history_path) {
        let reader = BufReader::new(file);
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
                trimmed_line.split_once(';').map_or(trimmed_line, |(_meta, command)| command).trim()
            } else {
                // Bash history is simpler
                trimmed_line
            };

            commands.push(cmd.to_string());
        }
        
        if env::var("GEMINI_DEBUG").is_ok() {
            log_debug(&format!("Found {} commands from history file", commands.len()));
        }
    } else if env::var("GEMINI_DEBUG").is_ok() {
        log_debug(&format!("Could not open history file: {}", history_path));
    }

    // Filter out gemini commands and the command to run this tool
    commands.retain(|cmd|
        !cmd.starts_with("gemini ") &&
        !cmd.starts_with("./gemini ") &&
        !cmd.contains("fc -l") &&
        !cmd.contains("history") &&
        !cmd.contains("gemini-cli-bin")
    );

    // Truncate to the requested count
    if commands.len() > count {
        commands.truncate(count);
    }

    if env::var("GEMINI_DEBUG").is_ok() && commands.is_empty() {
        log_debug("No recent commands found to add to system prompt");
    }

    commands
}

/// Create system prompt with recent command history context
pub fn create_system_prompt_with_history(base_prompt: &str) -> String {
    let recent_commands = get_recent_commands(5);
    
    if recent_commands.is_empty() {
        if env::var("GEMINI_DEBUG").is_ok() {
            log_debug("No recent commands found to add to system prompt");
        }
        return base_prompt.to_string();
    }
    
    let mut prompt = base_prompt.to_string();
    prompt.push_str("\n\nRecent command history:\n");
    
    for (i, cmd) in recent_commands.iter().enumerate() {
        prompt.push_str(&format!("{}. {}\n", i + 1, cmd));
        if env::var("GEMINI_DEBUG").is_ok() {
            log_debug(&format!("Adding command to system prompt: {}", cmd));
        }
    }
    
    prompt.push_str("\nPlease consider this command history when providing assistance.");
    
    if env::var("GEMINI_DEBUG").is_ok() {
        log_debug(&format!("Added {} recent commands to system prompt", recent_commands.len()));
        log_debug("SYSTEM PROMPT WITH HISTORY:");
        log_debug(&prompt);
    }
    
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