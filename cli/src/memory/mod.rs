// Memory broker integration for Gemini CLI
use gemini_memory::MemoryStore;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use tokio::task;
use std::time::Duration;
use std::error::Error;
use serde_json::Value;
use async_trait::async_trait;
use log::{info, error, warn};

mod types;
pub use types::*;

// Maximum number of embedding jobs that can be queued
const DEFAULT_QUEUE_SIZE: usize = 100;
// Default worker count for processing embedding jobs
const DEFAULT_WORKER_COUNT: usize = 2;

// An embedding job to be processed asynchronously
#[derive(Debug, Clone)]
struct EmbeddingJob {
    key: String,
    value: String,
    tags: Vec<String>,
    namespace: Option<String>,
    source: Option<String>,
    metadata: Option<Value>,
}

// AsyncMemoryStore wraps a regular MemoryStore and provides async embedding capabilities
pub struct AsyncMemoryStore {
    inner: Arc<MemoryStore>,
    job_queue: Arc<Mutex<VecDeque<EmbeddingJob>>>,
    queue_size: usize,
    is_async: bool,
}

impl AsyncMemoryStore {
    // Create a new AsyncMemoryStore with the given configuration
    pub async fn new(
        memory_store: MemoryStore,
        queue_size: Option<usize>,
        worker_count: Option<usize>,
        is_async: bool,
    ) -> Result<Self, Box<dyn Error>> {
        // Default or provided queue size
        let queue_size = queue_size.unwrap_or(DEFAULT_QUEUE_SIZE);
        
        // Create a thread-safe queue
        let job_queue = Arc::new(Mutex::new(VecDeque::with_capacity(queue_size)));
        
        // Number of worker tasks
        let worker_count = worker_count.unwrap_or(DEFAULT_WORKER_COUNT);

        // Wrap MemoryStore in Arc for thread-safe sharing
        let store_arc = Arc::new(memory_store);
        
        if is_async {
            // Start worker tasks for processing embedding jobs
            for worker_id in 0..worker_count {
                let worker_store = Arc::clone(&store_arc);
                let worker_queue = Arc::clone(&job_queue);
                
                task::spawn(async move {
                    info!("Embedding worker {} started", worker_id);
                    
                    loop {
                        // Get a job from the queue
                        let job = {
                            let mut queue = worker_queue.lock().unwrap();
                            queue.pop_front()
                        };
                        
                        match job {
                            Some(job) => {
                                info!("Worker {} processing embedding job for key: {}", worker_id, job.key);
                                
                                match worker_store.add_memory(
                                    &job.key,
                                    &job.value,
                                    job.tags.clone(),
                                    job.namespace.clone(),
                                    job.source.clone(),
                                    job.metadata.clone(),
                                ).await {
                                    Ok(_) => info!("Worker {} successfully embedded memory: {}", worker_id, job.key),
                                    Err(e) => error!("Worker {} failed to embed memory: {} - Error: {}", worker_id, job.key, e),
                                }
                            },
                            None => {
                                // No jobs in the queue, sleep a bit
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }
                });
            }
            
            info!("Started {} async embedding workers with queue size {}", worker_count, queue_size);
        }
        
        Ok(Self {
            inner: store_arc,
            job_queue,
            queue_size,
            is_async,
        })
    }
    
    // Queue an embedding job to be processed asynchronously
    pub async fn queue_memory(
        &self,
        key: &str,
        value: &str,
        tags: Vec<String>,
        namespace: Option<String>,
        source: Option<String>,
        metadata: Option<Value>,
    ) -> Result<(), Box<dyn Error>> {
        let job = EmbeddingJob {
            key: key.to_string(),
            value: value.to_string(),
            tags,
            namespace,
            source,
            metadata,
        };
        
        // Add to queue if there's space
        let queue_full = {
            let mut queue = self.job_queue.lock().unwrap();
            let is_full = queue.len() >= self.queue_size;
            
            if !is_full {
                queue.push_back(job);
            }
            
            is_full
        };
        
        if queue_full {
            warn!("Embedding queue is full, dropping job for key: {}", key);
            // We treat a full queue as non-fatal
            Ok(())
        } else {
            info!("Queued embedding job for key: {}", key);
            Ok(())
        }
    }
}

// Implement methods to delegate to the inner MemoryStore
#[async_trait]
impl MemoryAccessor for AsyncMemoryStore {
    async fn add_memory(
        &self,
        key: &str,
        value: &str,
        tags: Vec<String>,
        namespace: Option<String>,
        source: Option<String>,
        metadata: Option<Value>,
    ) -> Result<(), Box<dyn Error>> {
        if self.is_async {
            // Queue the operation to be processed asynchronously
            self.queue_memory(key, value, tags, namespace, source, metadata).await
        } else {
            // Process synchronously
            self.inner.add_memory(key, value, tags, namespace, source, metadata).await
        }
    }
    
    async fn query_memories(
        &self,
        query: &str,
        namespace: Option<String>,
        limit: Option<usize>,
        min_relevance: Option<f32>,
    ) -> Result<Vec<Memory>, Box<dyn Error>> {
        // Query operations are always synchronous
        self.inner.query_memories(query, namespace, limit, min_relevance).await
    }
    
    async fn delete_memory(&self, key: &str) -> Result<bool, Box<dyn Error>> {
        self.inner.delete_memory(key).await
    }
    
    async fn get_memory(&self, key: &str) -> Result<Option<Memory>, Box<dyn Error>> {
        self.inner.get_memory(key).await
    }
    
    async fn list_memories(
        &self,
        namespace: Option<String>,
        limit: Option<usize>,
    ) -> Result<Vec<Memory>, Box<dyn Error>> {
        self.inner.list_memories(namespace, limit).await
    }
}

// Trait to define common memory operations
#[async_trait]
pub trait MemoryAccessor: Send + Sync {
    async fn add_memory(
        &self,
        key: &str,
        value: &str,
        tags: Vec<String>,
        namespace: Option<String>,
        source: Option<String>,
        metadata: Option<Value>,
    ) -> Result<(), Box<dyn Error>>;
    
    async fn query_memories(
        &self,
        query: &str,
        namespace: Option<String>,
        limit: Option<usize>,
        min_relevance: Option<f32>,
    ) -> Result<Vec<Memory>, Box<dyn Error>>;
    
    async fn delete_memory(&self, key: &str) -> Result<bool, Box<dyn Error>>;
    
    async fn get_memory(&self, key: &str) -> Result<Option<Memory>, Box<dyn Error>>;
    
    async fn list_memories(
        &self,
        namespace: Option<String>,
        limit: Option<usize>,
    ) -> Result<Vec<Memory>, Box<dyn Error>>;
} 