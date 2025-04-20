use clap::Parser;
use happe::config::AppConfig;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the IDA daemon Unix socket
    #[arg(short, long, default_value = "/tmp/ida.sock")]
    ida_socket_path: String,

    /// Placeholder for LLM endpoint
    #[arg(short, long, default_value = "http://localhost:8080")]
    llm_endpoint: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Starting HAPPE Daemon...");

    let args = Args::parse();

    // Create configuration (replace with proper config loading later if needed)
    let config = AppConfig {
        ida_socket_path: args.ida_socket_path,
        llm_endpoint: args.llm_endpoint,
    };

    info!(?config, "Using configuration");

    // Start the main application logic (coordinator)
    if let Err(e) = happe::coordinator::run_coordinator(config).await {
        error!(error = %e, "Coordinator failed");
        return Err(e);
    }

    info!("HAPPE Daemon finished.");
    Ok(())
}
