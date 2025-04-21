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

## Phase 9: Enhance IDA with Broker LLM (Direct Integration)

This phase implements the plan to integrate a "broker" LLM directly within IDA
to refine memory retrieval results before sending them to HAPPE.

### Sub-Phase 1: Prerequisites and Setup

- [x] **Broker LLM Configuration Loading:**
    - [x] Verify `MemoryBrokerConfig` fields (`provider`, `api_key`, `model_name`, `base_url`) are loaded correctly into `IdaConfig`.
    - [x] Add loaded `IdaConfig` to shared `DaemonState` struct.
    - [x] Update `DaemonState` instantiation in `ida/src/bin/ida-daemon.rs`.
    - [x] Ensure `handle_message` in `ida/src/ipc_server.rs` can access the `IdaConfig`.
- [x] **Ensure MCP Access Reuse in IDA:**
    - [x] Identify where `MemoryStore` is initialized in `ida/src/bin/ida-daemon.rs`.
    - [x] Store the `Arc<dyn McpHostInterface + Send + Sync>` used for `MemoryStore` in the `DaemonState`.
    - [x] Update `DaemonState` definition and instantiation.
- [x] **Integrate Broker LLM Client into IDA:**
    - [x] **Configuration (`core/src/config.rs`):**
        - [x] Modify `MemoryBrokerConfig` struct (add `provider`, `base_url`, etc.).
        - [x] Update corresponding `IdaConfig` in `ida/src/config.rs` and its `From` impl.
        - [x] Update default config generation (`install` or `tools`) for `[ida.memory_broker]`.
    - [x] **Implementation (IDA Crate):**
        - [x] Add dependencies (`reqwest`, `serde`, `serde_json`, `async-trait`) to `ida/Cargo.toml`.
        - [x] Create `ida/src/llm_clients.rs`.
        - [x] Define `trait LLMClient { async fn generate(&self, prompt: &str) -> Result<String>; }`.
        - [x] Implement the trait for supported providers (e.g., `GeminiClient`, `OllamaClient`).
        - [x] Create factory function `create_llm_client(config: &CoreMemoryBrokerConfig) -> Result<Option<Arc<dyn LLMClient>>>`.
        - [x] Add `Option<Arc<dyn LLMClient>>` field to `DaemonState`.
        - [x] Initialize the client in `ida/src/bin/ida-daemon.rs` using the factory.

### Sub-Phase 2: Core Logic Implementation in IDA

- [x] **Modify `ida::memory_mcp_client::retrieve_memories`:**
    - [x] Change signature to accept `broker_llm_client: &Option<Arc<dyn LLMClient>>` and `conversation_context: Option<String>`.
    - [x] After semantic search, if results exist and client exists:
        - [x] Construct broker prompt (query, context, candidates, instructions).
        - [x] Call `client.generate(&broker_prompt).await`.
        - [x] Parse response (e.g., comma-separated keys).
        - [x] Filter semantic search results based on broker response.
        - [x] Implement fallback logic on broker error.
    - [x] Update call site in `ida/src/ipc_server.rs` (`handle_message`) to pass the client and context.

### Sub-Phase 3: Integrating Conversation Context (IPC Changes)

- [x] **Modify IPC Message (`ipc/src/internal_messages.rs`):**
    - [x] Add `conversation_context: Option<String>` to `GetMemoriesRequest`.
- [x] **Update HAPPE Client (`happe/src/ida_client.rs`):**
    - [x] Update `get_memories` signature and request construction.
- [x] **Update HAPPE Orchestrator (`happe/src/coordinator.rs` or similar):**
    - [x] Gather context and pass it to `ida_client::get_memories`.
- [x] **Update IDA Server (`ida/src/ipc_server.rs`):**
    - [x] Extract context and pass it to `memory_mcp_client::retrieve_memories`.

### Sub-Phase 4: Testing and Refinement

- [ ] **Testing:** Unit, Integration, End-to-End tests.
- [ ] **Refinement:** Prompt engineering, latency analysis, fallback logic, configuration.

## Phase 10: Implement Configurable Session Management in HAPPE

This phase adds a stateful session management system to HAPPE, allowing it to maintain conversation history across multiple requests within a session. This is crucial for providing proper context to `IDA` for memory retrieval.

