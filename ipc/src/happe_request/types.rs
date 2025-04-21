use serde::{Deserialize, Serialize};

/// A request from a client to the HAPPE daemon
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HappeQueryRequest {
    /// The query text from the user/client
    pub query: String,
    /// Optional session ID to maintain conversation context
    pub session_id: Option<String>,
}

/// A response from the HAPPE daemon to a client
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HappeQueryResponse {
    /// The response text to send back to the user/client
    pub response: String,
    /// Optional error message if something went wrong
    pub error: Option<String>,
    /// Session ID used for this conversation
    pub session_id: Option<String>,
}
