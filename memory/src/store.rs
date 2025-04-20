use crate::memory::Memory;
use crate::broker::McpHostInterface;
use crate::schema::EmbeddingModelVariant;
use anyhow::{Context, Result, anyhow, Error};
use arrow_array::{RecordBatch, RecordBatchIterator, Array, Float32Array};
use arrow_schema::SchemaRef;
use futures::{TryStreamExt, StreamExt, stream::BoxStream};
use lancedb::{connect, Connection as LanceConnection, Table as LanceTable};
use lancedb::query::ExecutableQuery;
use lancedb::query::*;
use serde_json::{self, json};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use uuid::Uuid;
use tracing::{debug, info, warn};
use std::pin::Pin;
use std::future::Future;

// Add use statements for items from the modules (referenced via crate::...)
use crate::schema::create_schema;
use crate::config::{get_memory_db_path, ensure_memory_db_dir};
// Import arrow_conversion functions needed
use crate::arrow_conversion;

/// MemoryStore manages a collection of Memory items using LanceDB.
// #[derive(Debug)] // Removed derive
pub struct MemoryStore {
    table: Arc<LanceTable>, // Use concrete type
    schema: SchemaRef,
    embedding_model: EmbeddingModelVariant,
    mcp_host: Option<Arc<dyn McpHostInterface + Send + Sync>>,
    _db_path: String, // Prefixed to silence warning
    _table_name: String, // Prefixed to silence warning
}

// Manual Debug implementation for MemoryStore
impl fmt::Debug for MemoryStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MemoryStore")
            .field("schema", &self.schema)
            .field("embedding_model", &self.embedding_model)
            // Skip non-Debug fields like table, mcp_host
            .finish_non_exhaustive()
    }
}

impl MemoryStore {
    /// Create a new MemoryStore instance, connecting to or creating the LanceDB table.
    pub async fn new(
        db_path: Option<PathBuf>,
        embedding_model: Option<EmbeddingModelVariant>, // Use type from schema module
        mcp_host: Option<Arc<dyn McpHostInterface + Send + Sync>>,
    ) -> Result<Self> {
        // Fix error handling for get_memory_db_path
        let db_path = match db_path {
            Some(p) => p,
            None => get_memory_db_path()?, // Use function from config module
        };
        ensure_memory_db_dir(&db_path)?; // Use function from config module
        let uri = db_path.to_str().context("Invalid DB path")?;

        // Use the provided embedding model variant or default to Base
        let embedding_model = embedding_model.unwrap_or_default();
        let embedding_dim = embedding_model.dimension();

        debug!("Connecting to LanceDB at: {}", uri);
        debug!("Using embedding model variant: {} ({} dimensions)", embedding_model.as_str(), embedding_dim);

        let conn = connect(uri)
            .execute()
            .await
            .context("Failed to connect to LanceDB")?;
        let conn: Arc<LanceConnection> = Arc::new(conn); // Use concrete type

        let table_name = "memories";
        let schema = create_schema(embedding_dim); // Use function from schema module

        let table_result = conn.open_table(table_name).execute().await;

        let table = match table_result {
            Ok(table) => {
                info!("Opened existing LanceDB table '{}'", table_name);
                let indices = table.list_indices().await.context("Failed to list indices")?;
                let fts_exists = indices.iter().any(|idx| idx.columns.contains(&"value".to_string()));
                if !fts_exists {
                    info!("FTS index on 'value' column not found. Skipping creation (feature unavailable in lancedb v0.4).");
                    warn!("Skipping FTS index creation on 'value': FtsIndexBuilder not found in lancedb v0.4.");
                } else {
                    debug!("FTS index on 'value' column already exists.");
                }
                Arc::new(table)
            }
            Err(lancedb::Error::TableNotFound { .. }) => {
                info!("Table '{}' not found, creating new table.", table_name);
                 let empty_batch = RecordBatch::new_empty(schema.clone());
                let initial_data = RecordBatchIterator::new(vec![Ok(empty_batch)], schema.clone());

                let table = conn
                    .create_table(table_name, Box::new(initial_data))
                    .execute()
                    .await
                    .context(format!("Failed to create LanceDB table '{}'", table_name))?;

                info!("Skipping FTS index creation on 'value' for new table (feature unavailable in lancedb v0.4).");
                warn!("Skipping FTS index creation on 'value' for new table: FtsIndexBuilder not found in lancedb v0.4.");

                Arc::new(table)
            }
            Err(e) => {
                return Err(e).context(format!("Failed to open LanceDB table '{}'", table_name));
            }
        };

        Ok(Self { table, schema, embedding_model, mcp_host, _db_path: db_path.to_str().unwrap().to_string(), _table_name: table_name.to_string() })
    }

