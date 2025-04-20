use anyhow::{Context, Result};
use std::path::PathBuf;
use tracing::debug;

/// Get the path for the LanceDB database directory.
pub fn get_memory_db_path() -> Result<PathBuf> {
    let mut config_dir = dirs::config_dir().context("Could not find config directory")?;
    config_dir.push("gemini-cli");
    config_dir.push("memory.lancedb"); // Use .lancedb extension/directory
    Ok(config_dir)
}

/// Ensure the directory for the LanceDB database exists.
pub fn ensure_memory_db_dir(db_path: &PathBuf) -> Result<()> {
    if let Some(parent) = db_path.parent() {
        // LanceDB connect creates the final directory, ensure parent exists
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    } else if !db_path.exists() {
        // If the path itself has no parent (e.g., relative path in cwd)
        // LanceDB connect should handle creating it.
        // No action needed here, but log for clarity.
        debug!(
            "LanceDB path {} has no parent, assuming connect will create it.",
            db_path.display()
        );
    }
    Ok(())
}
