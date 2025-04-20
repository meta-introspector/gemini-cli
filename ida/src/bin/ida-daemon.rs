// Use `ida::` to refer to the library crate from the binary
use ida::{ipc_server, memory_mcp_client, storage};
use tracing::{error, info};

// Modules are declared in src/lib.rs

// Placeholder configuration - replace with proper loading later
const DEFAULT_IPC_PATH: &str = "/tmp/gemini_suite_ida.sock";

// Ensure Args struct and clap related code is removed

#[tokio::main]
async fn main() {
    // Initialize tracing subscriber
    tracing_subscriber::fmt::init();

    info!("Starting IDA Daemon...");

    // Create placeholder configuration
    let config = ipc_server::DaemonConfig {
        ipc_path: DEFAULT_IPC_PATH.to_string(),
        // Add other fields as needed
    };

    info!("Configuration loaded: {:?}", config);

    // Run the IPC server
    if let Err(e) = ipc_server::run_server(config).await {
        error!("IDA Daemon failed: {}", e);
        std::process::exit(1); // Exit with error code if server fails
    }

    info!("IDA Daemon shut down gracefully."); // This line might not be reached if run_server loops indefinitely
}