    /// Add a new memory to the store. Generates a unique ID and embedding.
    /// Accepts optional metadata fields.
    pub async fn add_memory(
        &self,
        key: &str,
        value: &str,
        tags: Vec<String>,
        // Optional metadata parameters
        session_id: Option<String>,
        source: Option<String>,
        related_keys: Option<Vec<String>>,
        // confidence_score is typically derived from search, so not added here
    ) -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("SystemTime error: {}", e))?
            .as_secs();

        // Initialize with provided data (or None)
        let memory = Memory {
            key: key.to_string(),
            value: value.to_string(),
            timestamp,
            tags,
            token_count: None, // Calculated later
            session_id,        // Pass through optional metadata
            source,
            related_keys,
            confidence_score: None, // Not set during add/update
        };
        let id = Uuid::new_v4();

        // Define embedding generation closure with explicit type
        let generate_embedding_fn: Arc<dyn Fn(String, EmbeddingModelVariant, Option<Arc<dyn McpHostInterface + Send + Sync>>) -> Pin<Box<dyn Future<Output = Result<Vec<f32>, anyhow::Error>> + Send + 'static>> + Send + Sync> = Arc::new(
            move |text: String, model: EmbeddingModelVariant, host: Option<Arc<dyn McpHostInterface + Send + Sync>>| {
                 // Box the future immediately
                 Box::pin(async move {
                    let dimension = model.dimension();
                    if host.is_none() {
                        warn!("[Closure] No MCP host. Using zeros.");
                        return Ok(vec![0.0; dimension]);
                    }
                    let mcp = host.as_ref().unwrap();
                    let params = json!({ "text": text, "is_query": false, "variant": model.as_str() });
                    match mcp.execute_tool("embedding", "embed", params).await {
                        Ok(result) => {
                            let embedding = result.get("embedding").and_then(|v| v.as_array()).ok_or_else(|| anyhow!("Missing embedding"))?;
                            let embedding_vec: Vec<f32> = embedding.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect();
                            if embedding_vec.len() != dimension {
                                Err(anyhow!("Dim mismatch: got {}, expected {}", embedding_vec.len(), dimension))
                            } else {
                                Ok(embedding_vec)
                            }
                        },
                        Err(e) => {
                            warn!("[Closure] Failed embedding: {}. Using zeros.", e);
                            Ok(vec![0.0; dimension])
                        }
                    }
                 })
            }
        );

        // Generate embedding and convert to batch (token count calculated here)
        let batch = arrow_conversion::memories_to_batch_with_embeddings(
            vec![(id, memory)],
            self.schema.clone(),
            self.embedding_model,
            self.mcp_host.clone(),
            // Create a wrapping closure that matches the expected Fn(&str, ...) signature
            {
                let generator = generate_embedding_fn.clone(); // Clone Arc
                move |text: &str, model, host| { // Takes &str as expected
                    let text_owned = text.to_string(); // Convert to String
                    generator(text_owned, model, host) // Call the original Arc<dyn Fn(String, ...)> closure
                }
            }
        ).await?;

        // Correct: Wrap batch in Ok for the iterator
        let reader = RecordBatchIterator::new(vec![Ok(batch)], self.schema.clone());

        self.table
            .add(Box::new(reader))
            .execute()
            .await
            .context("Failed to add memory to LanceDB table")?;

        debug!("Added memory: {} = {}", key, value);
        Ok(())
    }

    /// Update an existing memory identified by its key.
    /// This is currently implemented as delete + add.
    /// Returns true if a matching key was found and updated, false otherwise.
    /// Propagates optional metadata to the underlying add_memory call.
    pub async fn update_memory(
        &self,
        key: &str,
        value: &str,
        tags: Vec<String>,
        // Optional metadata parameters
        session_id: Option<String>,
        source: Option<String>,
        related_keys: Option<Vec<String>>,
    ) -> Result<bool> {
        // First, try to delete existing memories with the key
        let delete_count = self.delete_by_key(key).await?;

        // Then, add the new version with metadata
        self.add_memory(key, value, tags, session_id, source, related_keys).await?;

        if delete_count > 0 {
            debug!("Updated memory with key: {}", key);
            Ok(true)
        } else {
            debug!("Added new memory (no existing key '{}' found for update)", key);
            Ok(false) // Indicate that it was an add, not an update of existing
        }
    }

    /// Execute a generic query against the store
    async fn execute_query<T: ExecutableQuery + Send + 'static>(&self, query: T) -> Result<BoxStream<'static, Result<RecordBatch, Error>>> {
        let stream = query.execute().await?;
        // Convert RecordBatchStream to our expected return type
        let mapped_stream = stream.map(|batch_result| {
            batch_result.map_err(|e| anyhow::Error::from(e))
        });
        Ok(Box::pin(mapped_stream))
    }

    /// Convert a stream of RecordBatches to a Vec<Memory>.
    async fn batch_stream_to_memories(&self, stream: BoxStream<'static, Result<RecordBatch, Error>>) -> Result<Vec<Memory>> {
        let batches = stream.try_collect::<Vec<_>>().await?;
        let mut all_memories = Vec::new();

        for batch in batches {
            // Use function from arrow_conversion module
            let memories = arrow_conversion::batch_to_memories(&batch)?;
            all_memories.extend(memories);
        }

        Ok(all_memories)
    }

    /// Get all memories with the specified key.
    pub async fn get_by_key(&self, key: &str) -> Result<Option<Memory>> {
        let filter = format!("key = '{}'", key.replace("'", "''"));
        let query = self.table.query().only_if(filter);
        let result = self.execute_query(query).await?;

        // Process results using batch_stream_to_memories which now calls arrow_conversion
        let memories = self.batch_stream_to_memories(result).await?;
        Ok(memories.into_iter().next())
    }

    /// Retrieve memories based on a specific tag.
    pub async fn get_by_tag(&self, tag: &str) -> Result<Vec<Memory>> {
        let filter = format!("tags LIKE '%{}%'", tag.replace("'", "''"));
        let query = self.table.query().only_if(filter);
        let result = self.execute_query(query).await?;

        self.batch_stream_to_memories(result).await
    }

    /// Get all memories in the store.
    pub async fn get_all_memories(&self) -> Result<Vec<Memory>> {
        let query = self.table.query();
        self.batch_stream_to_memories(self.execute_query(query).await?).await
    }

    /// Delete memories by key, returning the number of items removed.
    pub async fn delete_by_key(&self, key: &str) -> Result<usize> {
        // First, count how many match
        let filter = format!("key = '{}'", key.replace("'", "''"));
        let query = self.table.query().only_if(filter.clone());
        let matching_memories = self.execute_query(query).await?;

        // Collect all batches to count them
        let batches = matching_memories.try_collect::<Vec<_>>().await?;
        let count = batches.iter().map(|batch| batch.num_rows()).sum();

        if count > 0 {
            debug!("Attempting to delete {} memories with key '{}'", count, key);
            // Now delete using the same filter
            self.table
                .delete(&filter) // Pass the filter string
                .await
                .context(format!("Failed to delete memories with key '{}'", key))?;
            debug!("Successfully deleted memories with key '{}'", key);
        } else {
            debug!("No memories found with key '{}' to delete", key);
        }
        Ok(count)
    }

    /// Generate embedding for a query string.
    /// This is used for semantic search.
    async fn generate_query_embedding(&self, query_text: &str) -> Result<Vec<f32>> {
        let dimension = self.embedding_model.dimension();

        // If no MCP host is available, return zeros
        if self.mcp_host.is_none() {
            warn!("No MCP host available for query embedding generation. Using zeros.");
            return Ok(vec![0.0; dimension]);
        }

        let mcp = self.mcp_host.as_ref().unwrap();
        let _mcp_host_clone = self.mcp_host.clone(); // Add underscore prefix
        let _embedding_model_clone = self.embedding_model; // Add underscore prefix

        // Prepare the parameters for the embed tool - note is_query is true
        let params = json!({
            "text": query_text,
            "is_query": true, // This is a query, not a passage
            "variant": self.embedding_model.as_str()
        });

        // Call the embedding server's embed tool
        match mcp.execute_tool("embedding", "embed", params).await {
            Ok(result) => {
                // Extract the embedding vector from the result
                let embedding = result.get("embedding")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| anyhow::anyhow!("Missing or invalid embedding in response"))?;

                // Convert to Vec<f32>
                let embedding_vec: Vec<f32> = embedding.iter()
                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                    .collect();

                // Verify dimension
                if embedding_vec.len() != dimension {
                    return Err(anyhow::anyhow!(
                        "Unexpected embedding dimension: got {}, expected {}",
                        embedding_vec.len(),
                        dimension
                    ));
                }

                Ok(embedding_vec)
            },
            Err(e) => {
                warn!("Failed to generate query embedding: {}. Using zeros instead.", e);
                // Fallback to zeros on error
                Ok(vec![0.0; dimension])
            }
        }
    }

    /// Retrieve memories semantically similar to the query text.
    pub async fn get_semantically_similar(
        &self,
        query_text: &str,
        top_k: usize,
        min_relevance_score: f32,
    ) -> Result<Vec<(Memory, f32)>> {
        // Generate embedding for the query
        let query_vector = self.generate_query_embedding(query_text).await?;

        // If all zeros (likely no MCP), warn and return empty
        if query_vector.iter().all(|&v| v == 0.0) {
            warn!("Query embedding is all zeros, semantic search will be ineffective.");
            return Ok(Vec::new());
        }

        // Create a query for vector search with proper error handling
        let vector_query = match self.table.query()
            .limit(top_k)
            .nearest_to(query_vector) {
                Ok(q) => q,
                Err(e) => return Err(anyhow!("Failed to create vector query: {}", e))
            };

        // Execute the query
        let result = self.execute_query(vector_query).await?;
        let mut memories_with_scores = Vec::new();

        // Process the search results
        let batches = result.try_collect::<Vec<_>>().await?;

        for batch in batches {
            if let Some(distance_array) = batch.column_by_name("_distance") {
                let distances = distance_array
                    .as_any()
                    .downcast_ref::<Float32Array>()
                    .ok_or_else(|| anyhow!("Expected distance array to be Float32Array"))?;

                // Use function from arrow_conversion module
                let memories = arrow_conversion::batch_to_memories(&batch)?;

                for (i, memory) in memories.into_iter().enumerate() {
                    let distance = distances.value(i);
                    // Calculate similarity score (convert distance to similarity)
                    let similarity_score = 1.0 / (1.0 + distance);

                    if similarity_score >= min_relevance_score {
                        memories_with_scores.push((memory, similarity_score));
                    }
                }
            }
        }

        // Sort by similarity score (descending)
        memories_with_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        Ok(memories_with_scores)
    }

    /// Searches memories combining vector similarity and filtering.
    pub async fn search_memories(
        &self,
        query_text: Option<&str>,
        filter_tags: Option<&Vec<String>>,
        top_k: usize,
        start_time: Option<SystemTime>,
        end_time: Option<SystemTime>,
    ) -> Result<Vec<Memory>> {
        // Start with a basic query
        let mut query = self.table.query().limit(top_k);

        // Build filter expression if needed
        let mut filters = Vec::new();

        // Add tag filters if any
        if let Some(tags) = filter_tags {
            if !tags.is_empty() {
                let tag_filters: Vec<String> = tags
                    .iter()
                    .map(|tag| format!("array_has(tags, '{}')", tag.replace("'", "''")))
                    .collect();
                filters.push(format!("({})", tag_filters.join(" AND ")));
            }
        }

        // Add time range filters if any
        match (start_time, end_time) {
            (Some(start), Some(end)) => {
                let start_ts = start.duration_since(UNIX_EPOCH)?.as_secs() as i64;
                let end_ts = end.duration_since(UNIX_EPOCH)?.as_secs() as i64;
                if start_ts > end_ts {
                    return Err(anyhow::anyhow!("Start time must be before end time in search"));
                }
                filters.push(format!("(timestamp >= {} AND timestamp <= {})", start_ts, end_ts));
            },
            (Some(start), None) => {
                let start_ts = start.duration_since(UNIX_EPOCH)?.as_secs() as i64;
                filters.push(format!("timestamp >= {}", start_ts));
            },
            (None, Some(end)) => {
                let end_ts = end.duration_since(UNIX_EPOCH)?.as_secs() as i64;
                filters.push(format!("timestamp <= {}", end_ts));
            },
            (None, None) => {}
        }

        // Apply filter if we have one
        if !filters.is_empty() {
            let filter = filters.join(" AND ");
            debug!("Applying filter: {}", filter);
            query = query.only_if(filter);
        }

        // Apply vector search if needed
        if let Some(text) = query_text {
            debug!("Performing vector search for: \"{}\"", text);
            let query_vector = self.generate_query_embedding(text).await?;

            // Check for zero vector
            if !query_vector.iter().all(|&v| v == 0.0) {
                // Apply vector search with proper error handling
                // Use clone().nearest_to to avoid moving the query when we need fallback
                match query.clone().nearest_to(query_vector) {
                    Ok(vector_query) => {
                        let result = self.execute_query(vector_query).await?;
                        return self.batch_stream_to_memories(result).await;
                    },
                    Err(e) => {
                        warn!("Could not create vector query, falling back to filter only: {}", e);
                        // Continue with regular query
                    }
                }
            } else {
                warn!("Query embedding is all zeros, vector search will be ineffective");
                // If no filters and zero vector, return empty
                if filters.is_empty() {
                    return Ok(Vec::new());
                }
            }
        }

        debug!("Executing final search query");
        let result = self.execute_query(query).await?;
        self.batch_stream_to_memories(result).await
    }

    /// Retrieves memories created or updated within the specified duration from the present time.
    pub async fn get_recent(&self, duration: Duration) -> Result<Vec<Memory>> {
        let now = SystemTime::now();
        let cutoff_time = now.checked_sub(duration)
            .context("Failed to calculate cutoff time")?;
        let cutoff_timestamp = cutoff_time.duration_since(UNIX_EPOCH)?.as_secs() as i64;

        debug!("Getting recent memories created after timestamp: {}", cutoff_timestamp);
        let filter = format!("timestamp >= {}", cutoff_timestamp);
        let query = self.table.query().only_if(filter);
        let result = self.execute_query(query).await?;
        self.batch_stream_to_memories(result).await
    }

    /// Retrieves memories created or updated within the specified time range.
    pub async fn get_in_range(&self, start: SystemTime, end: SystemTime) -> Result<Vec<Memory>> {
        let start_timestamp = start.duration_since(UNIX_EPOCH)?.as_secs() as i64;
        let end_timestamp = end.duration_since(UNIX_EPOCH)?.as_secs() as i64;

        if start_timestamp > end_timestamp {
            return Err(anyhow::anyhow!("Start time must be before end time"));
        }

        debug!("Getting memories in range: {} - {}", start_timestamp, end_timestamp);
        let filter = format!("timestamp >= {} AND timestamp <= {}", start_timestamp, end_timestamp);
        let query = self.table.query().only_if(filter);
        let result = self.execute_query(query).await?;
        self.batch_stream_to_memories(result).await
    }

    /// Exports all memories in the store as a JSON string.
    pub async fn export_all_memories_json(&self) -> Result<String> {
        let all_memories = self.get_all_memories().await?;
        serde_json::to_string_pretty(&all_memories)
            .context("Failed to serialize memories to JSON")
    }
}

// We might need to add tests here later to verify LanceDB integration
