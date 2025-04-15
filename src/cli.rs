use clap::Parser;

/// Simple CLI to interact with Google Gemini models
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The prompt to send to the Gemini model (default positional argument)
    #[arg(index = 1)] // Positional argument
    pub prompt: Option<String>,

    /// Prepend prompt with "Provide the Linux command for: "
    #[arg(short, long, default_value_t = false)]
    pub command_help: bool,

    /// Set and save the Gemini API Key persistently
    #[arg(long)]
    pub set_api_key: Option<String>,

    /// Set and save the system prompt persistently
    #[arg(long)]
    pub set_system_prompt: Option<String>,

    /// Show the current configuration
    #[arg(long, default_value_t = false)]
    pub show_config: bool,

    /// Enable memory-based conversation history (default)
    #[arg(long, default_value_t = false)]
    pub enable_history: bool,

    /// Disable conversation history
    #[arg(long, default_value_t = false)]
    pub disable_history: bool,

    /// Start a new conversation (don't use previous history)
    #[arg(long, default_value_t = false)]
    pub new_chat: bool,

    /// Run as a filesystem MCP server (internal use only)
    #[arg(long, default_value_t = false)]
    pub filesystem_mcp: bool,
    
    /// Run as a command execution MCP server (internal use only)
    #[arg(long, default_value_t = false)]
    pub command_mcp: bool,
    
    /// Run as a memory MCP server (internal use only)
    #[arg(long, default_value_t = false)]
    pub memory_mcp: bool,
} 