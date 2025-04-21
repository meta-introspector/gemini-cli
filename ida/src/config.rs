use anyhow::{anyhow, Result};
use gemini_core::config::{
    IdaConfig as CoreIdaConfig, UnifiedConfig,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// The provider for the broker LLM ("gemini", "openai", "ollama", "anthropic", etc.)
    pub provider: Option<String>,
    /// API key for the memory broker LLM
    pub api_key: Option<String>,
    /// Model to use for memory broker operations
    pub model_name: Option<String>,
    /// Base URL (primarily for Ollama or other self-hosted endpoints)
    pub base_url: Option<String>,
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

impl From<&CoreIdaConfig> for IdaConfig {
    fn from(config: &CoreIdaConfig) -> Self {
        let mut ida_config = Self::default();

        // Copy fields from core IdaConfig to our IdaConfig
        if let Some(socket_path) = &config.ida_socket_path {
            ida_config.ida_socket_path = socket_path.clone();
        }

        if let Some(db_path) = &config.memory_db_path {
            ida_config.memory_db_path = db_path.clone();
        }

        if let Some(max_results) = config.max_memory_results {
            ida_config.max_memory_results = max_results;
        }

        if let Some(threshold) = config.semantic_similarity_threshold {
            ida_config.semantic_similarity_threshold = threshold;
        }

        // Convert memory broker config
        let memory_broker = &config.memory_broker;

        // Only overwrite if the fields are present in the core config
        if memory_broker.provider.is_some() {
            ida_config.memory_broker.provider = memory_broker.provider.clone();
        }

        if memory_broker.api_key.is_some() {
            ida_config.memory_broker.api_key = memory_broker.api_key.clone();
        }

        if memory_broker.model_name.is_some() {
            ida_config.memory_broker.model_name = memory_broker.model_name.clone();
        }

        if memory_broker.base_url.is_some() {
            ida_config.memory_broker.base_url = memory_broker.base_url.clone();
        }

        ida_config
    }
}

impl IdaConfig {
    /// Load configuration from the unified configuration
    pub fn load() -> Result<Self> {
        let unified_config = UnifiedConfig::load();
        Ok(Self::from(&unified_config.ida))
    }

    /// Resolve the memory database path, converting relative paths to absolute
    pub fn resolve_memory_db_path(&self) -> Result<PathBuf> {
        let path = &self.memory_db_path;

        // If the path is already absolute, return it
        if path.is_absolute() {
            return Ok(path.clone());
        }

        // Try to resolve relative to the unified config directory
        if let Ok(unified_config_path) = gemini_core::config::get_unified_config_path() {
            if let Some(config_dir) = unified_config_path.parent() {
                let resolved_path = config_dir.join(path);
                return Ok(resolved_path);
            }
        }

        // If we can't resolve the unified config directory, try to use home directory
        if let Some(home_dir) = dirs::home_dir() {
            let resolved_path = home_dir.join(".local/share/gemini-suite").join(path);
            return Ok(resolved_path);
        }

        // If all else fails, return the original path
        Err(anyhow!(
            "Could not resolve memory database path: {}",
            path.display()
        ))
    }
}
