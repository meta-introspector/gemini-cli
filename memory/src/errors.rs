use thiserror::Error;

#[derive(Debug, Error)]
pub enum MemoryStoreError {
    #[error("LanceDB error: {0}")]
    LanceDB(#[from] lancedb::Error),
    #[error("Arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Operation error: {0}")]
    Operation(String),
    #[error("Embedding error: {0}")]
    Embedding(String),
    #[error("Initialization error: {0}")]
    Initialization(String),
    #[error("Table not found: {0}")]
    TableNotFound(String),
    #[error("Search error: {0}")]
    Search(String),
}
