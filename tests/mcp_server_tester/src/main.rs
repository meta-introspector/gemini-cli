use anyhow::{Context, Result};
use gemini_core::config::McpServerConfig;
use gemini_mcp::McpHost;
use log::{debug, error, info};
use std::fs;
use std::path::Path;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logger with more detailed configuration
    env_logger::builder()
        .filter_level(log::LevelFilter::Info) // Default level for other crates
        .filter_module("gemini_mcp", log::LevelFilter::Debug) // Enable DEBUG for McpHost internals
        .filter_module("mcp_server_tester", log::LevelFilter::Debug) // Enable DEBUG for this crate
        .format_timestamp_millis() // Add timestamps
        .init();

    info!("Starting MCP Server Initialization Test (from config file)...");

    let config_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Could not get parent directory"))?
        .join("mcp_servers.json");

    debug!("Reading MCP configuration from: {}", config_path.display());
    let config_content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config: {}", config_path.display()))?;

    let mcp_configs: Vec<McpServerConfig> = serde_json::from_str(&config_content)
        .with_context(|| format!("Failed to parse mcp_servers.json at {}", config_path.display()))?;

    debug!("Successfully parsed {} server configurations.", mcp_configs.len());

    info!(
        "Attempting to initialize McpHost (launching {} servers defined in config)...",
        mcp_configs.len()
    );

    match McpHost::new(mcp_configs).await {
        Ok(host) => {
            info!("✅ SUCCESS: McpHost initialized successfully.");
            let capabilities = host.get_all_capabilities().await;
            info!("Combined capabilities fetched:");
            debug!("Tools:");
            for tool in capabilities.tools {
                debug!("  - {} ({:?})", tool.name, tool.description.unwrap_or_default());
            }
            debug!("Resources:");
             for resource in capabilities.resources {
                debug!("  - {}", resource.name);
            }
            info!("Shutting down McpHost...");
            host.shutdown().await;
            info!("McpHost shutdown complete.");
            Ok(())
        }
        Err(e) => {
            error!("❌ FAILED: McpHost initialization encountered an error.");
            error!("==================== ERROR DETAILS ====================");
            error!("{:#}", e);
            error!("=====================================================");
            Err(anyhow::Error::msg(e))
        }
    }
} 