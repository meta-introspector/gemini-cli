//! Session management for HAPPE
//!
//! This module provides session management capabilities, allowing HAPPE to maintain state
//! across multiple requests from the same client. It defines a `SessionStore` trait that
//! can be implemented by different storage backends.

pub mod adapters;
pub mod store;

pub use adapters::InMemorySessionStore;
pub use store::{Session, SessionStore, SessionStoreError, SessionStoreRef};
