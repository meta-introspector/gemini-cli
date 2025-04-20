use crate::memory::Memory;
use crate::schema::EmbeddingModelVariant;
use crate::broker::McpHostInterface; // Assuming this is needed for generate_embedding call within batch fn

use anyhow::{Context, Result, anyhow};
use arrow_array::{RecordBatch, types::{Int64Type, Float32Type}, Array};
use arrow_array::cast::AsArray;
use arrow_array::builder::{Int64Builder, ListBuilder, StringBuilder, Float32Builder, FixedSizeListBuilder};
use arrow_schema::{DataType, Field, SchemaRef};
use futures::future;
use log::warn;
use std::sync::Arc;
use uuid::Uuid;
use tiktoken_rs::cl100k_base;

/// Convert a Vec<Memory> (plus a generated UUID) to an Arrow RecordBatch.
/// Generates embeddings for each memory's value field if MCP is available.
pub(crate) async fn memories_to_batch_with_embeddings(
    memories: Vec<(Uuid, Memory)>,
    schema: SchemaRef,
    embedding_model: EmbeddingModelVariant, // Pass embedding model info
    mcp_host: Option<Arc<dyn McpHostInterface + Send + Sync>>, // Pass MCP host
    generate_embedding_fn: impl Fn(&str, EmbeddingModelVariant, Option<Arc<dyn McpHostInterface + Send + Sync>>) -> future::BoxFuture<'static, Result<Vec<f32>>> // Function to generate embeddings
) -> Result<RecordBatch> {
    // Get tokenizer instance once
    let bpe = cl100k_base().context("Failed to load cl100k_base tokenizer")?;

    let _capacity = memories.len();
    let mut id_builder = StringBuilder::new();
    let mut key_builder = StringBuilder::new();
    let mut value_builder = StringBuilder::new();
    let mut timestamp_builder = Int64Builder::new();
    let mut tags_builder = ListBuilder::new(StringBuilder::new());
    let mut token_count_builder = Int64Builder::new(); // Builder for token count
    // -- Metadata Builders --
    let mut session_id_builder = StringBuilder::new();
    let mut source_builder = StringBuilder::new();
    let mut related_keys_builder = ListBuilder::new(StringBuilder::new());
    let mut confidence_score_builder = Float32Builder::new();

    // Use FixedSizeListBuilder for the vector column
    let vector_dim = embedding_model.dimension();
    let _vector_field = Arc::new(Field::new("item", DataType::Float32, true));
    let mut vector_builder = FixedSizeListBuilder::new(
        Float32Builder::new(),
        vector_dim as i32
    );

    // First pass: Collect all the embeddings in parallel if possible
    let mut embedding_futures = Vec::with_capacity(memories.len());

    for (_, mem) in &memories {
        // Call the passed embedding generation function
        embedding_futures.push(generate_embedding_fn(&mem.value, embedding_model, mcp_host.clone()));
    }

    // Wait for all embeddings to be generated
    let embeddings = future::join_all(embedding_futures).await;

    // Second pass: Build the record batch with the generated embeddings
    for (i, (uuid, mem)) in memories.iter().enumerate() {
        id_builder.append_value(uuid.to_string());
        key_builder.append_value(&mem.key);
        value_builder.append_value(&mem.value);
        // Cast u64 timestamp to i64
        timestamp_builder.append_value(mem.timestamp as i64);

        // Build tags list
        let tag_builder = tags_builder.values();
        for tag in &mem.tags {
            tag_builder.append_value(tag);
        }
        tags_builder.append(true); // Mark the list entry as valid

        // Calculate and append token count
        // Use bpe.encode_with_special_tokens for a more accurate count if needed
        let token_count = bpe.encode_ordinary(&mem.value).len();
        token_count_builder.append_value(token_count as i64);

        // -- Append Metadata --
        match &mem.session_id {
            Some(id) => session_id_builder.append_value(id),
            None => session_id_builder.append_null(),
        }
        match &mem.source {
            Some(src) => source_builder.append_value(src),
            None => source_builder.append_null(),
        }
        match &mem.related_keys {
            Some(keys) => {
                let key_builder = related_keys_builder.values();
                for key in keys {
                    key_builder.append_value(key);
                }
                related_keys_builder.append(true);
            }
            None => related_keys_builder.append(false), // Mark list as null
        }
         match mem.confidence_score {
            Some(score) => confidence_score_builder.append_value(score),
            None => confidence_score_builder.append_null(),
        }

        // Add the embedding vector
        let embedding = match &embeddings[i] {
            Ok(emb) => emb.clone(), // Clone here
            Err(e) => {
                warn!("Failed to get embedding for key '{}', using zeros instead: {}", mem.key, e);
                vec![0.0; vector_dim]
            }
        };

        let values = vector_builder.values();
        if embedding.len() == vector_dim {
            for value in embedding {
                values.append_value(value);
            }
            vector_builder.append(true);
        } else {
             warn!("Embedding dimension mismatch for key '{}': expected {}, got {}. Appending null.", mem.key, vector_dim, embedding.len());
             vector_builder.append(false); // Append null if dimension mismatch
        }

    }

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(id_builder.finish()),
            Arc::new(key_builder.finish()),
            Arc::new(value_builder.finish()),
            Arc::new(timestamp_builder.finish()),
            Arc::new(tags_builder.finish()),
            Arc::new(vector_builder.finish()),
            Arc::new(token_count_builder.finish()), // Add token count array
            // -- Add Metadata Arrays --
            Arc::new(session_id_builder.finish()),
            Arc::new(source_builder.finish()),
            Arc::new(related_keys_builder.finish()),
            Arc::new(confidence_score_builder.finish()),
        ],
    )?;
    Ok(batch)
}

