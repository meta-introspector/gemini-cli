use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Error type for session store operations
#[derive(Debug)]
pub enum SessionStoreError {
    /// Session not found
    NotFound(String),
    /// Error occurred during a store operation
    StorageError(String),
}

impl Display for SessionStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStoreError::NotFound(id) => write!(f, "Session not found: {}", id),
            SessionStoreError::StorageError(msg) => write!(f, "Storage error: {}", msg),
        }
    }
}

impl Error for SessionStoreError {}

/// Session data structure
#[derive(Debug, Clone)]
pub struct Session {
    /// Unique session identifier
    pub id: String,
    /// When the session was created
    pub created_at: DateTime<Utc>,
    /// Last time the session was accessed or modified
    pub updated_at: DateTime<Utc>,
    /// Optional time when the session expires
    pub expires_at: Option<DateTime<Utc>>,
    /// Custom session data stored as key-value pairs
    pub data: HashMap<String, String>,
}

impl Session {
    /// Create a new session with the given ID
    pub fn new(id: String) -> Self {
        let now = Utc::now();
        Self {
            id,
            created_at: now,
            updated_at: now,
            expires_at: None,
            data: HashMap::new(),
        }
    }

    /// Set a key-value pair in the session data
    pub fn set(&mut self, key: String, value: String) {
        self.data.insert(key, value);
        self.updated_at = Utc::now();
    }

    /// Get a value from the session data by key
    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    /// Remove a key-value pair from the session data
    pub fn remove(&mut self, key: &str) -> Option<String> {
        let result = self.data.remove(key);
        if result.is_some() {
            self.updated_at = Utc::now();
        }
        result
    }

    /// Check if the session has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Set the expiration time for this session
    pub fn set_expiry(&mut self, expires_at: DateTime<Utc>) {
        self.expires_at = Some(expires_at);
        self.updated_at = Utc::now();
    }
}

/// Trait defining the interface for session stores
#[async_trait]
pub trait SessionStore: Send + Sync + Debug {
    /// Create a new session with the given ID
    async fn create_session(&self, id: String) -> Result<Session, SessionStoreError>;
    
    /// Get a session by ID
    async fn get_session(&self, id: &str) -> Result<Session, SessionStoreError>;
    
    /// Save changes to a session
    async fn save_session(&self, session: Session) -> Result<(), SessionStoreError>;
    
    /// Delete a session by ID
    async fn delete_session(&self, id: &str) -> Result<(), SessionStoreError>;
    
    /// Delete expired sessions
    async fn cleanup_expired_sessions(&self) -> Result<usize, SessionStoreError>;
    
    /// List all active (non-expired) sessions
    async fn list_sessions(&self) -> Result<Vec<Session>, SessionStoreError>;
}

/// Type alias for Arc-wrapped SessionStore trait objects
pub type SessionStoreRef = Arc<dyn SessionStore>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    use chrono::Duration as ChronoDuration;

    #[test]
    fn test_session_creation() {
        let id = "test_id".to_string();
        let session = Session::new(id.clone());
        
        assert_eq!(session.id, id);
        assert!(session.data.is_empty());
        assert_eq!(session.expires_at, None);
        assert!(!session.is_expired());
    }
    
    #[test]
    fn test_session_data_operations() {
        let mut session = Session::new("test_id".to_string());
        
        // Test setting data
        session.set("key1".to_string(), "value1".to_string());
        session.set("key2".to_string(), "value2".to_string());
        
        // Test getting data
        assert_eq!(session.get("key1"), Some(&"value1".to_string()));
        assert_eq!(session.get("key2"), Some(&"value2".to_string()));
        assert_eq!(session.get("nonexistent"), None);
        
        // Test updating data
        session.set("key1".to_string(), "updated".to_string());
        assert_eq!(session.get("key1"), Some(&"updated".to_string()));
        
        // Test removing data
        let removed = session.remove("key1");
        assert_eq!(removed, Some("updated".to_string()));
        assert_eq!(session.get("key1"), None);
        
        // Test removing non-existent data
        let removed = session.remove("nonexistent");
        assert_eq!(removed, None);
    }
    
    #[test]
    fn test_session_expiry() {
        let mut session = Session::new("test_id".to_string());
        
        // Set expiry in the future
        let future = Utc::now() + ChronoDuration::seconds(60);
        session.set_expiry(future);
        
        assert_eq!(session.expires_at, Some(future));
        assert!(!session.is_expired());
        
        // Set expiry in the past
        let past = Utc::now() - ChronoDuration::seconds(1);
        session.set_expiry(past);
        
        assert_eq!(session.expires_at, Some(past));
        assert!(session.is_expired());
    }
    
    #[test]
    fn test_updated_at_changes() {
        let mut session = Session::new("test_id".to_string());
        let initial_updated_at = session.updated_at;
        
        // Wait a tiny bit
        thread::sleep(Duration::from_millis(5));
        
        // Setting data should update the timestamp
        session.set("key".to_string(), "value".to_string());
        assert!(session.updated_at > initial_updated_at);
        
        let after_set = session.updated_at;
        thread::sleep(Duration::from_millis(5));
        
        // Removing data should update the timestamp
        session.remove("key");
        assert!(session.updated_at > after_set);
        
        let after_remove = session.updated_at;
        thread::sleep(Duration::from_millis(5));
        
        // Setting expiry should update the timestamp
        session.set_expiry(Utc::now() + ChronoDuration::minutes(5));
        assert!(session.updated_at > after_remove);
    }
}
