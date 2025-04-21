use clap::Parser;
use gemini_core::client::GeminiClient;
use gemini_happe::config::AppConfig;
use gemini_happe::http_server;
use gemini_happe::ipc_server;
use gemini_happe::mcp_client::McpHostClient;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(name = "happe-daemon", about = "HAPPE daemon for Gemini Suite")]
struct Args {
    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Path to MCP config file (unused)
    #[arg(short, long)]
    mcp_config: Option<PathBuf>,
    
    /// Path to socket for IDA daemon
    #[arg(short, long)]
    ida_socket: Option<PathBuf>,
    
    /// System prompt to use
    #[arg(short, long)]
    system_prompt: Option<String>,
    
    /// Gemini API key
    #[arg(short = 'k', long)]
    api_key: Option<String>,
    
    /// Gemini model to use
    #[arg(short = 'o', long, default_value = "gemini-2.5-pro-preview-03-25")]
    model: String,
    
    /// HTTP server address
    #[arg(long, default_value = "127.0.0.1:8080")]
    http_addr: SocketAddr,
    
    /// Path to the HAPPE IPC socket
    #[arg(long, default_value = "/tmp/gemini_suite_happe.sock")]
    happe_socket: PathBuf,
    
    /// Disable HTTP server
    #[arg(long)]
    no_http: bool,
    
    /// Disable IPC server
    #[arg(long)]
    no_ipc: bool,
    
    /// Path to the MCP host daemon socket
    #[arg(long)]
    mcp_socket: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set tracing subscriber");

    info!("Starting HAPPE daemon");

    // Parse command line args
    let args = Args::parse();

    // Load config from file or use defaults
    let mut config = if let Some(config_path) = &args.config {
        match AppConfig::load_from_file(config_path) {
            Ok(cfg) => {
                info!("Loaded configuration from {}", config_path.display());
                cfg
            }
            Err(e) => {
                error!("Failed to load configuration from {}: {}", config_path.display(), e);
                return Err(anyhow::anyhow!("Configuration error: {}", e));
            }
        }
    } else {
        // Try to load from unified config
        match AppConfig::load_from_default() {
            Ok(cfg) => {
                let path = match gemini_core::config::get_unified_config_path() {
                    Ok(p) => p,
                    Err(_) => PathBuf::from("default config")
                };
                info!("Loaded configuration from {}", path.display());
                cfg
            }
            Err(e) => {
                error!("Failed to load configuration: {}", e);
                return Err(anyhow::anyhow!("Configuration error: {}", e));
            }
        }
    };
    
    // Update config from CLI args
    if let Some(ida_socket) = args.ida_socket {
        config.ida_socket_path = ida_socket;
    }
    
    if let Some(system_prompt) = args.system_prompt {
        config.system_prompt = system_prompt;
    }
    
    // Update socket path
    if args.happe_socket != config.happe_socket_path {
        config.happe_socket_path = args.happe_socket;
    }
    
    // Update Gemini config
    if let Some(api_key) = args.api_key {
        config.gemini.api_key = Some(api_key);
    }
    config.gemini.model_name = Some(args.model);
    
    // Initialize Gemini client
    let gemini_client = match GeminiClient::new(config.gemini.clone()) {
        Ok(client) => {
            info!("Initialized Gemini client");
            client
        }
        Err(e) => {
            error!(error = %e, "Failed to initialize Gemini client");
            return Err(anyhow::anyhow!("Failed to initialize Gemini client: {}", e));
        }
    };
    
    // Initialize MCP host client
    let mcp_socket_path = args.mcp_socket
        .unwrap_or_else(|| McpHostClient::get_default_socket_path());
    
    info!("Using MCP host daemon socket at {}", mcp_socket_path.display());
    let mcp_client = McpHostClient::new(mcp_socket_path);
    
    // Test connection to MCP host daemon
    match mcp_client.get_capabilities().await {
        Ok(caps) => {
            info!(
                "Connected to MCP host daemon, found {} tools and {} resources", 
                caps.tools.len(), 
                caps.resources.len()
            );
        }
        Err(e) => {
            error!(error = %e, "Failed to connect to MCP host daemon");
            return Err(anyhow::anyhow!("Failed to connect to MCP host daemon: {}", e));
        }
    }
    
    // Start servers
    let mut tasks = Vec::new();
    
    // Start HTTP server if enabled
    if !args.no_http {
        let http_config = config.clone();
        let http_client = gemini_client.clone();
        let http_mcp_client = mcp_client.clone();
        let http_addr = args.http_addr;
        
        tasks.push(tokio::spawn(async move {
            if let Err(e) = http_server::run_server(http_config, http_client, http_mcp_client, http_addr).await {
                error!(error = %e, "HTTP server failed");
            }
        }));
    }
    
    // Start IPC server if enabled
    if !args.no_ipc {
        let ipc_config = config.clone();
        let ipc_client = gemini_client.clone();
        let ipc_mcp_client = mcp_client.clone();
        let ipc_socket = config.happe_socket_path.clone();
        
        tasks.push(tokio::spawn(async move {
            if let Err(e) = ipc_server::run_server(ipc_socket, ipc_config, ipc_client, ipc_mcp_client).await {
                error!(error = %e, "IPC server failed");
            }
        }));
    }
    
    // Run all tasks concurrently
    for task in tasks {
        if let Err(e) = task.await {
            error!(error = %e, "Task panicked");
        }
    }
    
    info!("HAPPE daemon shutting down");
    Ok(())
} 