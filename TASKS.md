# Implementation Task List: IDA, HAPPE, and IPC

This task list breaks down the implementation steps outlined in `PLANNING.md`.

## Phase 1: Define Shared IPC Communication (`@ipc` Crate)

- [x] **Module Setup:**
    - [x] Create directory `ipc/src/internal_messages/`.
    - [x] Create file `ipc/src/internal_messages/mod.rs`.
    - [x] Create file `ipc/src/internal_messages/types.rs`.
    - [x] Declare `internal_messages` module in `ipc/src/lib.rs` (`pub mod internal_messages;`).
- [x] **Define Structs:**
    - [x] Define `MemoryItem` struct in `internal_messages/types.rs` (e.g., with fields for content, source, timestamp, embedding ID).
    - [x] Define `ConversationTurn` struct in `internal_messages/types.rs` (e.g., with fields for query, used memories, response).
    - [x] Add `#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]` to `MemoryItem` and `ConversationTurn`.
- [x] **Define Enum:**
    - [x] Define `InternalMessage` enum in `internal_messages/mod.rs`.
    - [x] Add variant `GetMemoriesRequest { query: String }`.
    - [x] Add variant `GetMemoriesResponse { memories: Vec<MemoryItem> }`.
    - [x] Add variant `StoreTurnRequest { turn_data: ConversationTurn }`.
    - [x] Add `#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]` to `InternalMessage`.
    - [x] Re-export types from `types.rs` in `mod.rs` (`pub use types::*;`).
- [x] **Dependencies:**
    - [x] Ensure `serde` (with `derive` feature) is listed under `[dependencies]` in `ipc/Cargo.toml`.
    - [x] Ensure `gemini_core` is added if shared types are used.
- [x] **Verification:**
    - [x] Run `cargo check -p gemini-ipc` to ensure the crate compiles.

## Phase 2: Implement IDA Daemon (`@ida` Crate)

- [x] **Binary Skeleton:**
    - [x] Create `ida/src/bin/ida-daemon.rs`.
    - [x] Add `tokio::main` function.
    - [x] Set up basic logging (e.g., `tracing_subscriber::fmt::init();`).
    - [x] Add basic configuration loading (e.g., using `figment` or `clap`) for IPC path and placeholder MCP server address.
- [x] **IPC Server (`ida/src/ipc_server.rs`):**
    - [x] Create `run_server` async function accepting config.
    - [x] Implement IPC listener (e.g., `tokio::net::UnixListener::bind`).
    - [x] Add loop to accept incoming connections (`listener.accept().await`).
    - [x] Spawn a handler task for each connection (`tokio::spawn(handle_connection(...))`).
- [x] **Connection Handler (`ida/src/ipc_server.rs`):**
    - [x] Create `handle_connection` async function accepting the connection stream.
    - [x] Add loop to read data from the stream.
    - [x] Implement deserialization logic (e.g., using `serde_json::from_slice` or a framed transport) for `InternalMessage`.
    - [x] Add `match` statement for `InternalMessage` variants:
        - [x] `GetMemoriesRequest`: Log reception, call placeholder `retrieve_memories`, construct `GetMemoriesResponse`, serialize, send response back.
        - [x] `StoreTurnRequest`: Log reception, spawn background task (`tokio::spawn(handle_storage(...))`), **do not** send response.
    - [x] Add error handling for read/deserialization/write operations.
- [x] **Background Storage Task (`ida/src/storage.rs`):**
    - [x] Create `handle_storage` async function accepting `ConversationTurn`.
    - [x] Implement placeholder logic: log analysis, call placeholder `check_duplicates`, call placeholder `store_memory`.
    - [x] Add robust error handling and logging.
- [x] **MCP Client Placeholder (`ida/src/memory_mcp_client.rs`):**
    - [x] Create module.
    - [x] Define placeholder async functions: `retrieve_memories(query: &str) -> Result<Vec<MemoryItem>, Error>`, `check_duplicates(...) -> Result<bool, Error>`, `store_memory(...) -> Result<(), Error>`.
    - [x] Implement basic logging within placeholder functions.
- [x] **Dependencies (`ida/Cargo.toml`):**
    - [x] Add `tokio` (with `full` features).
    - [x] Add `ipc` crate (`{ path = "../ipc" }`).
    - [x] Add `serde`, `serde_json`.
    - [x] Add chosen IPC library dependencies (if not using `tokio::net`).
    - [x] Add configuration library (`figment`, `clap`).
    - [x] Add logging library (`tracing`, `tracing_subscriber`).
    - [x] Add error handling library (e.g., `thiserror`, `anyhow`).
    - [x] Add `chrono` (with `serde` feature).
    - [x] Add `thiserror`.
- [x] **Verification:**
    - [x] Run `cargo check -p ida --bin ida-daemon`.
    - [ ] Run `cargo build -p ida --bin ida-daemon`.

## Phase 3: Implement HAPPE Daemon (`@happe` Crate)

- [ ] **Binary Skeleton:**
    - [ ] Create `happe/src/bin/happe-daemon.rs`.
    - [ ] Add `tokio::main` function.
    - [ ] Set up basic logging.
    - [ ] Add basic configuration loading for IDA IPC path, placeholder LLM details, placeholder MCP details.
