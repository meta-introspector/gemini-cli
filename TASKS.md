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

## Gemini Suite Workspace Conversion

### Completed Tasks
- [x] Create workspace directory structure (cli, core, mcp, memory)
- [x] Create root Cargo.toml with workspace configuration
- [x] Set up core/Cargo.toml with dependencies
- [x] Set up mcp/Cargo.toml with dependencies
- [x] Set up memory/Cargo.toml with dependencies
- [x] Create placeholder lib.rs files for the library crates
- [x] Update cli/Cargo.toml with commented references to the new library crates
- [x] Clean up unused directory structure
- [x] Verify workspace setup with cargo check
- [x] Extract shared error types to core/src/errors.rs
- [x] Extract Gemini API data structures to core/src/types.rs
- [x] Extract configuration loading to core/src/config.rs
- [x] Create Gemini API client in core/src/client.rs
- [x] Update cli/Cargo.toml to use gemini-core
- [x] Successfully build workspace with Phase 2 changes
- [x] Extract memory logic to gemini-memory crate (Phase 3)
- [x] Extract MCP/Tool logic to gemini-mcp crate (Phase 4)

### Pending Tasks
- [ ] Final cleanup and verification (Phase 5)

## Active Task
- [ ] Final Cleanup & Verification (Phase 5)

## Pending Tasks
- [ ] Add basic unit/integration tests (Phase 5, Sub-task)
- [ ] Comprehensive testing of gemini-cli (Phase 5, Sub-task)
- [ ] **CLI Crate Cleanup (Phase 5, Sub-tasks):**
  - [ ] Delete obsolete module files/directories in cli/src (e.g., remnants of config.rs, mcp/)
  - [ ] Remove commented-out mod/use statements in cli/src/main.rs referencing moved code
  - [ ] Refactor/remove handle_config_flags in cli/src/main.rs to use core libraries appropriately
  - [ ] Remove deprecated MCP server direct launch logic in cli/src/main.rs
  - [ ] Review cli/src/app.rs and other modules for outdated dependencies or logic

## Completed Tasks
- [x] Preparation & Workspace Setup (Phase 1)
- [x] Extract Core Gemini Logic (`gemini-core`) (Phase 2)
- [x] Extract Memory Logic (`gemini-memory`) (Phase 3)
- [x] Extract MCP/Tool logic to gemini-mcp crate (Phase 4)

# TASKS: MCP Host Daemon Implementation

This document outlines the tasks required to implement the MCP Host Daemon based on the plan in `PLANNING.md`.

## Phase 1: Daemon Foundation (within `gemini-mcp` Crate)

- [x] **Task 1.1:** Add new binary target `mcp-hostd` to `gemini-mcp/Cargo.toml`.
    - [x] Create the corresponding source file (e.g., `mcp/src/bin/mcp-hostd.rs`).
- [x] **Task 1.2:** Implement basic daemon main function in `mcp/src/bin/mcp-hostd.rs`:
    - [x] Add necessary dependencies (e.g., `gemini_mcp`, `tokio`, logging crates).
    - [x] Initialize logging.
    - [x] Load server configurations (using `gemini_mcp::config`).
    - [x] Instantiate `gemini_mcp::host::McpHost` (using the library component).
    - [x] Keep the process alive (e.g., simple loop or park thread for now).
- [x] **Task 1.3:** Implement graceful shutdown trigger in daemon binary:
    - [x] Add signal handling (SIGINT, SIGTERM) using `tokio::signal`.
    - [x] Call `McpHost::shutdown()` upon receiving a shutdown signal.

## Phase 2: Inter-Process Communication (IPC)

- [ ] **Task 2.1:** Define IPC Protocol:
    - [ ] Finalize message format (e.g., `u32` length prefix + JSON payload).
    - [ ] Define structure of JSON requests from CLI to Daemon (e.g., `{"type": "execute_tool", "server": "...", "tool": "...", "args": {...}}`, `{"type": "get_capabilities"}`).
    - [ ] Define structure of JSON responses from Daemon to CLI (e.g., `{"status": "success", "result": {...}}` or `{"status": "error", "message": "..."}`).
- [ ] **Task 2.2:** Implement Daemon IPC Listener (in `mcp-hostd` binary):
    - [ ] Add dependency for Unix Domain Sockets (`tokio::net`).
    - [ ] Create and bind `UnixListener` to a standard path (e.g., `~/.local/share/gemini-cli/mcp-hostd.sock`). Ensure proper cleanup of the socket file on exit/restart.
    - [ ] Accept incoming connections in a loop.
    - [ ] Spawn a `tokio` task for each accepted client connection.
- [x] **Task 2.3:** Implement Daemon IPC Message Handling (per-client task in `mcp-hostd` binary):
    - [x] Implement length-prefixed message reading from the socket.
    - [x] Implement JSON deserialization for incoming requests.
    - [x] Implement logic to dispatch requests to the instantiated `McpHost` methods (`get_all_capabilities`, `execute_tool`, etc.).
    - [x] Implement JSON serialization for responses.
    - [x] Implement length-prefixed message writing to the socket.
    - [x] Handle client disconnection gracefully.

