use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub ida_socket_path: String,
    pub llm_endpoint: String,
    // Add other config fields as needed
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            ida_socket_path: "/tmp/ida.sock".to_string(), // Sensible default?
            llm_endpoint: "http://localhost:8080".to_string(), // Placeholder
        }
    }
}
