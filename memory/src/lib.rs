// This crate will contain persistent memory storage logic:
// - Memory store implementation
// - CRUD operations for memories
// - Memory item data structures

// Modules will be added in Phase 3

// The gemini-memory crate provides persistent memory storage logic for the Gemini Suite.
// It handles storing, retrieving, and managing memory items.

pub mod auto_memory;
pub mod broker;
mod memory;
mod store;

// Add declarations for the new modules
pub mod errors;
pub mod schema;
pub mod config;
pub mod arrow_conversion;

pub use memory::Memory;
pub use store::MemoryStore;
// Remove re-exports for store helper functions
// pub use store::{get_memory_store_path, ensure_memory_dir, load_memory_store, save_memory_store};
// Remove re-exports for broker functions
// pub use broker::{retrieve_all_memories, filter_relevant_memories, enhance_query, deduplicate_memories};
pub use broker::enhance_prompt;
// Remove re-exports for auto_memory functions
// pub use auto_memory::{extract_key_information, store_memories};

// Re-export error and common types
pub mod error {
    pub use std::error::Error;
}
