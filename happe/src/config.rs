use gemini_core::config::{GeminiApiConfig, HappeConfig, McpConfig, UnifiedConfig};
use gemini_mcp::McpServerConfig;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Application configuration that combines unified config with runtime settings
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Path to the socket for IDA daemon
    pub ida_socket_path: PathBuf,

    /// Path to the HAPPE socket
    pub happe_socket_path: PathBuf,

    /// Gemini API configuration
    pub gemini: GeminiApiConfig,

    /// MCP configuration
    pub mcp: McpConfig,

    /// Configuration for MCP servers (loaded separately)
    pub mcp_config: Vec<McpServerConfig>,

    /// Path to MCP config file (relative or absolute)
    pub mcp_config_path: Option<PathBuf>,

    /// System prompt to use for interactions
    pub system_prompt: String,

    /// Whether HTTP is enabled
    pub http_enabled: bool,

    /// Bind address for HTTP server
    pub http_bind_addr: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::from_unified_config(&UnifiedConfig::default())
    }
}

impl AppConfig {
    /// Convert from UnifiedConfig to AppConfig
    pub fn from_unified_config(config: &UnifiedConfig) -> Self {
        // Set default values
        let mut app_config = AppConfig {
            ida_socket_path: PathBuf::from("/tmp/gemini_suite_ida.sock"),
            happe_socket_path: PathBuf::from("/tmp/gemini_suite_happe.sock"),
            gemini: config.gemini_api.clone(),
            mcp: config.mcp.clone(),
            mcp_config: Vec::new(),
            mcp_config_path: None,
            system_prompt: String::new(),
            http_enabled: false,
            http_bind_addr: String::from("127.0.0.1:8080"),
        };

        // Get system prompt from Gemini API config
        if let Some(prompt) = &config.gemini_api.system_prompt {
            app_config.system_prompt = prompt.clone();
        }

        // Override with HAPPE-specific config if available
        let happe_config = &config.happe;

        // Set IDA socket path
        if let Some(ida_socket) = &happe_config.ida_socket_path {
            app_config.ida_socket_path = ida_socket.clone();
        }

        // Set HAPPE socket path
        if let Some(happe_socket) = &happe_config.happe_socket_path {
            app_config.happe_socket_path = happe_socket.clone();
        }

        // Set HTTP settings
        if let Some(http_enabled) = happe_config.http_enabled {
            app_config.http_enabled = http_enabled;
        }

        if let Some(http_bind) = &happe_config.http_bind_addr {
            app_config.http_bind_addr = http_bind.clone();
        }

        // Override system prompt with HAPPE-specific one if set
        if let Some(prompt) = &happe_config.system_prompt {
            if !prompt.is_empty() {
                app_config.system_prompt = prompt.clone();
            }
        }

        // Set MCP config path
        if let Some(mcp_path) = &config.mcp.mcp_servers_file_path {
            app_config.mcp_config_path = Some(mcp_path.clone());
        } else {
            // Use default path
            app_config.mcp_config_path = Some(PathBuf::from("mcp_servers.json"));
        }

        app_config
    }

    /// Load AppConfig from the unified configuration
    pub fn load() -> Result<Self, String> {
        let unified_config = UnifiedConfig::load();
        Ok(Self::from_unified_config(&unified_config))
    }

    /// Load MCP config from file (this should only be used if not using mcpd)
    pub fn load_mcp_config(&mut self) -> Result<(), String> {
        // If we already have MCP config, we're done
        if !self.mcp_config.is_empty() {
            return Ok(());
        }

        // Fall back to loading from default location or specified path
        match gemini_mcp::load_mcp_servers() {
            Ok(servers) => {
                self.mcp_config = servers;
                Ok(())
            }
            Err(e) => Err(format!("Failed to load MCP config: {}", e)),
        }
    }
}
