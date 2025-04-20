use gemini_core::config::GeminiConfig;
use std::env;

/// Extension trait for GeminiConfig to add async memory configuration
pub trait AsyncMemoryConfigExt {
    /// Check if async memory operations are enabled
    fn async_memory_enabled(&self) -> bool;
    
    /// Get the configured queue size for async memory operations
    fn async_memory_queue_size(&self) -> Option<usize>;
    
    /// Get the configured worker count for async memory operations
    fn async_memory_worker_count(&self) -> Option<usize>;
}

// Implement the trait for GeminiConfig
impl AsyncMemoryConfigExt for GeminiConfig {
    fn async_memory_enabled(&self) -> bool {
        // Check environment variable first
        if let Ok(val) = env::var("GEMINI_ASYNC_MEMORY") {
            return val.to_lowercase() != "false" && val != "0";
        }
        
        // Default to true if not specified
        true
    }
    
    fn async_memory_queue_size(&self) -> Option<usize> {
        env::var("GEMINI_ASYNC_QUEUE_SIZE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
    }
    
    fn async_memory_worker_count(&self) -> Option<usize> {
        env::var("GEMINI_ASYNC_WORKERS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
    }
} 