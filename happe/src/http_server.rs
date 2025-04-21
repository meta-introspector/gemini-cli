use crate::coordinator;
use crate::mcp_client::McpHostClient;
use gemini_core::config::HappeConfig;
use crate::session::{InMemorySessionStore, Session, SessionStoreRef};
use axum::{
    extract::{Extension, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use gemini_core::client::GeminiClient;
use gemini_ipc::internal_messages::ConversationTurn;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, error, info};
use uuid::Uuid;

/// Application state shared with all routes
#[derive(Clone)]
pub struct AppState {
    config: Arc<HappeConfig>,
    gemini_client: Arc<GeminiClient>,
    mcp_client: Arc<McpHostClient>,
    session_store: SessionStoreRef,
}

/// Request model for queries
#[derive(Deserialize)]
pub struct QueryRequest {
    query: String,
    #[serde(default)]
    session_id: Option<String>,
}

/// Response model for queries
#[derive(Serialize)]
pub struct QueryResponse {
    response: String,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Error type for HTTP server
#[derive(Debug)]
pub enum ApiError {
    InternalError(anyhow::Error),
    SessionError(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::InternalError(e) => {
                error!(error = %e, "Internal server error");
                let body = Json(QueryResponse {
                    response: String::new(),
                    session_id: String::new(),
                    error: Some(format!("Internal server error: {}", e)),
                });
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            },
            Self::SessionError(e) => {
                error!(error = %e, "Session error");
                let body = Json(QueryResponse {
                    response: String::new(),
                    session_id: String::new(),
                    error: Some(format!("Session error: {}", e)),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
        }
    }
}

/// Start the HTTP server
pub async fn run_server(
    config: HappeConfig,
    gemini_client: GeminiClient,
    mcp_client: McpHostClient,
    addr: SocketAddr,
) -> anyhow::Result<()> {
    info!("Starting HTTP server on {}", addr);

    // Create a session store
    let session_store = Arc::new(InMemorySessionStore::new()) as SessionStoreRef;

    // Create shared state
    let state = AppState {
        config: Arc::new(config),
        gemini_client: Arc::new(gemini_client),
        mcp_client: Arc::new(mcp_client),
        session_store,
    };

    // Set up CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build the router
    let app = Router::new()
        .route("/", get(health))
        .route("/query", post(handle_query))
        .layer(cors)
        .with_state(state);

    // Start the server
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start HTTP server: {}", e))
}

/// Health check handler
async fn health() -> impl IntoResponse {
    "HAPPE is running"
}

/// Handler for query requests
async fn handle_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    // Get or create session
    let session_id = get_or_create_session_id(headers, payload.session_id).await?;
    let mut session = get_or_create_session(&state.session_store, &session_id).await?;
    
    // Set session expiry (1 hour from now)
    session.set_expiry(Utc::now() + Duration::hours(1));
    
    // Process the query
    match coordinator::process_query(
        &state.config,
        &state.mcp_client,
        &state.gemini_client,
        &session,
        payload.query.clone(),
    )
    .await
    {
        Ok(response) => {
            // Create turn data and update session history
            let turn = ConversationTurn {
                user_query: payload.query,
                llm_response: response.clone(),
                retrieved_memories: vec![],
            };
            
            coordinator::update_session_history(&mut session, turn);
            
            // Save the session
            if let Err(e) = state.session_store.save_session(session.clone()).await {
                error!(error = %e, "Failed to save session");
                // Continue despite error
            }
            
            Ok(Json(QueryResponse {
                response,
                session_id: session.id,
                error: None,
            }))
        },
        Err(e) => {
            error!(error = %e, "Failed to process query");
            
            // Save the session anyway to preserve any state changes
            if let Err(save_err) = state.session_store.save_session(session.clone()).await {
                error!(error = %save_err, "Failed to save session after query error");
            }
            
            Ok(Json(QueryResponse {
                response: String::new(),
                session_id: session.id,
                error: Some(format!("Failed to process query: {}", e)),
            }))
        }
    }
}

/// Get or create a session ID from headers or request payload
async fn get_or_create_session_id(
    headers: HeaderMap,
    session_id_from_payload: Option<String>,
) -> Result<String, ApiError> {
    // Try to get session ID from header
    if let Some(session_header) = headers.get("X-Session-ID") {
        if let Ok(header_value) = session_header.to_str() {
            if !header_value.is_empty() {
                return Ok(header_value.to_string());
            }
        }
    }
    
    // Try to get session ID from payload
    if let Some(id) = session_id_from_payload {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    
    // Create a new session ID
    Ok(Uuid::new_v4().to_string())
}

/// Get an existing session or create a new one
async fn get_or_create_session(
    session_store: &SessionStoreRef,
    session_id: &str,
) -> Result<Session, ApiError> {
    // Try to get existing session
    match session_store.get_session(session_id).await {
        Ok(session) => {
            debug!(session_id = %session_id, "Using existing session");
            Ok(session)
        },
        Err(_) => {
            // Create a new session
            debug!(session_id = %session_id, "Creating new session");
            match session_store.create_session(session_id.to_string()).await {
                Ok(session) => Ok(session),
                Err(e) => Err(ApiError::SessionError(format!("Failed to create session: {}", e))),
            }
        }
    }
}
