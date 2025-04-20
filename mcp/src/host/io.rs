// The IO module handles the low-level transport details for MCP servers

// Note: IO functionality is now handled directly in types.rs
// - BufReader and BufWriter are used for stdin/stdout communication
// - Message formatting follows JSON-RPC over stdio protocol
// - Content-Length headers are used to frame messages
