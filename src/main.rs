use clap::Parser;
use dotenv::dotenv;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::env;
use std::error::Error;
use confy;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use pulldown_cmark::{Parser as MdParser, Event as MdEvent, Tag, Options, HeadingLevel, CodeBlockKind};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};
use dialoguer::{Confirm, theme::ColorfulTheme};

/// Simple CLI to interact with Google Gemini models
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The prompt to send to the Gemini model (default positional argument)
    #[arg(index = 1)] // Positional argument
    prompt: Option<String>,

    /// Prepend prompt with "Provide the Linux command for: "
    #[arg(short, long, default_value_t = false)]
    command_help: bool,

    /// Set and save the Gemini API Key persistently
    #[arg(long)]
    set_api_key: Option<String>,

    /// Set and save the system prompt persistently
    #[arg(long)]
    set_system_prompt: Option<String>,

    /// Show the current configuration
    #[arg(long, default_value_t = false)]
    show_config: bool,

    /// Enable memory-based conversation history (default)
    #[arg(long, default_value_t = false)]
    enable_history: bool,

    /// Disable conversation history
    #[arg(long, default_value_t = false)]
    disable_history: bool,

    /// Start a new conversation (don't use previous history)
    #[arg(long, default_value_t = false)]
    new_chat: bool,
}

// --- Chat History Types --- //

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ChatMessage {
    role: String, // "user" or "assistant"
    content: String,
    timestamp: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct ChatHistory {
    messages: Vec<ChatMessage>,
    session_id: String,
}

// --- Configuration --- //

#[derive(Debug, Serialize, Deserialize)]
struct AppConfig {
    api_key: Option<String>,
    system_prompt: Option<String>,
    // Add an option to disable conversation history
    save_history: Option<bool>,
}

/// Implement default values
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            system_prompt: Some(
                "You are a helpful command-line assistant for Linux. \
                You have access to the last few commands the user has run in their terminal. \
                Use this context to provide more relevant answers. When asked about commands, \
                provide concise and practical solutions focused on the user's needs."
                .to_string()
            ),
            save_history: Some(true), // Default to saving history
        }
    }
}

/// Generate a session ID unique to the current terminal session
fn generate_session_id() -> String {
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

/// Get the path for the configuration directory
fn get_config_dir() -> Result<PathBuf, Box<dyn Error>> {
    confy::get_configuration_file_path("gemini-cli", Some("config.toml"))?
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "Could not determine config directory".into())
}

/// Get the path for a session's history file
fn get_history_file_path(config_dir: &PathBuf, session_id: &str) -> PathBuf {
    // Sanitize session_id for filename
    let sanitized_session_id = session_id.replace(|c: char| !c.is_alphanumeric() && c != '-' && c != '_', "_");
    config_dir.join(format!("history_{}.json", sanitized_session_id))
}

/// Load chat history from JSON file
fn load_chat_history(config_dir: &PathBuf, session_id: &str) -> ChatHistory {
    let history_file_path = get_history_file_path(config_dir, session_id);

    match fs::read_to_string(&history_file_path) {
        Ok(json_str) => {
            match serde_json::from_str::<ChatHistory>(&json_str) {
                Ok(mut history) => {
                    // Ensure the loaded history has the correct session ID
                    history.session_id = session_id.to_string();
                    if env::var("GEMINI_DEBUG").is_ok() {
                         eprintln!("{} {}", "Loaded history from:".cyan(), history_file_path.display());
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
                 eprintln!("{} {} {}", "No history file found at:".cyan(), history_file_path.display(), "Starting new history.".cyan());
            }
            ChatHistory { messages: Vec::new(), session_id: session_id.to_string() }
        }
    }
}

/// Save chat history to JSON file
fn save_chat_history(config_dir: &PathBuf, history: &ChatHistory) -> Result<(), Box<dyn Error>> {
    let history_file_path = get_history_file_path(config_dir, &history.session_id);
    let json_str = serde_json::to_string_pretty(history)?;
    fs::write(&history_file_path, json_str)?;
    if env::var("GEMINI_DEBUG").is_ok() {
         eprintln!("{} {}", "Saved history to:".cyan(), history_file_path.display());
    }
    Ok(())
}

/// Function to get the last N commands from shell history
fn get_recent_commands(count: usize) -> Vec<String> {
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
                eprintln!("{}", "Warning: `fc` command failed, falling back to history file.".yellow());
                if let Ok(stderr) = String::from_utf8(output.stderr) {
                    eprintln!("{}", stderr.red());
                }
            }
        }
    } else if env::var("GEMINI_DEBUG").is_ok() {
        eprintln!("{}", "Warning: Could not execute `fc` command.".yellow());
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
        eprintln!("{} {} {}", "Warning: Could not open history file:".yellow(), history_path.cyan(), ".".yellow());
    }

    // Return collected commands (already reversed by reading file backwards)
    commands
}

