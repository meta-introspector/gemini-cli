use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::env;
use anyhow::{Result, Context, anyhow};
use gemini_core::config::{UnifiedConfig, get_unified_config_path};

/// Configuration for the IDA daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdaConfig {
    /// Path to the Unix socket for IPC communication with HAPPE
    pub ida_socket_path: PathBuf,
    
    /// Path to the memory database directory
    pub memory_db_path: PathBuf,
    
    /// Maximum number of memory items to return per query
    pub max_memory_results: usize,
    
    /// Semantic similarity threshold for memory retrieval
    pub semantic_similarity_threshold: f32,
    
    /// Configuration for the memory broker LLM
    #[serde(default)]
    pub memory_broker: MemoryBrokerConfig,
}

/// Configuration for the memory broker LLM
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryBrokerConfig {
    /// API key for the memory broker LLM
    pub api_key: Option<String>,
    
    /// Model to use for memory broker operations
    pub model_name: Option<String>,
}

impl Default for IdaConfig {
    fn default() -> Self {
        Self {
            ida_socket_path: PathBuf::from("/tmp/gemini_suite_ida.sock"),
            memory_db_path: PathBuf::from("memory/lance_db"),
            max_memory_results: 10,
            semantic_similarity_threshold: 0.7,
            memory_broker: MemoryBrokerConfig::default(),
        }
    }
}

impl IdaConfig {
    /// Attempt to load configuration from a file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(anyhow!("Configuration file does not exist: {}", path.display()));
        }
        
        // Check if this is the unified config
        if path.file_name().map_or(false, |f| f == "config.toml") {
            return Self::load_from_unified_config(path);
        }
        
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read configuration file: {}", path.display()))?;
        
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("Failed to parse configuration file: {}", path.display()))?;
        
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
    pub fn load_from_default() -> Result<Self> {
        // Always use unified config
        if let Ok(unified_path) = get_unified_config_path() {
            if unified_path.exists() {
                return Self::load_from_unified_config(&unified_path);
            }
        }
        
        // Return default config if no file found
        Ok(Self::default())
    }
    
    /// Load from unified configuration file
    pub fn load_from_unified_config(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(anyhow!("Unified configuration file does not exist: {}", path.display()));
        }
        
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read unified configuration file: {}", path.display()))?;
        
        // First try to parse the whole file as a toml Value to extract the ida section
        let mut config = Self::default();
        
        if let Ok(root) = content.parse::<toml::Value>() {
            if let Some(ida_table) = root.get("ida").and_then(|v| v.as_table()) {
                // Extract the ida section as its own TOML document
                let ida_section = toml::to_string(ida_table)
                    .unwrap_or_else(|_| String::new());
                if let Ok(ida_config) = toml::from_str::<IdaConfig>(&ida_section) {
                    config = ida_config;
                }
            }
            
            // Also check if there's a memory_broker section or memory.api_key to use for memory_broker.api_key
            if let Some(memory_table) = root.get("memory").and_then(|v| v.as_table()) {
                if config.memory_broker.api_key.is_none() {
                    if let Some(api_key) = memory_table.get("api_key").and_then(|v| v.as_str()) {
                        config.memory_broker.api_key = Some(api_key.to_string());
                    }
                }
            }
            
            // Check for gemini configuration to use as fallback for memory_broker
            if let Some(gemini_table) = root.get("gemini").and_then(|v| v.as_table()) {
                if config.memory_broker.api_key.is_none() {
                    if let Some(api_key) = gemini_table.get("api_key").and_then(|v| v.as_str()) {
                        config.memory_broker.api_key = Some(api_key.to_string());
                    }
                }
                
                if config.memory_broker.model_name.is_none() {
                    if let Some(model) = gemini_table.get("memory_broker_model").and_then(|v| v.as_str()) {
                        config.memory_broker.model_name = Some(model.to_string());
                    } else if let Some(model) = gemini_table.get("model_name").and_then(|v| v.as_str()) {
                        config.memory_broker.model_name = Some(model.to_string());
                    }
                }
            }
        }
        
        Ok(config)
    }
    
    /// Resolve the memory database path, converting relative paths to absolute
    pub fn resolve_memory_db_path(&self) -> Result<PathBuf> {
        let path = &self.memory_db_path;
        
        // If the path is already absolute, return it
        if path.is_absolute() {
            return Ok(path.clone());
        }
        
        // Try to resolve relative to config directory
        if let Some(config_dir) = Self::get_config_dir() {
            let resolved_path = config_dir.join(path);
            return Ok(resolved_path);
        }
        
        // If we can't resolve a config directory, try to resolve relative to home directory
        if let Some(home_dir) = dirs::home_dir() {
            let resolved_path = home_dir.join(".local/share/gemini-suite").join(path);
            return Ok(resolved_path);
        }
        
        // If all else fails, return the original path
        Err(anyhow!("Could not resolve memory database path: {}", path.display()))
    }
} 