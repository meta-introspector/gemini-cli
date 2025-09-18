use clap::Parser;
use std::path::PathBuf;

/// Simple CLI client for the HAPPE daemon
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The prompt to send to the HAPPE daemon
    #[arg(index = 1)] // Positional argument
    pub prompt: Option<String>,

    /// Path to one or more task files to execute in batch mode
    #[arg(short, long)]
    pub task_files: Option<Vec<PathBuf>>,

    /// Enter interactive chat mode
    #[arg(short, long, default_value_t = false)]
    pub interactive: bool,

    /// Start a new session, ignoring any existing session ID
    #[arg(long, default_value_t = false)]
    pub new_session: bool,

    /// Enable verbose output
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// List active sessions and let the user select one
    #[arg(long, default_value_t = false)]
    pub select_session: bool,

    /// Specify the path to the HAPPE daemon IPC socket
    #[arg(long, env = "HAPPE_IPC_PATH")]
    pub happe_ipc_path: Option<PathBuf>,

    /// Run in filesystem MCP server mode (Kept for standalone server functionality)
    #[arg(long, default_value_t = false)]
    pub filesystem_mcp: bool,

    /// Run in command MCP server mode (Kept for standalone server functionality)
    #[arg(long, default_value_t = false)]
    pub command_mcp: bool,

    /// Run in memory store MCP server mode (Kept for standalone server functionality)
    #[arg(long, default_value_t = false)]
    pub memory_store_mcp: bool,
}