/// Create system prompt with recent command history context
fn create_system_prompt_with_history(base_prompt: &str) -> String {
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

// --- Structs for Gemini API Request/Response --- //

#[derive(Serialize)]
struct GenerateContentRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Content>,
}

#[derive(Serialize, Clone)]
struct Content {
    parts: Vec<Part>,
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
}

#[derive(Serialize, Clone)]
struct Part {
    text: String,
}

#[derive(Deserialize, Debug, Serialize)]
struct GenerateContentResponse {
    candidates: Vec<Candidate>,
}

#[derive(Deserialize, Debug, Serialize)]
struct Candidate {
    content: ContentResponse,
}

#[derive(Deserialize, Debug, Serialize)]
struct ContentResponse {
    parts: Vec<PartResponse>,
    #[allow(dead_code)] // We deserialize this but don't use it currently
    role: String,
}

#[derive(Deserialize, Debug, Serialize)]
struct PartResponse {
    text: String,
}

// Constant for token summarization threshold
const TOKEN_THRESHOLD: usize = 700000; // 700k tokens 
const CHARS_PER_TOKEN: usize = 4; // Estimate 4 chars per token as a heuristic

/// Estimate the number of tokens in a string
/// This is a simple heuristic based on character count
fn estimate_tokens(text: &str) -> usize {
    text.chars().count() / CHARS_PER_TOKEN + 1 // +1 to avoid div by zero and round up
}

/// Estimate total tokens in a chat history
fn estimate_total_tokens(history: &ChatHistory, system_prompt: &str) -> usize {
    let mut total = estimate_tokens(system_prompt);
    
    for message in &history.messages {
        total += estimate_tokens(&message.content);
        // Add a small overhead for message formatting
        total += 4; // "role" field tokens
    }
    
    total
}

