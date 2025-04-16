Phase 1: Preparation & Workspace Setup

•  Goal: Create a Cargo workspace to manage the existing CLI binary and the new library crates.
•  Steps:
•  Navigate to the parent directory of `gemini-cli`: `cd /home/james/Documents/`
•  Rename the existing project directory: `mv gemini-cli gemini-suite` (or similar name for the workspace).
•  Enter the new workspace directory: `cd gemini-suite`
•  Create a root `Cargo.toml` file (`/home/james/Documents/gemini-suite/Cargo.toml`) with workspace definition:
toml:
────────────────────────────────────────
[workspace]
members = [
    "cli",       # The original binary application
    "core",      # Core Gemini API interaction, structs, errors
    "mcp",       # MCP/Tool calling client logic
    "memory",    # Persistent memory storage logic
]

[workspace.dependencies]
# Define shared dependencies here once if desired
# e.g., tokio = { version = "1", features = ["full"] }
# serde = { version = "1.0", features = ["derive"] }
# reqwest = { version = "0.12", features = ["json", "rustls-tls"] } # Or native-tls
# tracing = "0.1"
# thiserror = "1.0"
# anyhow = "1.0"
# config = "0.14"
# dotenvy = "0.15"
# jsonwebtoken = "9"
# uuid = { version = "1", features = ["v4", "serde"] }
# chrono = { version = "0.4", features = ["serde"] }
────────────────────────────────────────


•  Create subdirectories for the crates:
•  `mkdir cli core mcp memory`


•  Move the contents of the original `gemini-cli` project (src, Cargo.toml, README.md, config examples, etc.) into the `cli` subdirectory: `mv /path/to/original/contents/* cli/` (adjust path as needed).
•  Create basic `Cargo.toml` files for the new library crates (`core/Cargo.toml`, `mcp/Cargo.toml`, `memory/Cargo.toml`). Define them as libraries:
toml:
────────────────────────────────────────
# Example: core/Cargo.toml
[package]
name = "gemini-core"
version = "0.1.0"
edition = "2021"

[dependencies]
# Add specific dependencies needed for core logic (reqwest, serde, tokio, etc.)
# Or inherit from workspace: tokio = { workspace = true }
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
thiserror = "1.0"
anyhow = "1.0"
config = "0.14"
dotenvy = "0.15"
jsonwebtoken = "9"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
# Add others as identified...
────────────────────────────────────────

(Repeat similar basic structure for `mcp/Cargo.toml` and `memory/Cargo.toml`, adding their specific dependencies)
•  Update `cli/Cargo.toml` to reflect its new name (`gemini-cli`) and add path dependencies to the future library crates:
toml:
────────────────────────────────────────
# cli/Cargo.toml
[package]
name = "gemini-cli" # Or keep original name if preferred
version = "0.1.0"
edition = "2021"

[[bin]] # Ensure it's defined as a binary
name = "gemini" # The command name
path = "src/main.rs"

[dependencies]
# Keep original CLI dependencies (clap, etc.)
clap = { version = "4", features = ["derive"] }
# Add path dependencies to the new libs
gemini-core = { path = "../core" }
gemini-mcp = { path = "../mcp" }
gemini-memory = { path = "../memory" }
# Add/keep tokio, tracing, anyhow, etc.
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
# ... other cli specific dependencies ...
────────────────────────────────────────


•  Run `cargo check --workspace` from the root (`gemini-suite/`) to ensure the basic structure is recognized.



Phase 2: Extract Core Gemini Logic (`gemini-core`)

•  Goal: Move Gemini API interaction, shared data structures (requests, responses, context, claims), configuration loading, and potentially shared error types into the `gemini-core` library.
•  Steps:
•  Identify relevant code in `cli/src/`:
•  `gemini.rs` / `api.rs` (API client logic)
•  Struct definitions for requests/responses/content/parts/tools.
•  `context.rs` (Context management logic/structs).
•  `config.rs` (Configuration struct definition and loading).
•  `errors.rs` (If defining shared error types).
•  JWT `Claims` struct (if defined separately).


•  Move this code into `core/src/`. Organize into modules (e.g., `core/src/client.rs`, `core/src/types.rs`, `core/src/config.rs`, `core/src/errors.rs`, `core/src/context.rs`). Ensure `core/src/lib.rs` declares these modules (`mod client; pub use client::*;` etc.).
•  Define the public API of `gemini-core`: Mark necessary structs, functions (like `GeminiClient::new`, `GeminiClient::generate_content`, `Config::load`), enums, and traits as `pub`.
•  Update `core/Cargo.toml` with all necessary dependencies identified during the move (`reqwest`, `serde`, `tokio`, `config`, `dotenvy`, `jsonwebtoken`, `uuid`, `chrono`, `thiserror`, `tracing`, etc.).
•  Refactor code within `cli/src/` (mainly `main.rs` and `commands/chat.rs`) to:
•  Remove the moved modules/code.
•  Import types from `gemini_core::...`.
•  Instantiate the `GeminiClient` (likely passing the loaded `Config`).
•  Use the client methods for API calls.
•  Adapt to any changes in context management or error handling interfaces defined by `gemini-core`.