/// Convert an Arrow RecordBatch to a Vec<Memory>.
pub(crate) fn batch_to_memories(batch: &RecordBatch) -> Result<Vec<Memory>> {
    if batch.num_rows() == 0 {
        return Ok(Vec::new());
    }

    // Get references to the arrays based on the schema defined in `create_schema`
    let key_array = batch
        .column_by_name("key")
        .ok_or_else(|| anyhow!("Missing 'key' column"))?
        .as_string_opt::<i32>() // Use helper for Option<String>
        .ok_or_else(|| anyhow!("Failed to cast 'key' column"))?;

    let value_array = batch
        .column_by_name("value")
        .ok_or_else(|| anyhow!("Missing 'value' column"))?
        .as_string_opt::<i32>()
        .ok_or_else(|| anyhow!("Failed to cast 'value' column"))?;

    let timestamp_array = batch
        .column_by_name("timestamp")
        .ok_or_else(|| anyhow!("Missing 'timestamp' column"))?
        .as_primitive_opt::<Int64Type>() // Use helper for Option<i64>
        .ok_or_else(|| anyhow!("Failed to cast 'timestamp' column"))?;

    let tags_array = batch
        .column_by_name("tags")
        .ok_or_else(|| anyhow!("Missing 'tags' column"))?
        .as_list_opt::<i32>() // Use helper for Option<ListArray>
        .ok_or_else(|| anyhow!("Failed to cast 'tags' column"))?;
    // Ensure the inner type of the list is String
    let _tags_value_array_type = match tags_array.value_type() {
        DataType::Utf8 => Ok(()),
        other => Err(anyhow!("Expected Utf8 for tags list items, found {:?}", other)),
    }?;

    let token_count_array = batch
        .column_by_name("token_count")
        .ok_or_else(|| anyhow!("Missing 'token_count' column"))?
        .as_primitive_opt::<Int64Type>()
        .ok_or_else(|| anyhow!("Failed to cast 'token_count' column"))?;

    let session_id_array = batch
        .column_by_name("session_id")
        .ok_or_else(|| anyhow!("Missing 'session_id' column"))?
        .as_string_opt::<i32>()
        .ok_or_else(|| anyhow!("Failed to cast 'session_id' column"))?;

    let source_array = batch
        .column_by_name("source")
        .ok_or_else(|| anyhow!("Missing 'source' column"))?
        .as_string_opt::<i32>()
        .ok_or_else(|| anyhow!("Failed to cast 'source' column"))?;

    let related_keys_array = batch
        .column_by_name("related_keys")
        .ok_or_else(|| anyhow!("Missing 'related_keys' column"))?
        .as_list_opt::<i32>()
        .ok_or_else(|| anyhow!("Failed to cast 'related_keys' column"))?;
    // Ensure the inner type of the list is String
    let _related_keys_value_array_type = match related_keys_array.value_type() {
        DataType::Utf8 => Ok(()),
        other => Err(anyhow!("Expected Utf8 for related_keys list items, found {:?}", other)),
    }?;

    let confidence_score_array = batch
        .column_by_name("confidence_score")
        .ok_or_else(|| anyhow!("Missing 'confidence_score' column"))?
        .as_primitive_opt::<Float32Type>() // Use helper for Option<f32>
        .ok_or_else(|| anyhow!("Failed to cast 'confidence_score' column"))?;


    // Iterate through rows and reconstruct Memory objects
    let mut memories = Vec::with_capacity(batch.num_rows());
    for i in 0..batch.num_rows() {

        // Extract optional string list for tags
        let tags: Vec<String> = if tags_array.is_valid(i) {
            let tags_list = tags_array.value(i);
            let tags_values = tags_list.as_string::<i32>();
            (0..tags_values.len())
                .map(|j| tags_values.value(j).to_string())
                .collect()
        } else {
            Vec::new() // Default to empty vec if list is null
        };

        // Extract optional string list for related_keys
        let related_keys: Option<Vec<String>> = if related_keys_array.is_valid(i) {
            let keys_list = related_keys_array.value(i);
            let keys_values = keys_list.as_string::<i32>();
            Some((0..keys_values.len()) // Use Array::len()
                .map(|j| keys_values.value(j).to_string())
                .collect())
        } else {
            None
        };

        // Extract timestamp, converting i64 to u64
        let timestamp: u64 = timestamp_array.value(i)
            .try_into()
            .unwrap_or_else(|_| {
                warn!("Timestamp value {} out of range for u64 at row {}, using 0", timestamp_array.value(i), i);
                0 // Or handle the error more robustly if needed
            });

        // Extract optional token_count, converting i64 to usize
        let token_count: Option<usize> = if token_count_array.is_valid(i) { // Use is_valid/value
            token_count_array.value(i).try_into().ok()
        } else {
            None
        };


        memories.push(Memory {
            key: if key_array.is_valid(i) { key_array.value(i).to_string() } else { Default::default() }, // Use is_valid/value
            value: if value_array.is_valid(i) { value_array.value(i).to_string() } else { Default::default() }, // Use is_valid/value
            timestamp,
            tags,
            token_count,
            session_id: if session_id_array.is_valid(i) { Some(session_id_array.value(i).to_string()) } else { None }, // Use is_valid/value
            source: if source_array.is_valid(i) { Some(source_array.value(i).to_string()) } else { None }, // Use is_valid/value
            related_keys,
            confidence_score: if confidence_score_array.is_valid(i) { Some(confidence_score_array.value(i)) } else { None }, // Use is_valid/value
        });
    }

    Ok(memories)
} 