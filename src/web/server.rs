//! Lens visualisation web server.
//!
//! Serves the frontend (from `web/`) and JSON APIs with Axum.

use anyhow::Result;
use axum::extract::{Path, Query, State};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tracing::info;

use crate::engine::Store;

pub async fn serve(bind: &str, data_dir: &PathBuf) -> Result<()> {
    let addr: std::net::SocketAddr = bind.parse()?;
    let web_dir = std::env::current_dir()?.join("web");
    let store = Arc::new(Store::open(data_dir)?);

    let app = Router::new()
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/search", get(search_sessions))
        .route("/api/sessions/:id", get(get_session))
        .route("/api/sessions/:id/summary", get(get_session_summary))
        .route("/api/sessions/:id/graph", get(get_session_graph))
        .route("/api/stats", get(get_stats))
        .nest_service("/assets", ServeDir::new(web_dir.join("assets")))
        .fallback(index_handler)
        .layer(CorsLayer::permissive())
        .with_state(store);

    let listener = TcpListener::bind(&addr).await?;
    info!("Lens dashboard: http://{}", addr);

    if let Err(e) = _open_browser(&format!("http://{}", addr)) {
        info!("Could not auto-open browser: {}", e);
        info!("Open manually: http://{}", addr);
    }

    axum::serve(listener, app).await?;
    Ok(())
}

async fn index_handler() -> Result<Html<String>, AppError> {
    let paths = [
        std::env::current_dir()
            .map(|d| d.join("web/index.html"))
            .unwrap_or_default(),
        PathBuf::from("/mnt/data/catpaw/home/workspace/001/lens/web/index.html"),
    ];
    for p in &paths {
        if let Ok(html) = std::fs::read_to_string(p) {
            return Ok(Html(html));
        }
    }
    Err(anyhow::anyhow!("index.html not found").into())
}

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
}

async fn list_sessions(
    State(store): State<Arc<Store>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let sessions = blocking(move || store.list_sessions(100)).await?;
    Ok(Json(serde_json::json!({ "sessions": sessions })))
}

async fn search_sessions(
    State(store): State<Arc<Store>>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let q = params.q.unwrap_or_default();
    let sessions = blocking(move || store.search_sessions(&q, 50)).await?;
    Ok(Json(serde_json::json!({ "sessions": sessions })))
}

async fn get_session(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let detail = blocking(move || store.get_session(&id)).await?;
    Ok(Json(serde_json::json!({ "session": detail })))
}

async fn get_session_graph(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let data_dir = store.data_dir();
    let id = id.clone();
    let (nodes, edges) =
        blocking(move || crate::engine::graph::build_graph(&data_dir, Some(&id))).await?;
    Ok(Json(serde_json::json!({ "nodes": nodes, "edges": edges })))
}

async fn get_session_summary(
    State(store): State<Arc<Store>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Pre-check: session must exist
    let id_check = id.clone();
    let data_dir = store.data_dir();
    let exists = blocking(move || store.get_session(&id_check)).await?;
    if exists.is_none() {
        return Ok(Json(serde_json::json!({ "error": "会话不存在" })));
    }
    let summary =
        blocking(move || crate::engine::summary::summarize(&data_dir, &id)).await?;
    Ok(Json(serde_json::json!({ "summary": summary })))
}

async fn get_stats(
    State(store): State<Arc<Store>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let data_dir = store.data_dir();
    let stats = blocking(move || store.compute_stats(&data_dir)).await?;
    Ok(Json(stats))
}

#[derive(Debug)]
struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Error: {}", self.0),
        )
            .into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

async fn blocking<F, R>(job: F) -> Result<R, anyhow::Error>
where
    F: FnOnce() -> Result<R> + Send + 'static,
    R: Send + 'static,
{
    tokio::task::spawn_blocking(job)
        .await
        .map_err(|e| anyhow::anyhow!("Task failed: {}", e))?
}

#[cfg(target_os = "macos")]
fn _open_browser(url: &str) -> Result<()> {
    duct::cmd!("open", url).run()?;
    Ok(())
}
#[cfg(target_os = "linux")]
fn _open_browser(url: &str) -> Result<()> {
    duct::cmd!("xdg-open", url).run()?;
    Ok(())
}
#[cfg(target_os = "windows")]
fn _open_browser(url: &str) -> Result<()> {
    duct::cmd!("cmd", "/C", "start", url).run()?;
    Ok(())
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn _open_browser(_url: &str) -> Result<()> {
    anyhow::bail!("unsupported os")
}
