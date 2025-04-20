use crate::errors::GeminiResult;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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
