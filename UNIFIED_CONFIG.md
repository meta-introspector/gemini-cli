# Plan for Unified Configuration in Gemini Suite

**Goal:** Consolidate configuration loading logic and settings for all Gemini Suite components (CLI, daemons) into a single source of truth (`gemini-core/src/config.rs`), managed by the `core` crate and potentially generated/updated by the `tools/generate_unified_config.rs` script. Exclude `mcp_servers.json`.

**Plan:**

1.  **Define Unified Configuration Structure (`core` crate):**
    *   Create `core/src/config.rs`.
    *   Define `UnifiedConfig` struct holding all settings, using nested structs (e.g., `CliConfig`, `HappeConfig`, etc.).
    *   Derive `serde::Serialize` and `serde::Deserialize`.
    *   Choose TOML format and standard location (e.g., `~/.config/gemini-suite/config.toml` or env var `GEMINI_SUITE_CONFIG_PATH`).

2.  **Implement Configuration Loading (`core` crate):**
    *   Add dependencies (`serde`, `toml`, `config-rs`/`figment`, `directories`) to `core/Cargo.toml`.
    *   Implement `load_config()` in `core/src/config.rs` to:
        *   Find config file path.
        *   Read and deserialize into `UnifiedConfig`.
        *   Handle errors.
        *   (Optional) Merge sources (file, env vars).

3.  **Audit and Refactor Crates:**
    *   **For each relevant crate (`cli`, `daemon-manager`, `happe`, `ida`, `mcp`, `memory`):**
        *   **3.1 Audit:** Systematically search the crate's codebase (using `grep`, IDE search, etc.) for *any* existing configuration mechanisms:
            *   Reading environment variables (e.g., `std::env::var`).
            *   Reading specific config files (e.g., `.env`, custom formats).
            *   Hardcoded configuration values.
            *   `clap` arguments used for configuration values (not just operational flags).
            *   Any pre-existing attempts at unified configuration.
            *   **Maintain a meticulous log (`UNIFIED_CONFIG_REFACTOR_LOG.md`)** detailing the file, line number, and type of configuration found in each crate *before* refactoring.
        *   **3.2 Add Dependency:** Add `gemini-core` to the crate's `Cargo.toml`.
        *   **3.3 Refactor:** In the crate's entry point/initialization:
            *   Call `gemini_core::config::load_config()` once.
            *   Remove *all* identified legacy configuration logic (from step 3.1).
            *   Pass the relevant parts of the loaded `UnifiedConfig` struct (e.g., `config.happe`) to the components needing them. Update function signatures and struct definitions as necessary.
            *   **Update the refactoring log** indicating the changes made to replace the legacy logic with the unified config approach.
    *   **Note:** Handle `install` and `tools` crates appropriately (config generation vs. runtime needs). `ipc` likely needs no config.

4.  **Update Configuration Generator (`tools/generate_unified_config.rs`):**
    *   Add `gemini-core` dependency.
    *   Modify script to:
        *   Use `gemini_core::config::UnifiedConfig`.
        *   Populate defaults.
        *   Serialize to TOML.
        *   Write to the standard config path.

5.  **Update Documentation:**
    *   Modify `README.md` files (root, `cli`, `daemon-manager`, `happe`, `ida`, `mcp`, `memory`).
    *   Document unified config file (location, format, structure, modification).
    *   Explain `generate_unified_config` tool's role.

6.  **Testing:**
    *   Unit tests for `core::config::load_config()`.
    *   Integration tests for all components verifying config usage.
    *   Test overriding defaults.
    *   Run `cargo clippy --all -- -D warnings` and `cargo fmt --all`. 