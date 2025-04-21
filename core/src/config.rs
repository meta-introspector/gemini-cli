use crate::errors::GeminiResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Unified configuration structure for the entire Gemini Suite
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UnifiedConfig {
    /// Gemini API configuration
    #[serde(default)]
    pub gemini: GeminiConfig,
    
    /// HAPPE daemon configuration
    #[serde(default)]
    pub happe: Option<HappeConfig>,
    
    /// IDA daemon configuration
    #[serde(default)]
    pub ida: Option<IdaConfig>,
    
    /// Memory configuration
    #[serde(default)]
    pub memory: Option<MemoryConfig>,
}

impl UnifiedConfig {
    /// Loads the unified configuration from the default location
    pub fn load() -> GeminiResult<Self> {
        let config_path = get_unified_config_path()?;
        Self::load_from_file(&config_path)
    }
    
    /// Loads configuration from a file if it exists, otherwise returns the default config
    pub fn load_from_file(path: &Path) -> GeminiResult<Self> {
        if path.exists() {
            let content = fs::read_to_string(path).map_err(|e| {
                crate::errors::GeminiError::ConfigError(format!(
                    "Failed to read unified config file: {}",
                    e
                ))
            })?;

            let config: Self = toml::from_str(&content).map_err(|e| {
                crate::errors::GeminiError::ConfigError(format!(
                    "Failed to parse unified config file: {}",
                    e
                ))
            })?;

            Ok(config)
        } else {
            Ok(Self::default())
        }
    }
    
    /// Saves configuration to a file
    pub fn save_to_file(&self, path: &Path) -> GeminiResult<()> {
        let content = toml::to_string(self).map_err(|e| {
            crate::errors::GeminiError::ConfigError(format!("Failed to serialize unified config: {}", e))
        })?;

        // Ensure the directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                crate::errors::GeminiError::ConfigError(format!(
                    "Failed to create config directory: {}",
                    e
                ))
            })?;
        }

        fs::write(path, content).map_err(|e| {
            crate::errors::GeminiError::ConfigError(format!("Failed to write unified config file: {}", e))
        })?;

        Ok(())
    }
}

/// Get the path to the unified configuration file
pub fn get_unified_config_path() -> GeminiResult<PathBuf> {
    let home_dir = dirs::home_dir().ok_or_else(|| {
        crate::errors::GeminiError::ConfigError("Could not determine home directory".to_string())
    })?;

    let config_dir = home_dir.join(".config").join("gemini-suite");
    Ok(config_dir.join("config.toml"))
}

/// Configuration struct for Gemini API
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GeminiConfig {
    pub api_key: Option<String>,
    pub system_prompt: Option<String>,
    pub model_name: Option<String>,
    pub save_history: Option<bool>,
    pub enable_memory_broker: Option<bool>,
    pub enable_auto_memory: Option<bool>,
    pub memory_broker_model: Option<String>,
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            system_prompt: Some(
                "You are a helpful assistant. Answer the user's questions concisely and accurately."
                .to_string()
            ),
            model_name: Some("gemini-2.5-pro-preview-03-25".to_string()),
            save_history: Some(true),
            enable_memory_broker: Some(true),
            enable_auto_memory: Some(true),
            memory_broker_model: Some("gemini-2.0-flash".to_string()),
        }
    }
}

impl GeminiConfig {
    /// Creates a new configuration
    pub fn new(
        api_key: Option<String>,
        system_prompt: Option<String>,
        model_name: Option<String>,
        save_history: Option<bool>,
        enable_memory_broker: Option<bool>,
        enable_auto_memory: Option<bool>,
        memory_broker_model: Option<String>,
    ) -> Self {
        Self {
            api_key,
            system_prompt,
            model_name,
            save_history,
            enable_memory_broker,
            enable_auto_memory,
            memory_broker_model,
        }
    }

    /// Loads configuration from a file if it exists, otherwise returns the default config
    pub fn load_from_file(path: &Path) -> GeminiResult<Self> {
        if path.exists() {
            let content = fs::read_to_string(path).map_err(|e| {
                crate::errors::GeminiError::ConfigError(format!(
                    "Failed to read config file: {}",
                    e
                ))
            })?;

            let config: Self = toml::from_str(&content).map_err(|e| {
                crate::errors::GeminiError::ConfigError(format!(
                    "Failed to parse config file: {}",
                    e
                ))
            })?;

            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    /// Saves configuration to a file
    pub fn save_to_file(&self, path: &Path) -> GeminiResult<()> {
        let content = toml::to_string(self).map_err(|e| {
            crate::errors::GeminiError::ConfigError(format!("Failed to serialize config: {}", e))
        })?;

        // Ensure the directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                crate::errors::GeminiError::ConfigError(format!(
                    "Failed to create config directory: {}",
                    e
                ))
            })?;
        }

        fs::write(path, content).map_err(|e| {
            crate::errors::GeminiError::ConfigError(format!("Failed to write config file: {}", e))
        })?;

        Ok(())
    }

    /// Merges this config with another config, preferring values from the other config if present
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            api_key: other.api_key.clone().or_else(|| self.api_key.clone()),
            system_prompt: other
                .system_prompt
                .clone()
                .or_else(|| self.system_prompt.clone()),
            model_name: other.model_name.clone().or_else(|| self.model_name.clone()),
            save_history: other.save_history.or(self.save_history),
            enable_memory_broker: other.enable_memory_broker.or(self.enable_memory_broker),
            enable_auto_memory: other.enable_auto_memory.or(self.enable_auto_memory),
            memory_broker_model: other
                .memory_broker_model
                .clone()
                .or_else(|| self.memory_broker_model.clone()),
        }
    }
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
    
    /// System prompt for HAPPE interactions
    pub system_prompt: Option<String>,
}

/// IDA daemon configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct IdaConfig {
    /// Socket path
    pub ida_socket_path: Option<String>,
    
    /// Path to the memory database
    pub memory_db_path: Option<String>,
    
    /// Maximum number of memory results to return per query
    pub max_memory_results: Option<usize>,
    
    /// Semantic similarity threshold
    pub semantic_similarity_threshold: Option<f32>,
}

/// Memory configuration
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MemoryConfig {
    pub storage_path: Option<String>,
    pub embedding_model: Option<String>,
}

/// Helper function to get default config directory
pub fn get_default_config_dir(app_name: &str) -> GeminiResult<PathBuf> {
    let home_dir = dirs::home_dir().ok_or_else(|| {
        crate::errors::GeminiError::ConfigError("Could not determine home directory".to_string())
    })?;

    let config_dir = home_dir.join(".config").join(app_name);

    Ok(config_dir)
}

/// Helper function to get default config file path
pub fn get_default_config_file(app_name: &str) -> GeminiResult<PathBuf> {
    let config_dir = get_default_config_dir(app_name)?;
    Ok(config_dir.join("config.toml"))
}
