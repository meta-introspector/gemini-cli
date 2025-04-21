use clap::Parser;
use gemini_core::client::GeminiClient;
use gemini_core::config::UnifiedConfig;
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

    // Load config from unified configuration
    let mut config = if let Some(custom_config_path) = &args.config {
        // Legacy support for custom config file path
        match std::fs::read_to_string(custom_config_path) {
            Ok(content) => match toml::from_str::<UnifiedConfig>(&content) {
                Ok(unified_config) => {
                    info!(
                        "Loaded unified configuration from custom path: {}",
                        custom_config_path.display()
                    );
                    AppConfig::from_unified_config(&unified_config)
                }
                Err(e) => {
                    error!("Failed to parse custom config as unified config: {}", e);
                    return Err(anyhow::anyhow!("Configuration parse error: {}", e));
                }
            },
            Err(e) => {
                error!(
                    "Failed to read custom config file {}: {}",
                    custom_config_path.display(),
                    e
                );
                return Err(anyhow::anyhow!("Failed to read config file: {}", e));
            }
        }
    } else {
        // Use unified config
        match AppConfig::load() {
            Ok(cfg) => {
                info!("Loaded unified configuration");
                cfg
            }
            Err(e) => {
                error!("Failed to load unified configuration: {}", e);
                return Err(anyhow::anyhow!("Configuration error: {}", e));
            }
        }
    };

    // Update config from CLI args (these take precedence over config file)
    if let Some(ida_socket) = args.ida_socket {
        config.ida_socket_path = ida_socket;
    }

    if let Some(system_prompt) = args.system_prompt {
        config.system_prompt = system_prompt;
    }

    if let Some(happe_socket) = args.happe_socket {
        config.happe_socket_path = happe_socket;
    }

    if let Some(http_addr) = args.http_addr {
        config.http_bind_addr = http_addr.to_string();
        config.http_enabled = true;
    }

    if args.no_http {
        config.http_enabled = false;
    }

    // Update Gemini config
    if let Some(api_key) = args.api_key {
        config.gemini.api_key = Some(api_key);
    }

    if let Some(model) = args.model {
        config.gemini.model_name = Some(model);
    }

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
    let mcp_socket_path = args
        .mcp_socket
        .or_else(|| config.mcp.mcp_host_socket_path.clone())
        .unwrap_or_else(|| McpHostClient::get_default_socket_path());

    info!(
        "Using MCP host daemon socket at {}",
        mcp_socket_path.display()
    );
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
    if !args.no_ipc && config.http_enabled {
        let http_config = config.clone();
        let http_client = gemini_client.clone();
        let http_mcp_client = mcp_client.clone();
        let http_addr: SocketAddr = config
            .http_bind_addr
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid HTTP address: {}", e))?;

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
        let ipc_config = config.clone();
        let ipc_client = gemini_client.clone();
        let ipc_mcp_client = mcp_client.clone();
        let ipc_socket = config.happe_socket_path.clone();

        tasks.push(tokio::spawn(async move {
            if let Err(e) =
                ipc_server::run_server(ipc_socket, ipc_config, ipc_client, ipc_mcp_client).await
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
