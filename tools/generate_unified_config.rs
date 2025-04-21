use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use toml;

/// Unified configuration structure for the entire Gemini Suite
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UnifiedConfig {
    /// Gemini API configuration
    #[serde(default)]
    pub gemini: GeminiConfig,
    
    /// HAPPE daemon configuration
    #[serde(default)]
    pub happe: HappeConfig,
    
    /// IDA daemon configuration
    #[serde(default)]
    pub ida: IdaConfig,
    
    /// MCP server configuration
    #[serde(default)]
    pub mcp: McpConfig,
    
    /// Memory configuration
    #[serde(default)]
    pub memory: MemoryConfig,
}

/// Configuration struct for Gemini API
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct GeminiConfig {
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub model_name: Option<String>,
    pub save_history: Option<bool>,
    pub enable_memory_broker: Option<bool>,
    pub enable_auto_memory: Option<bool>,
    pub memory_broker_model: Option<String>,
}

/// HAPPE daemon configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct HappeConfig {
    /// Socket paths
    pub ida_socket_path: Option<String>,
    pub happe_socket_path: Option<String>,
    
    /// HTTP server settings
    pub http_enabled: Option<bool>,
    pub http_bind_addr: Option<String>,
}

/// IDA daemon configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct IdaConfig {
    /// Socket path
    pub socket_path: Option<String>,
}

/// MCP configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct McpConfig {
    pub servers: Vec<McpServerConfig>,
}

/// MCP server configuration
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub enabled: bool,
    pub transport: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub auto_execute: Vec<String>,
}

/// Memory configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MemoryConfig {
    pub storage_path: Option<String>,
    pub embedding_model: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Generating unified configuration file for Gemini Suite...");
    
    // Define the config directory
    let config_dir = Path::new(&dirs::home_dir().unwrap()).join(".config").join("gemini-suite");
    fs::create_dir_all(&config_dir)?;
    
    // Define the unified config file path
    let unified_config_path = config_dir.join("config.toml");
    
    // Create a default configuration
    let mut unified_config = UnifiedConfig::default();
    
    // Try to load Gemini config if exists
    let gemini_config_file = Path::new(&dirs::home_dir().unwrap())
        .join(".config")
        .join("gemini-cli")
        .join("config.toml");
    
    if gemini_config_file.exists() {
        println!("Loading existing Gemini configuration from {:?}", gemini_config_file);
        let content = fs::read_to_string(&gemini_config_file)?;
        if let Ok(config) = toml::from_str::<GeminiConfig>(&content) {
            unified_config.gemini = config;
        }
    }
    
    // Try to load HAPPE config if exists
    let happe_config_file = config_dir.join("happe").join("config.toml");
    if happe_config_file.exists() {
        println!("Loading existing HAPPE configuration from {:?}", happe_config_file);
        let content = fs::read_to_string(&happe_config_file)?;
        if let Ok(config) = toml::from_str::<HappeConfig>(&content) {
            unified_config.happe = config;
        }
    } else {
        // Set default HAPPE config
        unified_config.happe = HappeConfig {
            ida_socket_path: Some("/tmp/gemini_suite_ida.sock".to_string()),
            happe_socket_path: Some("/tmp/gemini_suite_happe.sock".to_string()),
            http_enabled: Some(true),
            http_bind_addr: Some("127.0.0.1:8080".to_string()),
        };
    }
    
    // Try to load IDA config if exists
    let ida_config_file = config_dir.join("ida").join("config.toml");
    if ida_config_file.exists() {
        println!("Loading existing IDA configuration from {:?}", ida_config_file);
        let content = fs::read_to_string(&ida_config_file)?;
        if let Ok(config) = toml::from_str::<IdaConfig>(&content) {
            unified_config.ida = config;
        }
    } else {
        // Set default IDA config
        unified_config.ida = IdaConfig {
            socket_path: Some("/tmp/gemini_suite_ida.sock".to_string()),
        };
    }
    
    // Try to load MCP servers config if exists
    let mcp_config_file = config_dir.join("mcp").join("servers.json");
    if mcp_config_file.exists() {
        println!("Loading existing MCP configuration from {:?}", mcp_config_file);
        let content = fs::read_to_string(&mcp_config_file)?;
        if let Ok(servers) = serde_json::from_str::<Vec<McpServerConfig>>(&content) {
            unified_config.mcp = McpConfig { servers };
        }
    } else {
        // Set default MCP config with standard servers
        let filesystem_server = McpServerConfig {
            name: "filesystem".to_string(),
            enabled: true,
            transport: "stdio".to_string(),
            command: vec!["/home/james/.local/bin/mcp-servers/filesystem-mcp".to_string()],
            args: vec![],
            env: {
                let mut env = HashMap::new();
                env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());
                env
            },
            auto_execute: vec![],
        };
        
        let command_server = McpServerConfig {
            name: "command".to_string(),
            enabled: true,
            transport: "stdio".to_string(),
            command: vec!["/home/james/.local/bin/mcp-servers/command-mcp".to_string()],
            args: vec![],
            env: {
                let mut env = HashMap::new();
                env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());
                env
            },
            auto_execute: vec![],
        };
        
        let memory_store_server = McpServerConfig {
            name: "memory_store".to_string(),
            enabled: true,
            transport: "stdio".to_string(),
            command: vec!["/home/james/.local/bin/mcp-servers/memory-store-mcp".to_string()],
            args: vec![],
            env: {
                let mut env = HashMap::new();
                env.insert("GEMINI_MCP_TIMEOUT".to_string(), "120".to_string());
                env
            },
            auto_execute: vec![
                "store_memory".to_string(),
                "list_all_memories".to_string(),
                "retrieve_memory_by_key".to_string(),
                "retrieve_memory_by_tag".to_string(),
                "delete_memory_by_key".to_string(),
            ],
        };
        
        unified_config.mcp = McpConfig {
            servers: vec![filesystem_server, command_server, memory_store_server],
        };
    }
    
    // Try to load Memory config if exists (create default if not)
    unified_config.memory = MemoryConfig {
        storage_path: Some(config_dir.join("memory").to_string_lossy().into_owned()),
        embedding_model: Some("gemini-2.0-flash".to_string()),
    };
    
    // Serialize and write the unified config
    let toml_content = toml::to_string_pretty(&unified_config)?;
    fs::write(&unified_config_path, toml_content)?;
    
    println!("Unified configuration file created at {:?}", unified_config_path);
    println!("You may need to modify it according to your needs.");
    
    Ok(())
} 