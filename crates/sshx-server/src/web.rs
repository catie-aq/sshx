//! HTTP and WebSocket handlers for the sshx web interface.

use std::sync::Arc;

use axum::extract::{Multipart, Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{any, get_service, post};
use axum::{Json, Router};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};
use tracing::warn;

use crate::ServerState;

pub mod ide_proxy;
pub mod protocol;
mod socket;

/// Returns the web application server, routed with Axum.
pub fn app() -> Router<Arc<ServerState>> {
    let root_spa = ServeFile::new("build/spa.html")
        .precompressed_gzip()
        .precompressed_br();

    // Serves static SvelteKit build files.
    let static_files = ServeDir::new("build")
        .precompressed_gzip()
        .precompressed_br()
        .fallback(root_spa);

    // Serve uploaded image files from the ./uploads/ directory.
    let uploads_dir = ServeDir::new("uploads");

    Router::new()
        .nest("/ide", ide_proxy::routes())
        .nest("/api", backend())
        .nest_service("/uploads", get_service(uploads_dir))
        .fallback_service(get_service(static_files))
}

/// Routes for the backend web API server.
fn backend() -> Router<Arc<ServerState>> {
    Router::new()
        .route("/s/{name}", any(socket::get_session_ws))
        .route("/s/{name}/upload", post(handle_upload))
}

/// Handle image upload: save the file to ./uploads/{session_name}/ and return its URL.
async fn handle_upload(
    Path(name): Path<String>,
    State(_state): State<Arc<ServerState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    // Sanitize session name for use as a directory component.
    let safe_name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe_name.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "invalid session name"}))).into_response();
    }

    let upload_dir = std::path::Path::new("uploads").join(&safe_name);
    if let Err(e) = tokio::fs::create_dir_all(&upload_dir).await {
        warn!("failed to create upload dir: {e}");
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "storage error"}))).into_response();
    }

    while let Ok(Some(field)) = multipart.next_field().await {
        // Only process the first "file" field.
        let original_name = field
            .file_name()
            .unwrap_or("upload.bin")
            .to_string();

        // Sanitize the filename: keep alphanumeric, dot, dash, underscore.
        let sanitized: String = original_name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
            .collect();
        let filename = if sanitized.is_empty() {
            format!("upload_{}.bin", uuid_short())
        } else {
            // Prefix with a short random string to avoid collisions.
            format!("{}_{}", uuid_short(), sanitized)
        };

        let data = match field.bytes().await {
            Ok(d) => d,
            Err(e) => {
                warn!("failed to read upload field: {e}");
                return (StatusCode::BAD_REQUEST, Json(json!({"error": "read error"}))).into_response();
            }
        };

        let file_path = upload_dir.join(&filename);
        if let Err(e) = tokio::fs::write(&file_path, &data).await {
            warn!("failed to write upload: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "write error"}))).into_response();
        }

        let url = format!("/uploads/{safe_name}/{filename}");
        return (StatusCode::OK, Json(json!({"url": url, "filename": filename}))).into_response();
    }

    (StatusCode::BAD_REQUEST, Json(json!({"error": "no file field found"}))).into_response()
}

/// Generate a short (8-char) random hex string for unique filenames.
fn uuid_short() -> String {
    use rand::Rng;
    let n: u32 = rand::thread_rng().gen();
    format!("{n:08x}")
}
