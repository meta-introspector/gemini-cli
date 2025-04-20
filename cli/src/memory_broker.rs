use gemini_memory::MemoryStore;
use log::info;

/// A memory broker that optionally processes memory operations asynchronously.
/// Currently just passes operations through to the underlying store.
pub struct MemoryBroker {
    store: MemoryStore,
    async_enabled: bool,
}

impl MemoryBroker {
    /// Create a new memory broker with the given MemoryStore
    pub fn new(store: MemoryStore, async_enabled: bool) -> Self {
        if async_enabled {
            info!("Async memory enabled (future feature, currently using synchronous operations)");
        } else {
            info!("Using synchronous memory operations");
        }
        
        Self {
            store,
            async_enabled,
        }
    }
    
    /// Access underlying MemoryStore
    pub fn store(&self) -> &MemoryStore {
        &self.store
    }
    
    /// Check if async mode is enabled
    pub fn is_async_enabled(&self) -> bool {
        self.async_enabled
    }
} 