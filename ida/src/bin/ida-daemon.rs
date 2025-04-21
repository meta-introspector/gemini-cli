// Use `ida::` to refer to the library crate from the binary
use ida::{config::IdaConfig, ipc_server, llm_clients};
use std::path::PathBuf;
use clap::Parser;
use tracing::{error, info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use anyhow::{Context, Result, anyhow};
use std::sync::Arc;

// Import MCP host for initialization
use gemini_mcp::host::McpHost;
use gemini_memory::broker::McpHostInterface; // Trait needed for Arc type
use gemini_memory::MemoryStore;
use gemini_core::config::UnifiedConfig; // Import UnifiedConfig

// Define command-line arguments using clap
#[derive(Parser, Debug)]
#[clap(
    name = "ida-daemon",
    about = "Internal Dialogue App Daemon for Gemini Suite",
    version
)]
struct Args {
    #[clap(long, env = "IDA_IPC_PATH")]
    /// Path to the Unix socket for IPC communication with HAPPE
    ipc_path: Option<String>,

    #[clap(long, env = "IDA_MEMORY_PATH")]
    /// Path to the memory database directory. If not provided, uses default in user config dir
    memory_path: Option<PathBuf>,

    #[clap(long, env = "IDA_MAX_MEMORY_RESULTS")]
    /// Maximum number of memory items to return per query
    max_memory_results: Option<usize>,

    #[clap(long, env = "IDA_THRESHOLD")]
    /// Semantic similarity threshold for memory retrieval (0.0 to 1.0)
    similarity_threshold: Option<f32>,

    #[clap(long, env = "IDA_LOG_LEVEL", default_value = "info")]
    /// Log level (trace, debug, info, warn, error)
    log_level: String,

    #[clap(short, long)]
    /// Path to the configuration file
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize tracing (logging)
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level)))
        .init();

    info!("Starting IDA Daemon...");

    // Load Unified Configuration
    let unified_config = UnifiedConfig::load(); // Load the unified config

    // Use IDA specific config from UnifiedConfig or defaults
    let config = unified_config.ida.unwrap_or_default();

    // Resolve the memory DB path to be absolute using the unified config's helper
    let resolved_db_path = unified_config.resolve_path(&config.memory_db_path, Some("memory/db"), true)
        .context("Failed to resolve memory database path")?;
    info!("Resolved memory DB path: {}", resolved_db_path.display());

    // Initialize MCP Host (required by MemoryStore for embeddings)
    // Get MCP socket path from unified config
    let mcp_socket_path = unified_config.resolve_mcp_host_socket_path()
        .context("Failed to determine MCP host socket path from config")?;

    let mcp_host = McpHost::new(mcp_socket_path).await
        .map_err(|e| anyhow!("Failed to create MCP Host: {}", e)) // Map String error to anyhow::Error
        .context("Failed to initialize MCP Host client for IDA")?;
    let mcp_host_interface: Arc<dyn McpHostInterface + Send + Sync> = Arc::new(mcp_host);
    info!("MCP Host interface initialized.");

    // Initialize MemoryStore
    info!("Initializing MemoryStore at {}", resolved_db_path.display());
    let memory_store = Arc::new(
        MemoryStore::new(
            Some(resolved_db_path.clone()), // Use resolved path
            None, // Use default embedding model variant from MemoryStore defaults for now
            Some(mcp_host_interface.clone()), // Provide the MCP interface
        )
        .await
        .context("Failed to initialize MemoryStore")?,
    );
    info!("MemoryStore initialized successfully.");

    // Initialize Broker LLM Client
    let llm_client = llm_clients::create_llm_client(&config.memory_broker)
        .context("Failed to create LLM client based on configuration")?;
    if llm_client.is_some() {
        info!("Broker LLM client initialized successfully.");
    } else {
        info!("No Broker LLM provider configured.");
    }

    // Start the IPC server, passing all components
    info!("Starting IPC server...");
    if let Err(e) = ipc_server::run_server(
        config.clone(), // Pass the full loaded config
        memory_store, 
        Some(mcp_host_interface), // Pass the MCP interface
        llm_client, // Pass the optional LLM client
    ).await {
        error!("IDA Daemon failed: {}", e);
        return Err(e.into());
    }

    Ok(())
}
