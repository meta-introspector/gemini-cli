use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use async_trait::async_trait;
use chrono::Utc;
use log::{debug, warn};

use crate::session::store::{Session, SessionStore, SessionStoreError};

/// In-memory implementation of SessionStore
#[derive(Debug)]
pub struct InMemorySessionStore {
    /// Thread-safe storage of sessions
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl InMemorySessionStore {
    /// Create a new InMemorySessionStore
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl SessionStore for InMemorySessionStore {
    async fn create_session(&self, id: String) -> Result<Session, SessionStoreError> {
        let session = Session::new(id.clone());
        
        let mut sessions = self.sessions.write().map_err(|e| {
            SessionStoreError::StorageError(format!("Failed to acquire write lock: {}", e))
        })?;
        
        sessions.insert(id, session.clone());
        debug!("Created session: {}", session.id);
        
        Ok(session)
    }
    
    async fn get_session(&self, id: &str) -> Result<Session, SessionStoreError> {
        let sessions = self.sessions.read().map_err(|e| {
            SessionStoreError::StorageError(format!("Failed to acquire read lock: {}", e))
        })?;
        
        let session = sessions.get(id).cloned().ok_or_else(|| {
            SessionStoreError::NotFound(id.to_string())
        })?;
        
        // Check if session has expired
        if session.is_expired() {
            return Err(SessionStoreError::NotFound(format!("Session expired: {}", id)));
        }
        
        Ok(session)
    }
    
    async fn save_session(&self, session: Session) -> Result<(), SessionStoreError> {
        let mut sessions = self.sessions.write().map_err(|e| {
            SessionStoreError::StorageError(format!("Failed to acquire write lock: {}", e))
        })?;
        
        sessions.insert(session.id.clone(), session);
        Ok(())
    }
    
    async fn delete_session(&self, id: &str) -> Result<(), SessionStoreError> {
        let mut sessions = self.sessions.write().map_err(|e| {
            SessionStoreError::StorageError(format!("Failed to acquire write lock: {}", e))
        })?;
        
        if sessions.remove(id).is_none() {
            return Err(SessionStoreError::NotFound(id.to_string()));
        }
        
        debug!("Deleted session: {}", id);
        Ok(())
    }
    
    async fn cleanup_expired_sessions(&self) -> Result<usize, SessionStoreError> {
        let mut sessions = self.sessions.write().map_err(|e| {
            SessionStoreError::StorageError(format!("Failed to acquire write lock: {}", e))
        })?;
        
        let now = Utc::now();
        let expired_ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| {
                if let Some(expires_at) = session.expires_at {
                    expires_at < now
                } else {
                    false
                }
            })
            .map(|(id, _)| id.clone())
            .collect();
        
        let count = expired_ids.len();
        for id in expired_ids {
            sessions.remove(&id);
            debug!("Cleaned up expired session: {}", id);
        }
        
        if count > 0 {
            warn!("Cleaned up {} expired sessions", count);
        }
        
        Ok(count)
    }

    async fn list_sessions(&self) -> Result<Vec<Session>, SessionStoreError> {
        let sessions = self.sessions.read().map_err(|e| {
            SessionStoreError::StorageError(format!("Failed to acquire read lock: {}", e))
        })?;
        
        let now = Utc::now();
        let active_sessions: Vec<Session> = sessions
            .values()
            .filter(|session| {
                match session.expires_at {
                    Some(expires_at) => expires_at > now,
                    None => true // No expiry, always active
                }
            })
            .cloned()
            .collect();
        
        debug!("Listed {} active sessions", active_sessions.len());
        Ok(active_sessions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use tokio::test;

    #[test]
    async fn test_create_and_get_session() {
        let store = InMemorySessionStore::new();
        let session_id = "test_session_1".to_string();
        
        let session = store.create_session(session_id.clone()).await.unwrap();
        assert_eq!(session.id, session_id);
        
        let retrieved = store.get_session(&session_id).await.unwrap();
        assert_eq!(retrieved.id, session_id);
    }
    
    #[test]
    async fn test_session_data() {
        let store = InMemorySessionStore::new();
        let session_id = "test_session_2".to_string();
        
        let mut session = store.create_session(session_id.clone()).await.unwrap();
        session.set("key1".to_string(), "value1".to_string());
        session.set("key2".to_string(), "value2".to_string());
        
        store.save_session(session).await.unwrap();
        
        let retrieved = store.get_session(&session_id).await.unwrap();
        assert_eq!(retrieved.get("key1"), Some(&"value1".to_string()));
        assert_eq!(retrieved.get("key2"), Some(&"value2".to_string()));
        assert_eq!(retrieved.get("key3"), None);
    }
    
    #[test]
    async fn test_delete_session() {
        let store = InMemorySessionStore::new();
        let session_id = "test_session_3".to_string();
        
        let session = store.create_session(session_id.clone()).await.unwrap();
        assert_eq!(session.id, session_id);
        
        // Delete the session
        store.delete_session(&session_id).await.unwrap();
        
        // Verify it's gone
        let result = store.get_session(&session_id).await;
        assert!(matches!(result, Err(SessionStoreError::NotFound(_))));
    }
    
    #[test]
    async fn test_session_expiry() {
        let store = InMemorySessionStore::new();
        let session_id = "test_session_4".to_string();
        
        let mut session = store.create_session(session_id.clone()).await.unwrap();
        
        // Set session to expire in the past
        let expiry = Utc::now() - Duration::seconds(1);
        session.set_expiry(expiry);
        store.save_session(session).await.unwrap();
        
        // Verify it's considered expired
        let result = store.get_session(&session_id).await;
        assert!(matches!(result, Err(SessionStoreError::NotFound(_))));
        
        // Clean up expired sessions
        let cleaned = store.cleanup_expired_sessions().await.unwrap();
        assert_eq!(cleaned, 1);
        
        // Verify it's gone after cleanup
        let result = store.get_session(&session_id).await;
        assert!(matches!(result, Err(SessionStoreError::NotFound(_))));
    }
}
