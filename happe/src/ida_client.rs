use gemini_ipc::internal_messages::{ConversationTurn, InternalMessage, MemoryItem};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::UnixStream;
use tracing::{debug, error, warn};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2); // Timeout for establishing connection
                                                          // Keep retry logic for initial connection attempt within a single request cycle
const MAX_RETRIES: u32 = 1; // Reduce retries for per-request connections
const RETRY_DELAY: Duration = Duration::from_millis(100);

#[derive(Error, Debug)]
pub enum IdaClientError {
    #[error("Failed to connect to IDA socket {0}: {1}")]
    ConnectionFailed(String, #[source] io::Error),
    #[error("Connection attempt timed out after {0:?}")]
    ConnectionTimeout(Duration),
    #[error("IO error during communication with IDA: {0}")]
    Io(#[from] io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Received unexpected response type from IDA")]
    UnexpectedResponse,
}

type Result<T> = std::result::Result<T, IdaClientError>;

// Alias io::Error for clarity
use std::io;

// IdaClient struct is now simplified or potentially unnecessary, keeping it for now as a namespace.
pub struct IdaClient {}

impl IdaClient {
    // Helper function to connect with retries and timeout
    async fn connect_with_retry(socket_path: &str) -> Result<UnixStream> {
        let mut last_error = None;
        for attempt in 0..=MAX_RETRIES {
            match tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(socket_path)).await {
                Ok(Ok(stream)) => {
                    debug!("Connected to IDA socket {}", socket_path);
                    return Ok(stream);
                }
                Ok(Err(e)) => {
                    warn!(
                        attempt,
                        error = %e,
                        "Failed to connect to IDA socket {} (attempt {}/{}), retrying in {:?}...",
                        socket_path,
                        attempt,
                        MAX_RETRIES,
                        RETRY_DELAY
                    );
                    last_error = Some(e);
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(RETRY_DELAY).await;
                    }
                }
                Err(_) => {
                    // Timeout error
                    return Err(IdaClientError::ConnectionTimeout(CONNECT_TIMEOUT));
                }
            }
        }
        Err(IdaClientError::ConnectionFailed(
            socket_path.to_string(),
            last_error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Other,
                    "Connection attempt failed without specific error",
                )
            }),
        ))
    }

    // Static method: Connects, sends, receives, disconnects.
    pub async fn get_memories(
        socket_path: &str, 
        query: &str,
        conversation_context: Option<String>
    ) -> Result<Vec<MemoryItem>> {
        let stream = Self::connect_with_retry(socket_path).await?;
        let (reader, writer) = stream.into_split();
        let mut buf_reader = BufReader::new(reader);
        let mut buf_writer = BufWriter::new(writer);

        // Log before moving the value
        let has_context = conversation_context.is_some();
        
        // Send request
        let request = InternalMessage::GetMemoriesRequest {
            query: query.to_string(),
            conversation_context, // Include the conversation context
        };
        let serialized = serde_json::to_vec(&request)?;
        let len_bytes = (serialized.len() as u32).to_be_bytes();
        buf_writer.write_all(&len_bytes).await?;
        buf_writer.write_all(&serialized).await?;
        buf_writer.flush().await?;
        debug!(query, has_context, "Sent GetMemoriesRequest");

        // Receive response
        let mut len_bytes_resp = [0u8; 4];
        buf_reader.read_exact(&mut len_bytes_resp).await?;
        let len_resp = u32::from_be_bytes(len_bytes_resp) as usize;
        let mut buffer_resp = vec![0u8; len_resp];
        buf_reader.read_exact(&mut buffer_resp).await?;
        let response: InternalMessage = serde_json::from_slice(&buffer_resp)?;
        debug!("Received response from IDA");

        match response {
            InternalMessage::GetMemoriesResponse { memories } => Ok(memories),
            _ => Err(IdaClientError::UnexpectedResponse),
        }
        // Connection automatically closed when stream/reader/writer go out of scope
    }

    // Static method: Connects, sends, disconnects.
    pub async fn store_turn_async(socket_path: &str, turn_data: ConversationTurn) -> Result<()> {
        let stream = Self::connect_with_retry(socket_path).await?;
        let (_reader, writer) = stream.into_split(); // Don't need reader
        let mut buf_writer = BufWriter::new(writer);

        let request = InternalMessage::StoreTurnRequest { turn_data };
        let serialized = serde_json::to_vec(&request)?;
        let len_bytes = (serialized.len() as u32).to_be_bytes();
        buf_writer.write_all(&len_bytes).await?;
        buf_writer.write_all(&serialized).await?;
        buf_writer.flush().await?;
        debug!("Sent StoreTurnRequest asynchronously");
        Ok(())
        // Connection automatically closed when stream/writer go out of scope
    }
}
