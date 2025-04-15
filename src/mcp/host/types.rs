use crate::mcp::config::McpServerConfig;
use crate::mcp::rpc::JsonRpcError;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, oneshot};
use tokio::task;
use std::env;

// Constants used throughout the host module
pub const APP_NAME: &str = env!("CARGO_PKG_NAME");
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const STDIO_BUFFER_SIZE: usize = 8192; // For BufReader/BufWriter
pub const JSON_RPC_PARSE_BUFFER_SIZE: usize = 4096; // Initial buffer for parsing JSON-RPC messages
pub const CHANNEL_BUFFER_SIZE: usize = 32; // For MPSC channels

// Represents a pending request waiting for a response
#[derive(Debug)]
pub struct PendingRequest {
    pub responder: oneshot::Sender<Result<Value, JsonRpcError>>,
    pub method: String, // For debugging/logging
}

// Represents an active, running MCP server process
#[derive(Debug)]
pub struct ActiveServer {
    pub config: McpServerConfig,
    pub process: Arc<Mutex<tokio::process::Child>>, // Fix the Child type to be tokio::process::Child
    pub stdin_tx: mpsc::Sender<String>, // Channel to send JSON-RPC messages (as strings) to the stdin writer task
    pub capabilities: Arc<Mutex<Option<crate::mcp::rpc::ServerCapabilities>>>, // Capabilities received from server
    pub pending_requests: Arc<Mutex<std::collections::HashMap<u64, PendingRequest>>>, // Track pending requests by ID
    // Track task handles for proper shutdown
    pub reader_task: Arc<Mutex<Option<task::JoinHandle<()>>>>,
    pub writer_task: Arc<Mutex<Option<task::JoinHandle<()>>>>,
    pub stderr_task: Arc<Mutex<Option<task::JoinHandle<()>>>>,
    pub shutdown_signal: Arc<Mutex<Option<oneshot::Sender<()>>>>, // To signal reader/writer tasks to stop
}

// Clone implementation needed for passing ActiveServer around (due to Arcs)
impl Clone for ActiveServer {
    fn clone(&self) -> Self {
        ActiveServer {
            config: self.config.clone(),
            process: self.process.clone(),
            stdin_tx: self.stdin_tx.clone(),
            capabilities: self.capabilities.clone(),
            pending_requests: self.pending_requests.clone(),
            reader_task: self.reader_task.clone(),
            writer_task: self.writer_task.clone(),
            stderr_task: self.stderr_task.clone(),
            shutdown_signal: self.shutdown_signal.clone(),
        }
    }
} 