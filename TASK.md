# Task List: Gemini CLI Refactoring & Bug Fixing

Date Format: YYYY-MM-DD

## High Priority

*   [ ] **Host Refactor:** Address compilation errors in `src/mcp/host.rs` (2024-08-18)
    *   [x] Move helper functions (`execute_tool_by_qualified_name`, `get_resource_by_qualified_name`) inside `impl McpHost`.
    *   [x] Fix unresolved imports (`GetResourceParams`, `Message`, `LogMessageParams`, `AsyncReadExt`).
    *   [x] Fix struct field usage (`ExecuteToolParams`, `Tool.parameters`).
    *   [x] Implement `execute_tool` and `get_resource` async methods on `ActiveServer`.
    *   [ ] Fix type mismatches & logic in `handle_response` (`response.id`, `response.result`).
    *   [ ] Fix async closure issue (`await` in non-async closure).
    *   [ ] Fix `rpc_error.message` access on `RecvError`.
    *   [ ] Fix `Display` trait errors for `Option<u64>` in logging macros.
*   [ ] **Config Error Handling:** Address `Send`/`Sync`/`Sized` errors for `dyn Error` in `src/mcp/config.rs` (2024-08-18).
*   [ ] **Code Cleanup:** Remove all remaining unused imports and variables warnings (2024-08-18).

## Medium Priority

*   [ ] **Enhance Filesystem MCP Server:** Add more tools (write, patch, create/delete dir, rename, metadata) and range support for read (2024-08-19).

## Backlog 