### Sub-Phase 1: Define Core Session Store Trait & In-Memory Adapter

- [x] **Module Setup:**
    - [x] Create directory `happe/src/session/`.
    - [x] Create directory `happe/src/session/adapters/`.
    - [x] Create file `happe/src/session/mod.rs`.
    - [x] Create file `happe/src/session/store.rs`.
    - [x] Create file `happe/src/session/adapters/in_memory.rs`.
    - [x] Declare `session` module in `happe/src/lib.rs` (`pub mod session;`).
- [x] **Define `SessionStore` Trait (`happe/src/session/store.rs`):**
    - [x] Define `#[async_trait] pub trait SessionStore: Send + Sync`.
    - [x] Add methods for session management:
        - [x] `create_session`
        - [x] `get_session`
        - [x] `save_session`
        - [x] `delete_session`
        - [x] `cleanup_expired_sessions`
- [x] **Implement `InMemorySessionStore` (`happe/src/session/adapters/in_memory.rs`):**
    - [x] Define `struct InMemorySessionStore`.
    - [x] Add thread-safe storage using `Arc<RwLock<HashMap<String, Session>>>`.
    - [x] Implement `SessionStore` trait.
    - [x] Add expiration and cleanup logic.
- [x] **Dependencies (`happe/Cargo.toml`):**
    - [x] Add `async-trait = "0.1"`.
    - [x] Add `uuid = { version = "1", features = ["v4"] }` (for session ID generation).
    - [x] Ensure `tokio` features include `sync`.

### Sub-Phase 2: Integrate Session Store into HAPPE State

- [x] **State (`happe/src/http_server.rs`, `happe/src/ipc_server.rs`):**
    - [x] Add `session_store: SessionStoreRef` to `AppState` struct (`http_server.rs`).
    - [x] Add `session_store: SessionStoreRef` to `IpcServerState` struct (`ipc_server.rs`).
    - [x] Initialize session store in HTTP and IPC servers.
    - [x] Add session cleanup task to the IPC server.

### Sub-Phase 3: Modify Coordinator & IPC Handler

- [x] **Coordinator (`happe/src/coordinator.rs`):**
    - [x] Modify `process_query` signature to accept `session: &Session`.
    - [x] Add helper function `get_conversation_history` to extract history from the session.
    - [x] Add helper function `update_session_history` to store turns in the session.
- [x] **IPC Handler (`happe/src/ipc_server.rs`):**
    - [x] Modify `handle_connection`:
        - [x] Get or create session for the request.
        - [x] Pass session to `coordinator::process_query`.
        - [x] Update session with new conversation turn.
        - [x] Save session back to the store.
- [x] **HTTP Handler (`happe/src/http_server.rs`):**
    - [x] Modify `handle_query`:
        - [x] Extract session ID from request or create a new one.
        - [x] Get or create session.
        - [x] Pass session to `coordinator::process_query`.
        - [x] Update and save session.

### Sub-Phase 4: Update IPC Request & Client (`@cli`)

- [x] **IPC Request (`ipc/src/happe_request/types.rs` or `mod.rs`):**
    - [x] Add `session_id: Option<String>` field to `HappeQueryRequest` struct.
    - [x] Add `session_id: Option<String>` field to `HappeQueryResponse` struct.
- [x] **CLI (`@cli` Crate):**
    - [x] Identify where the main interaction loop/HAPPE client logic resides (e.g., `cli/src/app.rs` or `cli/src/happe_client.rs`).
    - [x] On CLI startup, generate a persistent `session_id` for the duration of the run (e.g., `let session_id = uuid::Uuid::new_v4().to_string();`).
    - [x] Modify the code that creates and sends `HappeQueryRequest` to include this `session_id`.

### Sub-Phase 5: Testing & Refinement

- [x] **Unit Tests:**
    - [x] Add tests for the `Session` struct in `store.rs`.
    - [x] Add tests for `InMemorySessionStore`.
- [ ] **Integration Tests:** Test IPC handler with session history.
- [ ] **End-to-End Tests (`@cli` -> `happe-daemon`):** Verify context is maintained across multiple turns within a single CLI run.
- [ ] **Refinement:** Assess performance, history pruning, error handling, security of session IDs.
