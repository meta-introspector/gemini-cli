# Implementation Task List: IDA, HAPPE, and IPC

This task list breaks down the implementation steps outlined in `PLANNING.md`.

## Phase 1: Define Shared IPC Communication (`@ipc` Crate)

- [x] **Module Setup:**
    - [x] Create directory `ipc/src/internal_messages/`.
    - [x] Create file `ipc/src/internal_messages/mod.rs`.
    - [x] Create file `ipc/src/internal_messages/types.rs`.
    - [x] Declare `internal_messages` module in `ipc/src/lib.rs` (`pub mod internal_messages;`).
- [x] **Define Structs & Enums (HAPPE <-> IDA):**
    - [x] Define `MemoryItem`, `ConversationTurn` in `internal_messages/types.rs`.
    - [x] Define `InternalMessage` enum (`GetMemoriesRequest`, `GetMemoriesResponse`, `StoreTurnRequest`) in `internal_messages/mod.rs`.
    - [x] Add `#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]` to all.
    - [x] Re-export types from `types.rs` in `mod.rs`.
- [ ] **Define Structs & Enums (Client <-> HAPPE):**
    - [ ] Create `ipc/src/happe_request/mod.rs` and `types.rs`.
    - [ ] Define `HappeQueryRequest { query: String }` struct.
    - [ ] Define `HappeQueryResponse { response: String, error: Option<String> }` struct.
    - [ ] Add `#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]` to all.
    - [ ] Add `happe_request` module to `ipc/src/lib.rs`.
- [x] **Dependencies:**
    - [x] Ensure `serde` (with `derive` feature) is listed under `[dependencies]` in `ipc/Cargo.toml`.
    - [x] Ensure `gemini_core` is added if shared types are used.
- [x] **Verification:**
    - [x] Run `cargo check -p gemini-ipc` to ensure the crate compiles.
- **Milestone:** All IPC message definitions complete and build successfully.

## Phase 2: Implement IDA Daemon Skeleton (`@ida` Crate)

- [x] **Binary Skeleton & Config:** (Basic setup)
- [x] **IPC Server (`ida/src/ipc_server.rs`):** (Listens for HAPPE)
- [x] **Connection Handler (`ida/src/ipc_server.rs`):** (Handles `InternalMessage`)
- [x] **Background Storage Task (`ida/src/storage.rs`):** (Placeholder logic)
- [x] **MCP Client Placeholder (`ida/src/memory_mcp_client.rs`):** (Placeholder functions)
- [x] **Dependencies (`ida/Cargo.toml`):** (Initial dependencies)
- [x] **Verification:** (Basic build)
- **Milestone:** `ida-daemon` builds and runs, successfully accepting IPC connections from HAPPE and processing known message types (using placeholders for external calls).

## Phase 3: Implement HAPPE Daemon Skeleton (`@happe` Crate)

- [x] **Binary Skeleton & Config:** (Basic setup)
- [x] **IDA IPC Client (`happe/src/ida_client.rs`):** (Connects to IDA)
- [x] **Core Interaction Loop (`happe/src/coordinator.rs`):** (Placeholder loop calling IDA client and placeholder LLM/MCP)
- [x] **LLM Client Placeholder (`happe/src/llm_client.rs`):**
- [x] **MCP Client Placeholder (`happe/src/mcp_client.rs`):**
- [x] **Dependencies (`happe/Cargo.toml`):** (Initial dependencies)
- [x] **Verification:** (Basic build)
- **Milestone:** `happe-daemon` builds and runs, successfully connecting to IDA, sending requests, receiving responses, and sending async notifications (using placeholders for external calls).

## Phase 4: Initial Integration Test (HAPPE <-> IDA Placeholders)

- [x] **Build Workspace:**
- [x] **Direct IDA Test (Optional):**
- [x] **Concurrent Daemon Test:**
- [x] **Interaction Flow Test (via stdin):** (Verify logs for HAPPE <-> IDA placeholder flow)
- [x] **Repeat:**
- **Milestone:** Successful execution of the basic HAPPE <-> IDA communication flow verified via logs using placeholder logic.

## Phase 5: Implement HAPPE Core Logic & API (`@happe` Crate)

