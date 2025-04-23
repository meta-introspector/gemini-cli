use clap::Parser;
use gemini_core::client::GeminiClient;
use gemini_core::config::{UnifiedConfig, HappeConfig, get_unified_config_path};
use gemini_happe::http_server;
use gemini_happe::ipc_server;
use gemini_happe::mcp_client::McpHostClient;
use std::net::SocketAddr;
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;
use std::path::PathBuf;
use std::fs; // Added for create_dir_all

#[derive(Parser, Debug)]
#[command(name = "happe-daemon", about = "HAPPE daemon for Gemini Suite")]
struct Args {
    /// Path to config file (if not using the unified config)
    #[arg(short, long)]
    config: Option<PathBuf>,

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
    #[arg(short = 'o', long)]
    model: Option<String>,

    /// HTTP server address
    #[arg(long)]
    http_addr: Option<SocketAddr>,

    /// Path to the HAPPE IPC socket
    #[arg(long)]
    happe_socket: Option<PathBuf>,

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

// Helper function to determine the MCP socket path (mirrors mcp-hostd logic)
fn get_dynamic_mcp_socket_path() -> anyhow::Result<PathBuf> {
    let base_dir = dirs::runtime_dir()
        .or_else(dirs::data_local_dir)
        .ok_or_else(|| anyhow::anyhow!("Could not determine runtime or data local directory"))?;
    let socket_dir = base_dir.join("gemini-cli"); // Consistent directory name
    fs::create_dir_all(&socket_dir) // Ensure the directory exists
        .map_err(|e| anyhow::anyhow!("Failed to create socket directory: {}", e))?;
    Ok(socket_dir.join("mcp-hostd.sock")) // Consistent socket filename
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    info!("Starting HAPPE daemon");

    // Parse command line args
    let args = Args::parse();

    // Determine which HAPPE config to use: component-specific if given, else unified
    let unified_config = UnifiedConfig::load();
    let mut config: HappeConfig = if let Some(path) = args.config.clone() {
        // Load component-specific HAPPE config file
        let content = std::fs::read_to_string(&path)?;
        let cfg: HappeConfig = toml::from_str(&content)?;
        info!("Loaded HAPPE configuration from {}", path.display());
        cfg
    } else {
        // Load from the unified configuration file
        let cfg_path = get_unified_config_path()
            .map_err(|e| anyhow::anyhow!("Could not determine unified config path: {}", e))?;
        info!("LOADING HAPPE CONFIG FROM UNIFIED CONFIG FILE: {}", cfg_path.display());
        unified_config.happe.clone()
    };

    // Override with CLI args if provided
    if let Some(ida_socket) = args.ida_socket {
        config.ida_socket_path = Some(ida_socket);
    }
    if let Some(system_prompt) = args.system_prompt {
        config.system_prompt = Some(system_prompt);
    }
    if let Some(happe_socket) = args.happe_socket {
        config.happe_socket_path = Some(happe_socket);
    }
    if let Some(http_addr) = args.http_addr {
        config.http_bind_addr = Some(http_addr.to_string());
        config.http_enabled = Some(true);
    }
    if args.no_http {
        config.http_enabled = Some(false);
    }

    // --- Extract Gemini settings from unified config, applying CLI overrides --- 
    let mut gemini_config = unified_config.gemini_api; // Removed .unwrap_or_default()
    if let Some(api_key) = args.api_key {
        gemini_config.api_key = Some(api_key);
    }
    if let Some(model) = args.model {
        gemini_config.model_name = Some(model);
    }
    // --- --- 

    // --- Extract MCP settings from unified config, applying CLI overrides --- 
    let mut mcp_config = unified_config.mcp;
    if let Some(mcp_socket) = args.mcp_socket {
        mcp_config.mcp_host_socket_path = Some(mcp_socket);
    }
    // --- --- 

    // Initialize Gemini client using the resolved Gemini config
    let gemini_client = match GeminiClient::new(gemini_config) { // Pass the resolved gemini_config
        Ok(client) => {
            info!("Initialized Gemini client");
            client
        }
        Err(e) => {
            error!(error = %e, "Failed to initialize Gemini client");
            return Err(anyhow::anyhow!("Failed to initialize Gemini client: {}", e));
        }
    };

    // Determine and initialize MCP host client socket path
    let mcp_socket_path = match mcp_config.mcp_host_socket_path {
        Some(path) => {
            info!("Using configured MCP host daemon socket path: {}", path.display());
            path
        }
        None => {
            info!("MCP host socket path not configured, attempting dynamic resolution...");
            match get_dynamic_mcp_socket_path() {
                Ok(path) => {
                    info!("Dynamically resolved MCP host daemon socket path: {}", path.display());
                    path
                }
                Err(e) => {
                     error!("Failed to dynamically resolve MCP host socket path: {}", e);
                     return Err(e); // Propagate the error
                }
            }
        }
    };

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
            return Err(anyhow::anyhow!(
                "Failed to connect to MCP host daemon: {}",
                e
            ));
        }
    }

    // Start servers
    let mut tasks = Vec::new();

    // Start HTTP server if enabled
    if config.http_enabled.unwrap_or(false) { // Handle Option<bool>
        let http_config = config.clone();
        let http_client = gemini_client.clone();
        let http_mcp_client = mcp_client.clone();
        let http_addr_str = config.http_bind_addr.clone().unwrap_or_else(|| "127.0.0.1:3000".to_string());
        let http_addr: SocketAddr = http_addr_str
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid HTTP address '{}': {}", http_addr_str, e))?;

        tasks.push(tokio::spawn(async move {
            if let Err(e) =
                http_server::run_server(http_config, http_client, http_mcp_client, http_addr).await
            {
                error!(error = %e, "HTTP server failed");
            }
        }));
    }

    // Start IPC server if enabled
    if !args.no_ipc {
        let ipc_config = config.clone(); // Clone HAPPE config
        let ipc_client = gemini_client.clone();
        let ipc_mcp_client = mcp_client.clone();
        let ipc_socket_path = config
            .happe_socket_path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("HAPPE IPC socket path not configured"))?;

        tasks.push(tokio::spawn(async move {
            if let Err(e) =
                ipc_server::run_server(ipc_socket_path, ipc_config, ipc_client, ipc_mcp_client).await
            {
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
