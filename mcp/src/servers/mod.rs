// This module will contain server implementation utilities
// Note: The actual server implementations are standalone crates in subdirectories:
// - src/mcp/servers/command/
// - src/mcp/servers/filesystem/
// - src/mcp/servers/memory/

// These are built and run separately, not imported as modules 

pub mod filesystem; 
// pub mod memory; // Removed old declaration
pub mod command; 
pub mod memory_store; // Added new declaration 