- [ ] **IDA IPC Client (`happe/src/ida_client.rs`):**
    - [ ] Create `connect` async function to establish IPC stream (e.g., `tokio::net::UnixStream::connect`) with retries.
    - [ ] Create `get_memories` async function: serialize `GetMemoriesRequest`, send, receive, deserialize `GetMemoriesResponse`.
    - [ ] Create `store_turn_async` async function: serialize `StoreTurnRequest`, send (fire-and-forget).
    - [ ] Add robust error handling for connection and communication.
    - [ ] Consider a struct `IdaClient` to manage the connection state.
- [ ] **Core Interaction Loop (`happe/src/main.rs` or `happe/src/coordinator.rs`):**
    - [ ] Implement placeholder for receiving user queries (e.g., read from stdin in a loop).
    - [ ] Inside the loop:
        - [ ] Establish/get connection to IDA using `ida_client::connect`.
        - [ ] Call `ida_client::get_memories`.
        - [ ] Construct prompt (placeholder: combine query + memories).
        - [ ] Call placeholder `llm_client::generate_response`.
        - [ ] Handle placeholder MCP tool calls (if simulated response requires it).
        - [ ] Get final LLM response (placeholder).
        - [ ] Send response to user (placeholder: print to stdout).
        - [ ] Gather `ConversationTurn` data.
        - [ ] Call `ida_client::store_turn_async`.
    - [ ] Add error handling for each step.
- [ ] **LLM Client Placeholder (`happe/src/llm_client.rs`):**
    - [ ] Create module.
    - [ ] Define placeholder async function `generate_response(prompt: &str) -> Result<String, Error>`.
    - [ ] Simulate basic LLM interaction (e.g., echo prompt, return canned response).
- [ ] **MCP Client Placeholder (`happe/src/mcp_client.rs` - Optional):**
    - [ ] Create module if simulating LLM tool calls.
    - [ ] Define placeholder functions for expected tools.
- [ ] **Dependencies (`happe/Cargo.toml`):**
    - [ ] Add `tokio` (with `full` features).
    - [ ] Add `ipc` crate (`{ path = "../ipc" }`).
    - [ ] Add `serde`, `serde_json`.
    - [ ] Add chosen IPC library dependencies (if not using `tokio::net`).
    - [ ] Add configuration library.
    - [ ] Add logging library.
    - [ ] Add error handling library.
- [ ] **Verification:**
    - [ ] Run `cargo check -p happe --bin happe-daemon`.
    - [ ] Run `cargo build -p happe --bin happe-daemon`.

## Phase 4: Integration and Basic Testing

- [ ] **Build Workspace:**
    - [ ] Run `cargo build` from the workspace root.
- [ ] **Direct IDA Test (Optional but Recommended):**
    - [ ] Start `ida-daemon` in one terminal.
    - [ ] Use `socat` or a simple script to send a serialized `GetMemoriesRequest` to the IDA IPC socket.
    - [ ] Verify `ida-daemon` logs the request and sends back a serialized `GetMemoriesResponse`.
    - [ ] Send a serialized `StoreTurnRequest`.
    - [ ] Verify `ida-daemon` logs the request and logs placeholder storage actions.
- [ ] **Concurrent Daemon Test:**
    - [ ] Start `target/debug/ida-daemon` in one terminal.
    - [ ] Start `target/debug/happe-daemon` in another terminal.
- [ ] **Interaction Flow Test:**
    - [ ] Provide input (query) to `happe-daemon` (e.g., via stdin).
    - [ ] **Verify HAPPE Logs:**
        - [ ] Connection to IDA established.
        - [ ] `GetMemoriesRequest` sent.
        - [ ] `GetMemoriesResponse` received (with placeholder data).
        - [ ] Placeholder prompt construction.
        - [ ] Placeholder LLM call initiated.
        - [ ] Placeholder LLM response received.
        - [ ] Response sent to user (stdout).
        - [ ] `StoreTurnRequest` sent.
    - [ ] **Verify IDA Logs:**
        - [ ] Connection from HAPPE accepted.
        - [ ] `GetMemoriesRequest` received.
        - [ ] Placeholder memory retrieval logged.
        - [ ] `GetMemoriesResponse` sent.
        - [ ] `StoreTurnRequest` received.
        - [ ] Placeholder storage actions logged.
- [ ] **Repeat:** Test with a few different inputs.

## General Tasks (Ongoing)

- [ ] **Code Formatting:** Run `cargo fmt` regularly across the workspace.
- [ ] **Linting:** Run `cargo clippy --all-targets --all-features` regularly and address warnings.
- [ ] **Unit Tests:** Add unit tests for specific functions with complex logic (e.g., message serialization/deserialization helpers, complex state logic if any).
- [ ] **Documentation:** Add doc comments (`///`) for public functions, structs, and enums, especially in `@ipc`.
- [ ] **Error Handling:** Ensure errors are propagated or handled gracefully (avoid `.unwrap()`/`.expect()` in production code).
- [ ] **Configuration:** Refine configuration loading (e.g., use environment variables, config files).
- [ ] **Logging:** Improve logging messages for clarity and debugging.
- [ ] **Version Control:** Commit changes frequently with clear messages.