- [x] **Core Interaction Implementation (`happe/src/coordinator.rs`):**
    - [x] Define core `process_query(config: &AppConfig, mcp_provider: &mcp::host::McpHost, gemini_client: &core::client::GeminiClient, query: String) -> Result<String, Error>` function.
        - *Adapt structure from `cli/src/app.rs::process_prompt`.*
    - [x] Implement real prompt construction (using `gemini_core::prompt`, memories from IDA, MCP info).
        - *Reference `cli/src/app.rs` lines ~107-121, ~115 for structure.*
        - *Use `mcp::gemini::build_mcp_system_prompt`.*
    - [x] Integrate real `llm_client::generate_response` call (using implementation from below).
    - [x] Implement logic to handle LLM function calls.
        - *Use `mcp_provider` to get capabilities (`core::rpc_types::ServerCapabilities`).*
        - *Use `mcp::gemini::generate_gemini_function_declarations` to create `core::types::Tool` list.*
        - *Use `llm_client` to parse function calls from response (`mcp::gemini::FunctionCall`).*
        - *Call `happe/src/mcp_client.rs::execute_tool`.*
        - *Reference `cli/src/app.rs` lines ~81-112, ~211, ~232-247.*
    - [x] Call `IdaClient::store_turn_async` with `ipc::internal_messages::ConversationTurn`.
        - *Reference `cli/src/app.rs` lines ~218-227, ~250-270.*
- [x] **LLM Client Implementation (`happe/src/llm_client.rs`):**
    - [x] Implement API call logic (using `reqwest`, `core::client::GeminiClient`, `core::types::GenerateContentRequest`).
        - *Adapt from `gemini-core::client` & `cli/src/app.rs` lines ~189-211.*
    - [x] Implement response parsing (`extract_text_from_response`, `extract_function_calls_from_response` using `mcp::gemini::parse_function_calls`).
    - [x] Define proper error types (consider using/extending `core::errors`).
- [x] **MCP Client Implementation (`happe/src/mcp_client.rs` - If needed):**
    - [x] Implement `get_capabilities` function (likely wrapper around `mcp::host::McpHost::get_all_capabilities`).
        - *Reference `cli/src/app.rs::get_capabilities` (~line 1235).*\
    - [x] Implement `execute_tool` function (using `mcp::host::McpHost::execute_tool`).
        - *Reference `cli/src/app.rs::execute_tool` (~line 1209).*\
    - [x] Ensure `HAPPE` has an `McpProvider` instance (`mcp::host::McpHost`).
- [x] **HAPPE Input API & Servers:**
    - [x] Implement IPC server (`happe/src/ipc_server.rs`) using `ipc::happe_request` types.
    - [x] Implement HTTP server (`happe/src/http_server.rs`) using `axum`/`warp`.
    - [x] Update `happe-daemon.rs` to initialize `core::client::GeminiClient` and `mcp::host::McpHost` and pass them to handlers.
- [x] **Binary Implementation (`happe/src/bin/happe-daemon.rs`):**
    - [x] Implement real configuration loading (using `core::config::GeminiConfig` as base, loading MCP config with `mcp::config::load_mcp_servers`).
        - *Migrate from `cli/src/config.rs` & `cli/src/main.rs`.*\
    - [x] Define comprehensive CLI arguments (`clap`).
- [x] **Dependencies Update (`happe/Cargo.toml`):**
    - [x] Add `@core`, `@mcp`, `@ipc`.
    - [x] Add `reqwest`, `axum`/`warp`, `serde_json`.
    - [x] Ensure features match usage.
- [x] **Verification (Full):**
    - [x] Run `cargo check --all-targets` / `build --all-targets`.
- **Milestone:** `happe-daemon` fully functional: processes queries via IPC/HTTP, interacts with real LLM and MCP servers, and uses placeholder IDA for memory.

## Phase 6: Implement IDA Core Logic (`@ida` Crate)

- [x] **Storage Logic (`ida/src/storage.rs`):**
    - [x] Replace placeholder logic in `handle_storage`.
    - [x] Implement analysis of `ipc::internal_messages::ConversationTurn`.
    - [x] Implement memory summarization/formatting.
        - *Consider adapting `cli/src/history.rs::summarize_conversation`.*
    - [x] Implement interaction with Memory Backend:
        - **Option A (Direct Access):** Use `gemini_memory::store::MemoryStore`.
            - [x] Initialize `MemoryStore` in `ida-daemon.rs`.
            - [x] Call `MemoryStore` methods (add, check duplicates, etc.).
            - [x] Handle conversion between `gemini_memory::memory::Memory` and `ipc::internal_messages::MemoryItem`.
            - *Replaces `cli/src/memory/mod.rs`, `cli/src/memory_broker.rs`.*
            - *Include async queue/workers if needed, migrating from `cli/src/memory/mod.rs`.*
        - **Option B (MCP Access):** Use `@mcp::host::McpHost` as a client.
            - [ ] Initialize `McpHost` in `ida-daemon.rs`, configured for the Memory MCP server.
            - [ ] Define and call MCP methods for `store_memory`, `check_duplicates`, etc.
    - [x] Implement duplicate checking logic (either via `MemoryStore` or MCP call).
