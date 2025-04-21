// Use `ida::` to refer to the library crate from the binary
use ida::{config::IdaConfig, ipc_server, memory_mcp_client, storage};
use std::path::PathBuf;
use clap::Parser;
use tracing::{error, info};
use gemini_core::config::get_unified_config_path;

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
async fn main() {
    // Parse command-line arguments
    let args = Args::parse();
    
    // Initialize tracing subscriber with the specified log level
    let log_level = match args.log_level.as_str() {
        "trace" => tracing::Level::TRACE,
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => {
            eprintln!("Invalid log level: {}. Using 'info' instead.", args.log_level);
            tracing::Level::INFO
        }
    };
    
    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .init();

    info!("Starting IDA Daemon...");

    // Load configuration (file or default)
    let mut config = if let Some(config_path) = &args.config {
        match IdaConfig::load_from_file(config_path) {
            Ok(cfg) => {
                info!("Loaded configuration from {}", config_path.display());
                cfg
            }
            Err(e) => {
                error!("Failed to load configuration from {}: {}", config_path.display(), e);
                std::process::exit(1);
            }
        }
    } else {
        // Try to load from unified config
        match IdaConfig::load_from_default() {
            Ok(cfg) => {
                match get_unified_config_path() {
                    Ok(path) => {
                        info!("Loaded configuration from {}", path.display());
                    },
                    Err(_) => {
                        info!("Using default configuration (no unified config found)");
                    }
                }
                cfg
            }
            Err(e) => {
                error!("Failed to load default configuration: {}", e);
                error!("Exiting due to configuration error");
                std::process::exit(1);
            }
        }
    };
    
    // Override config with command-line args if provided
    if let Some(ipc_path) = args.ipc_path {
        config.ida_socket_path = PathBuf::from(ipc_path);
    }
    
    if let Some(memory_path) = args.memory_path {
        config.memory_db_path = memory_path;
    }
    
    if let Some(max_results) = args.max_memory_results {
        config.max_memory_results = max_results;
    }
    
    if let Some(threshold) = args.similarity_threshold {
        config.semantic_similarity_threshold = threshold;
    }
    
    // Resolve any relative paths in the config
    let memory_db_path = match config.resolve_memory_db_path() {
        Ok(path) => {
            info!("Using memory database path: {}", path.display());
            path
        }
        Err(e) => {
            error!("Failed to resolve memory database path: {}", e);
            error!("Using unresolved path: {}", config.memory_db_path.display());
            config.memory_db_path.clone()
        }
    };

    info!("Configuration loaded: ida_socket_path={}, max_memory_results={}, similarity_threshold={}",
          config.ida_socket_path.display(), config.max_memory_results, config.semantic_similarity_threshold);

    // Create IPC server configuration 
    let server_config = ipc_server::DaemonConfig {
        ipc_path: config.ida_socket_path.to_string_lossy().to_string(),
        memory_db_path: Some(memory_db_path),
        max_memory_results: config.max_memory_results,
    };

    // Run the IPC server
    if let Err(e) = ipc_server::run_server(server_config).await {
        error!("IDA Daemon failed: {}", e);
        std::process::exit(1); // Exit with error code if server fails
    }

    info!("IDA Daemon shut down gracefully."); // This line might not be reached if run_server loops indefinitely
}
