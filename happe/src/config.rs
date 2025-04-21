use gemini_core::config::{GeminiConfig, UnifiedConfig, get_unified_config_path};
use gemini_mcp::McpServerConfig;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::env;

/// Application configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    /// Path to the socket for IDA daemon
    pub ida_socket_path: PathBuf,
    
    /// Path to the HAPPE socket
    pub happe_socket_path: PathBuf,
    
    /// Gemini API configuration
    pub gemini: GeminiConfig,
    
    /// Configuration for MCP servers
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
        AppConfig {
            ida_socket_path: PathBuf::from("/tmp/gemini_suite_ida.sock"), // Match IDA's default
            happe_socket_path: PathBuf::from("/tmp/gemini_suite_happe.sock"),
            gemini: GeminiConfig::default(),
            mcp_config: Vec::new(),
            mcp_config_path: None,
            system_prompt: String::new(),
            http_enabled: false,
            http_bind_addr: String::new(),
        }
    }
}

impl AppConfig {
    /// Attempt to load configuration from a file directly
    /// This function should only be used for backward compatibility
    /// or when explicitly specifying a custom config path
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("Configuration file does not exist: {}", path.display()));
        }
        
        // Check if this is the unified config
        if path.file_name().map_or(false, |f| f == "config.toml") {
            return Self::load_from_unified_config(path);
        }
        
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => return Err(format!("Failed to read configuration file: {}", e)),
        };
        
        let config: Self = match toml::from_str(&content) {
            Ok(config) => config,
            Err(e) => return Err(format!("Failed to parse configuration file: {}", e)),
        };
        
        Ok(config)
    }
    
    /// Get the unified config directory
    pub fn get_config_dir() -> Option<PathBuf> {
        // Check environment variable first
        if let Ok(env_path) = env::var("GEMINI_CONFIG_DIR") {
            let path = PathBuf::from(env_path);
            if path.exists() {
                return Some(path);
            }
        }
        
        // Fall back to default location
        if let Some(config_home) = dirs::config_dir() {
            let path = config_home.join("gemini-suite");
            if path.exists() {
                return Some(path);
            }
        }
        
        None
    }
    
    /// Load configuration from default location
    pub fn load_from_default() -> Result<Self, String> {
        // Always try to load from unified config
        if let Ok(path) = get_unified_config_path() {
            if path.exists() {
                return Self::load_from_unified_config(&path);
            }
        }
        
        // Return default config if no file found
        Ok(Self::default())
    }
    
    /// Load from the unified configuration file
    pub fn load_from_unified_config(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Err(format!("Unified configuration file does not exist: {}", path.display()));
        }
        
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) => return Err(format!("Failed to read unified configuration file: {}", e)),
        };
        
        let unified_config: UnifiedConfig = match toml::from_str(&content) {
            Ok(config) => config,
            Err(e) => return Err(format!("Failed to parse unified configuration file: {}", e)),
        };
        
        // Convert from unified config to app config
        let mut app_config = Self::default();
        
        // Set Gemini config and store system prompt if available before we move the config
        let system_prompt = unified_config.gemini.system_prompt.clone();
        
        // Set Gemini config
        app_config.gemini = unified_config.gemini;
        
        // Set HAPPE config if present
        if let Some(happe_config) = unified_config.happe {
            if let Some(ida_socket) = happe_config.ida_socket_path {
                app_config.ida_socket_path = PathBuf::from(ida_socket);
            }
            
            if let Some(happe_socket) = happe_config.happe_socket_path {
                app_config.happe_socket_path = PathBuf::from(happe_socket);
            }
            
            if let Some(http_enabled) = happe_config.http_enabled {
                app_config.http_enabled = http_enabled;
            }
            
            if let Some(http_bind) = happe_config.http_bind_addr {
                app_config.http_bind_addr = http_bind;
            }
            
            // If happe has a system prompt, use it
            if let Some(prompt) = happe_config.system_prompt {
                if !prompt.is_empty() {
                    app_config.system_prompt = prompt;
                }
            }
        }
        
        // If we have a system prompt from Gemini config and no HAPPE system prompt, use it
        if app_config.system_prompt.is_empty() {
            if let Some(prompt) = system_prompt {
                app_config.system_prompt = prompt;
            }
        }
        
        // MCP configuration is now handled separately via mcp_servers.json
        // Not loading MCP servers from unified config as that field has been removed
        app_config.mcp_config_path = Some(PathBuf::from("mcp_servers.json"));
        
        Ok(app_config)
    }
    
    /// Load MCP config from file (this should only be used if not using mcpd)
    pub fn load_mcp_config(&mut self) -> Result<(), String> {
        // If we already have MCP config from unified config, we're done
        if !self.mcp_config.is_empty() {
            return Ok(());
        }
        
        // Fall back to loading from default location
        match gemini_mcp::load_mcp_servers() {
            Ok(servers) => {
                self.mcp_config = servers;
                Ok(())
            },
            Err(e) => Err(format!("Failed to load MCP config from default location: {}", e)),
        }
    }
}
