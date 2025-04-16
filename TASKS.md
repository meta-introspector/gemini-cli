## Discovered TODOs

### Completed Tasks
- [x] `src/mcp/rpc.rs:167`: Add prompts if needed
- [x] `src/mcp/rpc.rs:168`: Add other capabilities like sampling support
- [x] `src/mcp/rpc.rs:178`: Add result schema?
- [x] `src/mcp/rpc.rs:186`: Add schema/type information?
- [x] `src/mcp/rpc.rs:236`: Define structs for other MCP methods like `mcp/resource/get`, notifications, etc.
- [x] `src/mcp/host/active_server.rs:171`: Add trace support?
- [x] `src/mcp/servers/command.rs:8`: Move basic JSON-RPC structures to shared rpc module
- [x] `src/mcp/servers/command.rs:10`: Move MCP server capabilities to shared rpc module
- [x] `src/mcp/servers/filesystem.rs:10`: Move basic JSON-RPC structures to shared rpc module
- [x] `src/mcp/servers/filesystem.rs:12`: Move MCP server capabilities to shared rpc module
- [x] `src/mcp/servers/filesystem.rs:308`: Implement recursive listing if needed
- [x] `src/mcp/host/message_handler.rs:39`: Handle server-initiated requests if MCP spec allows/requires (e.g., Sampling)
- [x] `src/mcp/host/message_handler.rs:190`: Implement handling for specific notifications if needed (e.g., $/progress, logMessage)
- [x] `src/mcp/host/message_handler.rs:209`: Implement request cancellation if needed
- [x] `src/mcp/config.rs:28`: Add SSE later if needed
- [x] `src/mcp/host/mod.rs:96`: Handle other transports

### Pending Tasks

#### Schema & State Management
- [ ] `src/mcp/host/mod.rs:159`: Recursively find $ref fields in the schema (JSON Value)
- [ ] `src/mcp/host/mod.rs:216`: Add more explicit state check (e.g., `server.status == Status::Ready`) 