## Phase 3: Client Implementation (`gemini-cli` Modification)

- [x] **Task 3.1:** Implement CLI IPC Client Logic:
  - [x] Add logic to connect to the daemon's `UnixStream` at the standard path.
  - [x] Implement error handling for connection failure (e.g., if daemon is not running).
  - [x] Implement length-prefixed message writing (for sending requests).
  - [x] Implement length-prefixed message reading (for receiving responses).
  - [x] Implement JSON serialization/deserialization for requests/responses.
- [x] **Task 3.2:** Refactor `gemini-cli` Core:
  - [x] Remove the direct instantiation and usage of the internal `McpHost` (in favor of McpProvider).
  - [x] Replace calls to `mcp_host.get_all_capabilities()` with an IPC request to the daemon (via helper).
  - [x] Replace calls to `mcp_host.execute_tool()` with an IPC request to the daemon (via helper).
  - [x] Update how capabilities are stored/accessed within the CLI context (uses helper).
  - [/] Ensure consent flow still functions correctly (now uses McpProvider in utils).
  - [ ] Refine MemoryStore integration when using daemon IPC.

## Phase 3.5: MemoryStore IPC Integration

- [x] **Task 3.5.1:** Analyze `McpHostInterface` (`memory/src/broker.rs`)
    - [x] Determine the precise methods and data types required by `MemoryStore` for embedding generation and any other critical functions.
    - [x] Clarify the purpose of the `send_request` method in the trait.
- [x] **Task 3.5.2:** Design IPC Extensions (`mcp/src/ipc.rs`)
    - [x] Define new `DaemonRequest` enum variants (e.g., `GenerateEmbedding { text: String, model: String }`).
    - [x] Define corresponding `DaemonResult` variants (e.g., `Embedding(Vec<f32>)`).
    - [x] Consider adding variants for `GetBrokerCapabilities` if the specific `broker::Capabilities` struct is needed, or adapt client/trait.
- [x] **Task 3.5.3:** Implement Daemon Handlers (`mcp/src/bin/mcp-hostd.rs`)
    - [x] Add match arms in `process_request` for new `DaemonRequest` variants.
    - [x] Implement logic to handle `GenerateEmbedding` (likely calling a new/existing method on `McpHost` instance).
    - [x] Implement handlers for other necessary requests (e.g., `GetBrokerCapabilities` if added).
- [x] **Task 3.5.4:** (Optional) Extend `McpHost` (`mcp/src/host/mod.rs`)
    - [x] No changes needed to McpHost, able to use existing method (execute_tool).
- [x] **Task 3.5.5:** Implement Client Methods (`cli/src/ipc_client.rs`)
    - [x] Add public async methods to `McpDaemonClient` (e.g., `generate_embedding`).
    - [x] Implement logic to send the new `DaemonRequest` variants and parse the corresponding `DaemonResult` variants.
- [x] **Task 3.5.6:** Implement `McpHostInterface` for `McpDaemonClient` (`cli/src/ipc_client.rs`)
    - [x] Add `impl gemini_memory::broker::McpHostInterface for McpDaemonClient`.
    - [x] Implement all required trait methods using the client's public methods.
    - [x] Address any error type mismatches (e.g., map errors in `get_capabilities`).
    - [x] Decide how to handle `send_request` if its purpose remains unclear or unsupported via IPC (e.g., return error, no-op).
- [x] **Task 3.5.7:** Update CLI Initialization (`cli/src/main.rs`)
    - [x] Remove the conditional compilation (`if mcp_client.is_none()`) around `MemoryStore::new`.
    - [x] Ensure `McpDaemonClient` is correctly passed to `MemoryStore::new` via the `Arc`. 
- [ ] **Task 3.5.8:** Testing
    - [ ] Add integration tests verifying `MemoryStore` embedding and retrieval work correctly when `gemini-cli` uses the `mcp-hostd` daemon.

## Phase 4: Testing and Documentation

- [ ] **Task 4.1:** Unit Testing:
    - [ ] Test IPC message serialization/deserialization.
    - [ ] Test daemon request dispatch logic.
- [ ] **Task 4.2:** Integration Testing:
    - [ ] Test daemon startup and server initialization (building and running the `mcp-hostd` binary).
    - [ ] Test CLI connecting to the daemon.
    - [ ] Test sending various commands (`get_capabilities`, `execute_tool`) from CLI through daemon to servers and back.
    - [ ] Test daemon graceful shutdown.
    - [ ] Test behavior when daemon is not running.
    - [ ] Test handling of individual server failures within the persistent daemon.
    - [ ] (Moved to 3.5.8) Test MemoryStore functionality via daemon.
- [ ] **Task 4.3:** Documentation:
    - [ ] Update project README or add new docs explaining the daemon architecture.
    - [ ] Document how to build, run, and manage the `mcp-hostd` daemon binary.
    - [ ] Document the IPC protocol details (including MemoryStore additions).
    - [ ] Document configuration paths (socket, server configs).

## Phase 5: Packaging and Deployment (Optional)

- [ ] **Task 5.1:** Create systemd service file for `mcp-hostd`.
- [ ] **Task 5.2:** Update build/installation scripts to include the `mcp-hostd` binary. 