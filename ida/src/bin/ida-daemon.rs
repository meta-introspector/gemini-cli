// Use `ida::` to refer to the library crate from the binary
use ida::{ipc_server, llm_clients};
use std::path::PathBuf;
use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use anyhow::{Context, Result, anyhow};
use std::sync::Arc;

// Removed MCP host/client imports

use gemini_memory::MemoryStore;
use gemini_core::config::UnifiedConfig;

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
    /// Path to the configuration file (Note: Unified config is loaded by default)
    config: Option<PathBuf>, // Keep for potential future direct config loading
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

    // Access IDA specific config directly from UnifiedConfig
    let config = unified_config.ida; // Assuming unified_config.ida is IdaConfig

    // Resolve memory DB path: Use path from config if present, otherwise use default
    let resolved_db_path = match config.memory_db_path.clone() {
        Some(path) => path,
        None => {
            // Determine default path based on user config or data dir
            let base_dir = dirs::config_dir()
                .or_else(dirs::data_local_dir)
                .ok_or_else(|| anyhow!("Could not determine config or local data directory"))?;
            base_dir.join("gemini-suite/memory/db")
        }
    };

    info!("Using memory DB path: {}", resolved_db_path.display());
    // Ensure the directory exists
    if let Some(parent) = resolved_db_path.parent() {
        std::fs::create_dir_all(parent)
            .context(format!("Failed to create directory for memory DB at {}", parent.display()))?;
    }

    // ---- Removed MCP Host/Client Initialization ----

    // Initialize MemoryStore
    info!("Initializing MemoryStore at {}", resolved_db_path.display());
    let memory_store = Arc::new(
        MemoryStore::new(
            Some(resolved_db_path.clone()), // Use resolved path
            None, // Use default embedding model variant from MemoryStore defaults for now
            None, // Pass None for McpHostInterface
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
        config.clone(), // Pass the IDA config (still gemini_core::IdaConfig type)
        memory_store,
        None, // Pass None for McpHostInterface here too, if needed by run_server signature
        llm_client,
    )
    .await
    {
        error!("IDA Daemon failed: {}", e);
        return Err(e.into());
    }

    Ok(())
}
