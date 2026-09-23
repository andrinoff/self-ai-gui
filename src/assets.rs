//! Serves the built frontend out of the binary (web/dist, via rust-embed).

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web/dist"]
struct Assets;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(file) => {
            let mime = mime_for(path);
            (
                [
                    (header::CONTENT_TYPE, mime.as_str()),
                    (header::CACHE_CONTROL, "no-cache"),
                ],
                file.data.into_owned(),
            )
                .into_response()
        }
        // Deep links fall back to index.html so client routing keeps working.
        None => match Assets::get("index.html") {
            Some(file) => (
                [
                    (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                    (header::CACHE_CONTROL, "no-store"),
                ],
                file.data.into_owned(),
            )
                .into_response(),
            None => (
                StatusCode::SERVICE_UNAVAILABLE,
                "frontend not built yet; run `make ui` from the repo root",
            )
                .into_response(),
        },
    }
}

fn mime_for(path: &str) -> String {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    }
    .to_string()
}
