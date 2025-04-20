// This crate will contain the core Gemini API functionality:
// - API client for Gemini
// - Request/response data structures
// - Context management
// - Configuration loading
// - Shared error types

// Modules will be added in Phase 2

// Core Gemini API functionality

// Export client module - API client for Gemini
pub mod client;
pub use client::*;

// Export types module - Request/response data structures
pub mod types;
pub use types::*;

// Export config module - Configuration loading
pub mod config;
pub use config::*;

// Export errors module - Shared error types
pub mod errors;
pub use errors::*;

// Export shared RPC types
pub mod rpc_types;
// pub use rpc_types::*; // Replace glob export
pub use rpc_types::{
    JsonRpcError, Request, Resource, Response, ServerCapabilities, Tool as RpcTool,
};