- [x] **Retrieval Logic (`ida/src/memory_mcp_client.rs` or direct):**
    - [x] Replace placeholder `retrieve_memories`.
    - [x] Implement query logic against Memory Backend:
        - **Option A (Direct Access):** Use `gemini_memory::store::MemoryStore::query_memories`.
            - [x] Handle conversion from `gemini_memory::memory::Memory` to `ipc::internal_messages::MemoryItem`.
        - **Option B (MCP Access):** Call MCP method for `query_memories`.
            - *Replaces calls like `enhance_prompt` in `cli/src/app.rs`.*
- [x] **Configuration (`ida/src/bin/ida-daemon.rs`):**
    - [x] Add config based on chosen backend access (MemoryStore path/config OR Memory MCP server address).
    - [x] Add config for async workers, etc.
        - *Migrate from `cli/src/config.rs::AsyncMemoryConfigExt`.*
- [x] **Dependencies (`ida/Cargo.toml`):**
    - [x] Add `@ipc`.
    - [x] **Option A:** Add `@memory`.
    - [x] **Option B:** Add `@mcp`.
    - [x] Maybe add `@core` (for errors/config base).
- [x] **Verification:**
    - [x] Run `cargo check`/`build`.
    - [ ] Unit tests for storage/retrieval logic.
- **Milestone:** `ida-daemon` fully functional: stores, summarizes, and retrieves memories based on real logic, interacting with the memory backend.

## Phase 7: Refactor CLI (`@cli` Crate)

- [x] **Modify Main Logic (`cli/src/main.rs`, `cli/src/app.rs`):**
    - [x] Remove LLM/MCP/Memory/History logic.
    - [x] Keep input loop (`run_interactive_chat`, etc.).
    - [x] Add IPC client (`cli/src/happe_client.rs`?) to connect to `happe-daemon` query socket.
    - [x] Send query via IPC, receive response, display using `output.rs`.
- [x] **Update CLI Arguments (`cli/src/cli.rs::Args`):**
    - [x] Remove LLM/memory/etc. args. Add HAPPE connection arg.
- [x] **Update Configuration (`cli/src/config.rs`):**
    - [x] Remove old config logic. Add HAPPE connection config.
- [x] **Remove Unused Code:**
    - [x] Gut/delete `history.rs`, `memory/`, `memory_broker.rs`.
    - [x] Remove unused dependencies (`gemini-core`, `gemini-memory`, `@mcp`) from `cli/Cargo.toml`.
    - [x] Clean up `app.rs`.
- [x] **Dependencies (`cli/Cargo.toml`):**
    - [x] Add `@ipc` (for `HappeQueryRequest`/`Response`).
    - [x] Add necessary IPC client library (`tokio`?).
- [x] **Verification:**
    - [x] Run `cargo check`/`build`.
- **Milestone:** `cli` executable successfully acts as a front-end to `happe-daemon` via IPC, core logic removed.

## Phase 8: End-to-End Integration Testing

- [ ] **Setup:** Run `ida-daemon`, `happe-daemon`, and any required MCP servers.
- [ ] **CLI Test:** Use the refactored `@cli` to interact with `happe-daemon`.
    - [ ] Verify queries are processed correctly.
    - [ ] Verify responses are displayed.
    - [ ] Verify memory retrieval influences responses over time (check `IDA` logs).
    - [ ] Verify turns are stored (check `IDA` logs).
- [ ] **HTTP Test:** Use `curl` or similar to send queries to `happe-daemon`'s HTTP endpoint.
    - [ ] Verify responses are correct.
    - [ ] Verify interaction affects memory via `IDA` logs.
- [ ] **Tool Call Test:** If MCP tools are implemented, trigger them via queries.
    - [ ] Verify `HAPPE` logs show function call parsing and execution via MCP.
    - [ ] Verify tool results are sent back to LLM and influence final response.
- **Milestone:** Full system (CLI/HTTP -> HAPPE -> LLM/MCP & IDA -> Memory Backend) functions correctly for multiple interaction turns.

## General Tasks (Ongoing)

- [ ] **Code Formatting:** Run `cargo fmt` regularly across the workspace.
- [ ] **Linting:** Run `cargo clippy --all-targets --all-features` regularly and address warnings.
- [ ] **Unit Tests:** Add unit tests for specific functions with complex logic.
- [ ] **Documentation:** Add doc comments (`///`) for public functions, structs, and enums.
- [ ] **Error Handling:** Ensure errors are propagated or handled gracefully.
- [ ] **Configuration:** Refine configuration loading (e.g., use environment variables, config files).
- [ ] **Logging:** Improve logging messages for clarity and debugging.
- [ ] **Version Control:** Commit changes frequently with clear messages.