/// Request Gemini to summarize the conversation history
async fn summarize_conversation(
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

    // Create a system instruction that explains what we need
    let system_instruction = Content {
        parts: vec![Part { text: "You are a helpful assistant tasked with summarizing a conversation. Create a concise summary that preserves the key points and context while reducing the token count significantly.".to_string() }],
        role: None,
    };
    
    // Prepare the content - all the messages
    let mut contents = Vec::new();
    
    // First, add all history
    for msg in &history.messages {
        contents.push(Content {
            parts: vec![Part { text: msg.content.clone() }],
            role: Some(msg.role.clone()),
        });
    }
    
    // Add the explicit summarization request
    contents.push(Content {
        parts: vec![Part { text: "Please summarize our conversation so far in a concise yet informative way. Keep the most important context and details. Format the summary as a conversation with clear USER and ASSISTANT roles. I'll use this summary to continue our conversation while reducing token usage.".to_string() }],
        role: Some("user".to_string()),
    });
    
    let request_body = GenerateContentRequest {
        contents,
        system_instruction: Some(system_instruction),
    };
    
    // Prepare API URL
    let model = "gemini-1.5-flash-latest";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model,
        api_key
    );
    
    // Make the request
    let result = client
        .post(&url)
        .json(&request_body)
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
    
    let response_data: GenerateContentResponse = res.json().await?;
    if let Some(candidate) = response_data.candidates.first() {
        if let Some(part) = candidate.content.parts.first() {
            // Parse the summarized conversation 
            let summary_text = part.text.clone();
            
            // Create a new ChatHistory with the summarized content
            let mut new_history = ChatHistory {
                messages: Vec::new(),
                session_id: history.session_id.clone(),
            };
            
            // Add a system note about summarization
            new_history.messages.push(ChatMessage {
                role: "system".to_string(),
                content: "Note: The following is a summarized version of a longer conversation.".to_string(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
            
            // Add the summary as a single message
            new_history.messages.push(ChatMessage {
                role: "assistant".to_string(),
                content: summary_text,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            });
            
            return Ok(new_history);
        }
    }
    
    Err("Failed to get a usable summary from the API".red().into())
}

/// Render markdown in the terminal
fn render_markdown(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);
    
    let parser = MdParser::new_ext(markdown, options);
    
    // Initialize syntax highlighting
    let syntax_set = SyntaxSet::load_defaults_newlines();
    let theme_set = ThemeSet::load_defaults();
    let theme = &theme_set.themes["base16-ocean.dark"];
    
    let mut in_code_block = false;
    let mut code_block_lang = String::new();
    let mut code_block_content = String::new();
    let mut output = String::new();
    
    // Table state tracking
    let mut in_table = false;
    let mut in_table_cell = false;
    let mut current_row: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    
    for event in parser {
        match event {
            // Table handling
            MdEvent::Start(Tag::Table(_)) => {
                in_table = true;
                table_rows.clear();
                output.push_str("\n");
            },
            MdEvent::End(Tag::Table(_)) => {
                if !table_rows.is_empty() {
                    // Calculate column widths
                    let col_count = table_rows.iter().map(|row| row.len()).max().unwrap_or(0);
                    let mut col_widths = vec![0; col_count];
                    
                    for row in &table_rows {
                        for (i, cell) in row.iter().enumerate() {
                            if i < col_widths.len() {
                                col_widths[i] = col_widths[i].max(cell.len());
                            }
                        }
                    }
                    
                    // Render table with proper spacing
                    for (i, row) in table_rows.iter().enumerate() {
                        // Print cells with proper padding
                        for (j, cell) in row.iter().enumerate() {
                            if j < col_widths.len() {
                                let padding = col_widths[j].saturating_sub(cell.len());
                                let formatted_cell = if i == 0 {
                                    // Header cell
                                    format!("{}{} ", cell.bold(), " ".repeat(padding))
                                } else {
                                    format!("{}{} ", cell, " ".repeat(padding))
                                };
                                output.push_str(&formatted_cell);
                            }
                        }
                        output.push_str("\n");
                        
                        // Add separator line after header
                        if i == 0 {
                            for (j, width) in col_widths.iter().enumerate() {
                                output.push_str(&"─".repeat(*width).dimmed().to_string());
                                if j < col_widths.len() - 1 {
                                    output.push_str(" ");
                                }
                            }
                            output.push_str("\n");
                        }
                    }
                    output.push_str("\n");
                }
                in_table = false;
            },
            MdEvent::Start(Tag::TableHead) => {
                // Just track that we're in a table header
            },
            MdEvent::End(Tag::TableHead) => {
                // No action needed
            },
            MdEvent::Start(Tag::TableRow) => {
                current_row.clear();
            },
            MdEvent::End(Tag::TableRow) => {
                if !current_row.is_empty() {
                    table_rows.push(current_row.clone());
                }
            },
            MdEvent::Start(Tag::TableCell) => {
                in_table_cell = true;
                current_row.push(String::new());
            },
            MdEvent::End(Tag::TableCell) => {
                in_table_cell = false;
            },
            // Regular markdown elements
            MdEvent::Start(Tag::Heading(level, ..)) => {
                match level {
                    HeadingLevel::H1 => output.push_str(&format!("\n{} ", "##".bright_cyan().bold())),
                    HeadingLevel::H2 => output.push_str(&format!("\n{} ", "#".bright_cyan().bold())),
                    _ => output.push_str("\n"),
                }
            },
            MdEvent::End(Tag::Heading(..)) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::Paragraph) => {
                if !in_table && !output.is_empty() && !output.ends_with("\n\n") && !output.ends_with('\n') {
                    output.push_str("\n\n");
                }
            },
            MdEvent::End(Tag::Paragraph) => {
                if !in_table {
                    output.push_str("\n");
                }
            },
            MdEvent::Start(Tag::BlockQuote) => {
                output.push_str("\n");
            },
            MdEvent::End(Tag::BlockQuote) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::CodeBlock(info)) => {
                in_code_block = true;
                // Extract the language from the code block info
                match info {
                    CodeBlockKind::Fenced(lang) => code_block_lang = lang.to_string(),
                    _ => code_block_lang = String::new(),
                }
                code_block_content.clear();
                output.push_str("\n");
            },
            MdEvent::End(Tag::CodeBlock(_)) => {
                // Apply syntax highlighting
                let syntax = if code_block_lang.is_empty() {
                    syntax_set.find_syntax_plain_text()
                } else {
                    syntax_set.find_syntax_by_token(&code_block_lang)
                        .unwrap_or_else(|| syntax_set.find_syntax_plain_text())
                };
                
                let mut highlighter = HighlightLines::new(syntax, theme);
                
                // Add a separator line
                output.push_str(&format!("{}:\n", code_block_lang.cyan()));
                output.push_str(&"─".repeat(40).dimmed().to_string());
                output.push_str("\n");
                
                for line in LinesWithEndings::from(&code_block_content) {
                    let highlighted = highlighter.highlight_line(line, &syntax_set).unwrap_or_default();
                    let escaped = as_24_bit_terminal_escaped(&highlighted, false);
                    output.push_str(&escaped);
                }
                
                // Add a separator line
                output.push_str(&"─".repeat(40).dimmed().to_string());
                output.push_str("\n\n");
                
                in_code_block = false;
            },
            MdEvent::Start(Tag::List(_)) => {
                output.push_str("\n");
            },
            MdEvent::End(Tag::List(_)) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::Item) => {
                output.push_str(&format!("{}  ", "•".yellow()));
            },
            MdEvent::End(Tag::Item) => {
                output.push_str("\n");
            },
            MdEvent::Start(Tag::Emphasis) => {
                if !in_table_cell {
                    // No special handling needed for table cells
                }
            },
            MdEvent::End(Tag::Emphasis) => {
                // No special handling needed
            },
            MdEvent::Start(Tag::Strong) => {
                if !in_table_cell {
                    // No special handling needed
                }
            },
            MdEvent::End(Tag::Strong) => {
                // No special handling needed
            },
            MdEvent::Code(ref code) => {
                if in_table_cell && !current_row.is_empty() {
                    let idx = current_row.len() - 1;
                    current_row[idx].push_str(&format!("`{}`", code));
                } else {
                    output.push_str(&format!("`{}`", code.on_bright_black().white()));
                }
            },
            MdEvent::Text(ref text) => {
                if in_code_block {
                    code_block_content.push_str(text);
                } else if in_table_cell && !current_row.is_empty() {
                    let idx = current_row.len() - 1;
                    current_row[idx].push_str(text);
                } else {
                    output.push_str(text);
                }
            },
            MdEvent::Html(ref html) => {
                // Just pass through HTML
                if !in_table_cell {
                    output.push_str(html);
                }
            },
            MdEvent::SoftBreak => {
                if !in_table_cell {
                    output.push(' ');
                }
            },
            MdEvent::HardBreak => {
                if !in_table_cell {
                    output.push('\n');
                }
            },
            _ => {
                // Handle other cases as needed
            }
        }
    }
    
    output
}

