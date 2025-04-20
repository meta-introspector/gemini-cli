use arrow_schema::{DataType, Field, Schema, SchemaRef};
use std::sync::Arc;

// Constants for embedding dimensions based on E5 model variants
pub const EMBEDDING_DIM_SMALL: usize = 384;
pub const EMBEDDING_DIM_BASE: usize = 768;
pub const EMBEDDING_DIM_LARGE: usize = 1024;

// Enum for supported embedding model variants
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EmbeddingModelVariant {
    Small,
    Base,
    Large,
}

impl EmbeddingModelVariant {
    pub fn dimension(&self) -> usize {
        match self {
            Self::Small => EMBEDDING_DIM_SMALL,
            Self::Base => EMBEDDING_DIM_BASE,
            Self::Large => EMBEDDING_DIM_LARGE,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Base => "base",
            Self::Large => "large",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "small" => Some(Self::Small),
            "base" => Some(Self::Base),
            "large" => Some(Self::Large),
            _ => None,
        }
    }
}

impl Default for EmbeddingModelVariant {
    fn default() -> Self {
        Self::Base
    }
}

/// Defines the LanceDB schema for storing Memory objects.
/// Includes a vector field for embeddings.
pub fn create_schema(embedding_dim: usize) -> SchemaRef {
    Arc::new(Schema::new(vec![
        // Use a unique ID instead of the potentially non-unique 'key'
        Field::new("id", DataType::Utf8, false), // Primary key (UUID)
        Field::new("key", DataType::Utf8, true),
        Field::new("value", DataType::Utf8, true),
        // LanceDB prefers Int64 for timestamps
        Field::new("timestamp", DataType::Int64, true),
        Field::new(
            "tags",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        ),
        // Vector field for embeddings - dimension depends on model variant
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), embedding_dim as i32),
            true, // Nullable for now, until embedding generation is integrated
        ),
        // Add optional token count field
        Field::new("token_count", DataType::Int64, true),
        // -- Metadata Fields --
        Field::new("session_id", DataType::Utf8, true),
        Field::new("source", DataType::Utf8, true),
        Field::new(
            "related_keys",
            DataType::List(Arc::new(Field::new("item", DataType::Utf8, true))),
            true,
        ),
        Field::new("confidence_score", DataType::Float32, true),
    ]))
} 