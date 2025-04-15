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

    /// Enter interactive chat mode with continuous conversation
    #[arg(short, long, default_value_t = false)]
    pub interactive: bool,

    /// Start a task loop with the given description, allowing the AI to work on a task autonomously
    #[arg(short, long)]
    pub task: Option<String>,

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

    /// Enable memory broker for enhancing queries with relevant memories (default)
    #[arg(long, default_value_t = false)]
    pub enable_memory_broker: bool,

    /// Disable memory broker
    #[arg(long, default_value_t = false)]
    pub disable_memory_broker: bool,

    /// Enable automatic memory storage for important information (default)
    #[arg(long, default_value_t = false)]
    pub enable_auto_memory: bool,

    /// Disable automatic memory storage
    #[arg(long, default_value_t = false)]
    pub disable_auto_memory: bool,

    /// Run as a filesystem MCP server (internal use only)
    #[arg(long, default_value_t = false)]
    pub filesystem_mcp: bool,
    
    /// Run as a command execution MCP server (internal use only)
    #[arg(long, default_value_t = false)]
    pub command_mcp: bool,

    /// Run as a memory store MCP server (internal use only)
    #[arg(long, default_value_t = false)]
    pub memory_store_mcp: bool,
} 