// --- Main Application Logic --- //

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // Force color output even if not detecting a TTY (due to wrapper script)
    colored::control::set_override(true);

    dotenv().ok();

    // --- Get Configuration Directory --- //
    let config_dir = get_config_dir()?;
    // Ensure config directory exists
    fs::create_dir_all(&config_dir)?;
    let config_file_path = config_dir.join("config.toml");

    // --- Load or create configuration --- //
    let mut cfg: AppConfig = confy::load_path(&config_file_path).unwrap_or_default();

    // Parse command-line arguments
    let args = Args::parse();

    // Handle enable/disable history flag before other config checks
    let mut history_config_changed = false;
    if args.enable_history {
        cfg.save_history = Some(true);
        println!("{}", "Conversation history enabled.".green());
        history_config_changed = true;
    } else if args.disable_history {
        cfg.save_history = Some(false);
        println!("{}", "Conversation history disabled.".yellow());
        history_config_changed = true;
    }

    if history_config_changed {
        confy::store_path(&config_file_path, &cfg)?;
        // Optionally exit here if desired, or let it continue to potentially handle other flags/prompt
        // return Ok(());
    }

    // --- Handle Configuration Flags --- //
    let mut config_updated = false;
    if let Some(key) = args.set_api_key {
        cfg.api_key = Some(key);
        config_updated = true;
        println!("{}", "API Key updated.".green());
    }

    if let Some(prompt) = args.set_system_prompt {
        cfg.system_prompt = Some(prompt);
        config_updated = true;
        println!("{}", "System prompt updated.".green());
    }

    if config_updated {
        confy::store_path(&config_file_path, &cfg)?;
        println!("{} {}", "Configuration saved to:".cyan(), config_file_path.display());
        return Ok(()); // Exit after saving config
    }

    if args.show_config {
        println!("{} ({})", "Current Configuration".cyan().bold(), config_file_path.display());
        println!("  {}: {}", "API Key".blue(), cfg.api_key.as_deref().map_or("Not Set".yellow().to_string(), |k| if k.len() > 8 { format!("{}...", &k[..8]).bright_black().to_string() } else { "Set".green().to_string() }));
        println!("  {}: {}", "System Prompt".blue(), cfg.system_prompt.as_deref().map_or("Default".yellow().to_string(), |p| if p.len() > 50 { format!("{}...", &p[..50]).italic().to_string() } else { p.italic().to_string() }));
        println!("  {}: {}", "Save History".blue(), if cfg.save_history.unwrap_or(true) { "Enabled".green() } else { "Disabled".yellow() });
        return Ok(()); // Exit after showing config
    }

    // --- Proceed with API Call if Prompt is Provided --- //
    if let Some(user_prompt) = args.prompt {
        // Determine the API key: config > environment variable
        let api_key = cfg.api_key.clone().or_else(|| env::var("GEMINI_API_KEY").ok())
            .ok_or_else(|| -> Box<dyn Error> {
                Box::from(format!("{}", "Error: GEMINI_API_KEY not set. Set it via --set-api-key flag or environment variable.".red()))
            })?;

        // Get the base system prompt from config or use default
        let base_system_prompt = cfg.system_prompt.clone().unwrap_or_else(|| {
            AppConfig::default().system_prompt.unwrap_or_default() // Use the default from AppConfig impl
        });
        
        // Create a system prompt with command history
        let system_prompt_with_history = create_system_prompt_with_history(&base_system_prompt);

        // Determine if we should save history
        let should_save_history = cfg.save_history.unwrap_or(true);
        
        // Try to find an existing session ID in environment first, otherwise generate new
        // We still use an env var for the session ID itself to persist across calls within one shell session
        let session_id = env::var("GEMINI_SESSION_ID").unwrap_or_else(|_| generate_session_id());
        
        // Handle --new-chat: Delete existing history file for this session
        if args.new_chat {
            let history_file_path = get_history_file_path(&config_dir, &session_id);
            if history_file_path.exists() {
                 if let Err(e) = fs::remove_file(&history_file_path) {
                     eprintln!("{}: {}: {}", "Warning: Failed to delete existing history file".yellow(), history_file_path.display(), e);
                } else {
                     println!("{} {}", "Started new chat (removed previous history file for session:".yellow(), session_id.cyan());
                }
            }
        }

        // Load existing chat history for this session from file
        let mut chat_history = if should_save_history {
            load_chat_history(&config_dir, &session_id)
        } else {
            // If history is disabled, start with an empty one regardless of files
            ChatHistory { messages: Vec::new(), session_id: session_id.clone() }
        };

        // Prepare the final user prompt (handle -c flag)
        let final_user_prompt = if args.command_help {
            // Special prompt for command help to get only the command
            format!(
                "Provide ONLY the Linux command line to accomplish the following task, without any explanation, code blocks, or markdown formatting. Just the raw command itself: {}",
                user_prompt
            )
        } else {
            user_prompt
        };

        let client = Client::new();
        
        // Check if we need to summarize the conversation due to token count
        let estimated_tokens = estimate_total_tokens(&chat_history, &system_prompt_with_history);
        let mut history_was_summarized = false;
        
        // If token count exceeds threshold, summarize before sending
        if should_save_history && estimated_tokens > TOKEN_THRESHOLD && !chat_history.messages.is_empty() {
            println!("{}", format!("Conversation history is large (est. {} tokens). Summarizing...", estimated_tokens).yellow());
            
            match summarize_conversation(&client, &api_key, &chat_history).await {
                Ok(summarized_history) => {
                    chat_history = summarized_history;
                    history_was_summarized = true;
                    
                    // Re-calculate token count after summarization
                    let new_token_count = estimate_total_tokens(&chat_history, &system_prompt_with_history);
                    println!("{}", format!("Conversation summarized (new est. token count: {})", new_token_count).green());
                },
                Err(e) => {
                    eprintln!("{}: {}", "Warning: Failed to summarize conversation".yellow(), e);
                    eprintln!("{}", "Proceeding with full history".yellow());
                }
            }
        }
        
        // For debugging
        if std::env::var("GEMINI_DEBUG").is_ok() {
            println!("{}", "\n--- DEBUG INFO START ---".purple());
            println!("{}", "\nSystem prompt:".cyan());
            println!("{}", "--------------------".purple());
            println!("{}", system_prompt_with_history);
            println!("{}", "--------------------".purple());
            
            println!("{} {}", "\nUsing session ID:".cyan(), session_id.yellow());
            
            if !chat_history.messages.is_empty() {
                println!("{}", format!("\nConversation history (estimated tokens: {}):", estimated_tokens).cyan());
                if history_was_summarized {
                    println!("{}", "(History was automatically summarized)".yellow());
                }
                println!("{}", "--------------------".purple());
                for (i, msg) in chat_history.messages.iter().enumerate() {
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

        // --- Start Spinner ---
        let pb = ProgressBar::new_spinner();
        pb.enable_steady_tick(Duration::from_millis(120));
        pb.set_style(
             ProgressStyle::default_spinner()
                 .tick_strings(&[
                    "🌑 ", "🌒 ", "🌓 ", "🌔 ", "🌕 ", "🌖 ", "🌗 ", "🌘 "
                 ])
                .template("{spinner:.yellow} {msg}")? // Use a different color for the main spinner
        );
        pb.set_message("Asking Gemini...".cyan().to_string());

        // Current timestamp for the new messages
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Add user message to history *before* the API call
        if should_save_history {
            chat_history.messages.push(ChatMessage {
                role: "user".to_string(),
                content: final_user_prompt.clone(),
                timestamp: now,
            });
        }

        let history_for_context = if should_save_history {
            // Pass the potentially updated history
            Some(&chat_history)
        } else {
            None
        };

        // Call the API
        let api_result = call_gemini_api(&client, &api_key, Some(&system_prompt_with_history), &final_user_prompt, history_for_context).await;

        // --- Finish Spinner ---
        pb.finish_and_clear(); // Clear the spinner regardless of success/failure

        match api_result {
            Ok(response_text) => {
                if args.command_help {
                    // Handle command suggestion
                    let potential_command = response_text.trim();
                    
                    if potential_command.is_empty() {
                        eprintln!("{}", "Gemini did not suggest a command.".yellow());
                    } else {
                        println!("{}: `{}`", "Suggested command".cyan(), potential_command.green());
                        
                        if Confirm::with_theme(&ColorfulTheme::default())
                            .with_prompt("Do you want to run this command?")
                            .default(true)
                            .interact()? 
                        {
                            println!("{}", "Executing command...".cyan());
                            let output = Command::new("sh")
                                .arg("-c")
                                .arg(potential_command)
                                .output()?; // Use output() to capture stdout/stderr

                            if output.status.success() {
                                println!("{}", "Command executed successfully:".green());
                                if !output.stdout.is_empty() {
                                    println!("{}", String::from_utf8_lossy(&output.stdout).blue());
                                }
                            } else {
                                eprintln!("{}", "Command failed:".red());
                                if !output.stderr.is_empty() {
                                     eprintln!("{}", String::from_utf8_lossy(&output.stderr).red());
                                }
                            }
                        } else {
                            println!("{}", "Command not executed.".yellow());
                        }
                    }

                } else {
                    // Render markdown in the response for normal chat
                    let rendered_response = render_markdown(&response_text);
                    
                    // Print the response with a colored prefix
                    println!("{}: {}", "Gemini".blue().bold(), rendered_response);
                }
                
                // Add assistant response to history and save to file
                if should_save_history {
                    chat_history.messages.push(ChatMessage {
                        role: "assistant".to_string(),
                        // Store the raw response even if it was a command
                        content: response_text.clone(), 
                        timestamp: now, // Use the same timestamp as the user message for pairing
                    });
                    
                    // Save the updated chat history to file
                    if let Err(e) = save_chat_history(&config_dir, &chat_history) {
                        eprintln!("{}: {}", "Warning: Failed to save chat history".yellow(), e);
                    }
                }
            }
            Err(e) => {
                eprintln!("{}: {}", "\nError calling Gemini API".red().bold(), e);
                std::process::exit(1); // Exit with error code on API failure
            }
        }
    } else if !history_config_changed { // Only show usage if no other action was taken
        // No prompt provided and no config flags used
        println!("{}", "No prompt provided. Use 'gemini \"your prompt\"' to query Gemini.".yellow());
        println!("{}", "Use --set-api-key, --set-system-prompt, or --show-config for configuration.".cyan());
        println!("{}", "Use --help for more options.".cyan());
    }

    Ok(())
}

async fn call_gemini_api(
    client: &Client,
    api_key: &str,
    system_prompt: Option<&str>,
    user_prompt: &str,
    chat_history: Option<&ChatHistory>,
) -> Result<String, Box<dyn Error>> {
    let model = "gemini-1.5-flash-latest";
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model,
        api_key
    );

    // Create system instruction if provided
    let system_instruction = system_prompt.map(|p| Content {
        parts: vec![Part { text: p.to_string() }],
        role: None,
    });

    // Convert chat history to content format expected by Gemini API
    let mut contents = Vec::new();
    
    // Add previous messages from history if available
    if let Some(history) = chat_history {
        for msg in &history.messages {
            // Skip system messages in the main content list if they exist
            if msg.role != "system" {
                contents.push(Content {
                    parts: vec![Part { text: msg.content.clone() }],
                    role: Some(msg.role.clone()), // Ensure role is set for user/assistant
                });
            }
        }
    } else {
        // If no history, just add the current user prompt
         contents.push(Content {
            parts: vec![Part { text: user_prompt.to_string() }],
            role: Some("user".to_string()), // Explicitly set role
        });
    }

    // Ensure the current user prompt is always the last message in the request `contents`
    // It might already be there if it was the last message added to history before calling
    // Remove the last message if it is identical to the current user prompt
    if let Some(last_msg) = contents.last() {
        if last_msg.role.as_deref() == Some("user") && last_msg.parts.first().map_or(false, |p| p.text == user_prompt) {
            contents.pop();
        }
    }
    // Add the current user prompt as the final part of the conversation
    contents.push(Content {
        parts: vec![Part { text: user_prompt.to_string() }],
        role: Some("user".to_string()),
    });

    let request_body = GenerateContentRequest {
        contents,
        system_instruction,
    };

    // Optional: Log the request body in debug mode
    if std::env::var("GEMINI_DEBUG").is_ok() {
         if let Ok(json_req) = serde_json::to_string_pretty(&request_body) {
            println!("{}", "\n--- API Request Body ---".purple());
            println!("{}", json_req);
            println!("{}", "------------------------".purple());
        }
    }

    let res = client
        .post(&url)
        .json(&request_body)
        .send()
        .await?;

    if res.status().is_success() {
        let response_data: GenerateContentResponse = res.json().await?;
        // Optional: Log the response body in debug mode
        if std::env::var("GEMINI_DEBUG").is_ok() {
             if let Ok(json_res) = serde_json::to_string_pretty(&response_data) {
                println!("{}", "\n--- API Response Body ---".purple());
                println!("{}", json_res);
                println!("{}", "-------------------------".purple());
            }
        }

        if let Some(candidate) = response_data.candidates.first() {
            if let Some(part) = candidate.content.parts.first() {
                return Ok(part.text.clone());
            }
        }
        Err("No content found in Gemini response".yellow().into())
    } else {
        let status = res.status();
        let error_text = res.text().await.unwrap_or_else(|_| "Could not read error body".to_string());
        Err(format!("{} {}: {}", "API request failed with status".red(), status, error_text).into())
    }
}
