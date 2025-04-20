use gemini_ipc::internal_messages::{ConversationTurn, InternalMessage, MemoryItem};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::UnixStream;
use tracing::{debug, error, info, warn};

const MAX_RETRIES: u32 = 3;
const RETRY_DELAY: Duration = Duration::from_millis(500);

#[derive(Error, Debug)]
pub enum IdaClientError {
    #[error("Failed to connect to IDA socket after {MAX_RETRIES} retries: {0}")]
    ConnectionFailed(#[source] io::Error),
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

pub struct IdaClient {
    // Use BufReader/BufWriter for potentially better performance with framed messages
    reader: BufReader<tokio::net::unix::ReadHalf<'static>>,
    writer: BufWriter<tokio::net::unix::WriteHalf<'static>>,
}

impl IdaClient {
    pub async fn connect(socket_path: &str) -> Result<Self> {
        let mut last_error = None;
        for attempt in 0..MAX_RETRIES {
            match UnixStream::connect(socket_path).await {
                Ok(stream) => {
                    info!(
                        attempt,
                        "Successfully connected to IDA socket {}", socket_path
                    );
                    // Need to Box the halves to erase the lifetime bounds
                    let (reader, writer) = stream.into_split();
                    let reader_boxed = Box::new(reader);
                    let writer_boxed = Box::new(writer);
                    // SAFETY: This is safe because we own the stream and its halves.
                    // We convert to static lifetimes to store them in the struct.
                    // The struct's lifetime dictates the actual validity.
                    let reader_static = unsafe {
                        std::mem::transmute::<_, tokio::net::unix::ReadHalf<'static>>(reader_boxed)
                    };
                    let writer_static = unsafe {
                        std::mem::transmute::<_, tokio::net::unix::WriteHalf<'static>>(writer_boxed)
                    };

                    return Ok(IdaClient {
                        reader: BufReader::new(reader_static),
                        writer: BufWriter::new(writer_static),
                    });
                }
                Err(e) => {
                    warn!(
                        attempt,
                        error = %e,
                        "Failed to connect to IDA socket {}, retrying in {:?}...",
                        socket_path,
                        RETRY_DELAY
                    );
                    last_error = Some(e);
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
        Err(IdaClientError::ConnectionFailed(last_error.unwrap_or_else(
            || {
                io::Error::new(
                    io::ErrorKind::Other,
                    "Connection attempt failed without error",
                )
            },
        )))
    }

    async fn send_message(&mut self, message: &InternalMessage) -> Result<()> {
        let serialized = serde_json::to_vec(message)?;
        debug!(size = serialized.len(), "Sending message to IDA");

        // Prepend message length (as u32)
        let len_bytes = (serialized.len() as u32).to_be_bytes();
        self.writer.write_all(&len_bytes).await?;
        self.writer.write_all(&serialized).await?;
        self.writer.flush().await?;
        Ok(())
    }

    async fn receive_message(&mut self) -> Result<InternalMessage> {
        // Read message length (u32)
        let mut len_bytes = [0u8; 4];
        self.reader.read_exact(&mut len_bytes).await?;
        let len = u32::from_be_bytes(len_bytes) as usize;
        debug!(len, "Expecting message of length");

        // Read the message body
        let mut buffer = vec![0u8; len];
        self.reader.read_exact(&mut buffer).await?;
        debug!(size = buffer.len(), "Received message data from IDA");

        // Deserialize
        let message: InternalMessage = serde_json::from_slice(&buffer)?;
        Ok(message)
    }

    pub async fn get_memories(&mut self, query: &str) -> Result<Vec<MemoryItem>> {
        let request = InternalMessage::GetMemoriesRequest {
            query: query.to_string(),
        };
        self.send_message(&request).await?;

        match self.receive_message().await? {
            InternalMessage::GetMemoriesResponse { memories } => Ok(memories),
            _ => Err(IdaClientError::UnexpectedResponse),
        }
    }

    pub async fn store_turn_async(&mut self, turn_data: ConversationTurn) -> Result<()> {
        let request = InternalMessage::StoreTurnRequest { turn_data };
        // Send and forget (no response expected for this message type)
        self.send_message(&request).await
    }

    // Optional: Method to explicitly close the connection or handle disconnection
    // pub async fn close(mut self) -> Result<()> {
    //     self.writer.shutdown().await?;
    //     Ok(())
    // }
}
