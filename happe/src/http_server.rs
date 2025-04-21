use crate::config::AppConfig;
use crate::coordinator;
use crate::mcp_client::McpHostClient;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use gemini_core::client::GeminiClient;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

/// Application state shared with all routes
#[derive(Clone)]
pub struct AppState {
    config: Arc<AppConfig>,
    gemini_client: Arc<GeminiClient>,
    mcp_client: Arc<McpHostClient>,
}

/// Request model for queries
#[derive(Deserialize)]
pub struct QueryRequest {
    query: String,
}

/// Response model for queries
#[derive(Serialize)]
pub struct QueryResponse {
    response: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Error type for HTTP server
#[derive(Debug)]
pub enum ApiError {
    InternalError(anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::InternalError(e) => {
                error!(error = %e, "Internal server error");
                let body = Json(QueryResponse {
                    response: String::new(),
                    error: Some(format!("Internal server error: {}", e)),
                });
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
        }
    }
}

/// Start the HTTP server
pub async fn run_server(
    config: AppConfig,
    gemini_client: GeminiClient,
    mcp_client: McpHostClient,
    addr: SocketAddr,
) -> anyhow::Result<()> {
    info!("Starting HTTP server on {}", addr);

    // Create shared state
    let state = AppState {
        config: Arc::new(config),
        gemini_client: Arc::new(gemini_client),
        mcp_client: Arc::new(mcp_client),
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
    Json(payload): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    // Process the query
    match coordinator::process_query(
        &state.config,
        &state.mcp_client,
        &state.gemini_client,
        payload.query,
    )
    .await
    {
        Ok(response) => Ok(Json(QueryResponse {
            response,
            error: None,
        })),
        Err(e) => {
            error!(error = %e, "Failed to process query");
            Ok(Json(QueryResponse {
                response: String::new(),
                error: Some(format!("Failed to process query: {}", e)),
            }))
        }
    }
} 