•  Run `cargo check --workspace` and `cargo build --workspace` frequently. Fix compilation errors.
•  Verification: Run the `gemini chat` command. Ensure it still connects, sends prompts, receives responses, and handles context correctly. Test basic error conditions (e.g., invalid API key if possible).



Phase 3: Extract Memory Logic (`gemini-memory`)

•  Goal: Move the persistent memory storage logic into the `gemini-memory` library.
•  Steps:
•  Identify relevant code in `cli/src/`:
•  `memory.rs` (Storage logic, CRUD functions).
•  Struct definitions for memory items.


•  Move this code into `memory/src/`. Organize into modules if needed (e.g., `memory/src/store.rs`). Ensure `memory/src/lib.rs` declares modules and exports the public API.
•  Define the public API: Mark the main storage struct/trait (e.g., `MemoryStore`) and its methods (`store`, `retrieve`, `list`, `delete`) as `pub`.
•  Update `memory/Cargo.toml` with dependencies (e.g., `serde`, `tokio`, file I/O crates, potentially `sled` or `rusqlite`, maybe `gemini-core` for shared errors).
•  Refactor code within `cli/src/` (mainly `commands/memory.rs` and potentially `commands/chat.rs`) to:
•  Remove the moved memory module.
•  Import types/functions from `gemini_memory::...`.
•  Instantiate the `MemoryStore` (passing configuration like storage path if needed).
•  Use the `MemoryStore` methods for all memory operations.


•  Run `cargo check/build --workspace`. Fix errors.
•  Verification: Run all `gemini memory` subcommands (`store`, `retrieve`, `list`, `delete`). Ensure they work as before. Check if `gemini chat` still interacts with memory correctly (if it did previously).



Phase 4: Extract MCP/Tool Logic (`gemini-mcp`)

•  Goal: Move the client logic for interacting with MCP tool servers into the `gemini-mcp` library.
•  Steps:
•  Identify relevant code in `cli/src/`:
•  `mcp.rs` / `tools.rs` (MCP client, dispatch logic).


•  Move this code into `mcp/src/`. Organize into modules (e.g., `mcp/src/client.rs`). Ensure `mcp/src/lib.rs` declares modules and exports the public API.
•  Define the public API: Mark the MCP client struct/functions (e.g., `McpClient`, `execute_tool_call`) as `pub`.
•  Update `mcp/Cargo.toml` with dependencies (`reqwest`, `serde`, `tokio`, `tracing`). Crucially, add `gemini-core = { path = "../core" }` as it needs types like `FunctionCall`, `FunctionResponse`, and potentially shared errors/config.
•  Refactor code within `cli/src/` (mainly `commands/chat.rs`) to:
•  Remove the moved MCP module.
•  Import types/functions from `gemini_mcp::...`.
•  Instantiate the `McpClient` (passing necessary configuration like tool server URLs).
•  Use the `McpClient` methods for dispatching tool calls.


•  Run `cargo check/build --workspace`. Fix errors.
•  Verification: Run `gemini chat` with prompts designed to trigger tool calls (if you have testable MCP servers). Verify that the calls are dispatched and responses are handled correctly.



Phase 5: Final Cleanup & Verification

•  Goal: Ensure the workspace is clean, dependencies are correct, and the original CLI functionality is fully preserved.
•  Steps:
•  Review the public APIs (`pub` items) of `gemini-core`, `gemini-mcp`, and `gemini-memory`. Ensure only necessary items are public.
•  Review dependencies in all `Cargo.toml` files. Remove unused dependencies. Ensure versions are consistent (consider using workspace dependencies).
•  Run `cargo fmt --workspace` and `cargo clippy --workspace` to enforce style and catch potential issues.
•  Perform comprehensive testing of the `gemini-cli` binary:
•  Basic chat.
•  Chat with context.
•  All `memory` subcommands.
•  Tool calling (if possible).
•  Configuration loading scenarios.
•  Error handling (e.g., invalid commands, connection errors if mockable).


•  Consider adding basic unit or integration tests within the library crates themselves.



Outcome:
You will have a `gemini-suite` workspace containing:

•  `cli/`: The original `gemini-cli` application, now significantly thinner and relying on the library crates.
•  `core/`: A library for core Gemini interaction, types, config, and errors.
•  `mcp/`: A library for MCP client logic.
•  `memory/`: A library for persistent memory storage.

These libraries (`gemini-core`, `gemini-mcp`, `gemini-memory`) can then be published (if desired) or directly included as path dependencies in the `pywatt-rust` project for integration. The original CLI remains fully functional.
