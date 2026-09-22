//! `shot://` custom protocol serving frozen frames and captures as raw RGBA or PNG.
//!
//! Paths:
//!   /monitor/<id>        raw RGBA of the current session frame for that monitor
//!   /capture/<id>        raw RGBA of a finished capture
//!   /capture/<id>.png    PNG of a finished capture (for <img> and pins)
//!   /history/<id>.jpg    history thumbnail
//!   /history/<id>.png    full history image
//! Raw responses carry `x-width` / `x-height` headers.

use tauri::http::{Request, Response, StatusCode};
use tauri::{Manager, Runtime, UriSchemeContext};

use crate::history;
use crate::image_util::encode_png;
use crate::state::AppState;

pub const SCHEME: &str = "shot";

fn base_url() -> String {
    if cfg!(windows) {
        format!("http://{SCHEME}.localhost")
    } else {
        format!("{SCHEME}://localhost")
    }
}

pub fn monitor_url(id: u32) -> String {
    format!("{}/monitor/{id}", base_url())
}

pub fn capture_url(id: u64) -> String {
    format!("{}/capture/{id}", base_url())
}

pub fn capture_png_url(id: u64) -> String {
    format!("{}/capture/{id}.png", base_url())
}

pub fn history_thumb_url(id: u64) -> String {
    format!("{}/history/{id}.jpg", base_url())
}

pub fn history_png_url(id: u64) -> String {
    format!("{}/history/{id}.png", base_url())
}

fn error(status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*")
        .header("Content-Type", "text/plain")
        .body(msg.as_bytes().to_vec())
        .unwrap()
}

fn raw(width: u32, height: u32, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Expose-Headers", "x-width, x-height")
        .header("Content-Type", "application/octet-stream")
        .header("Cache-Control", "no-store")
        .header("x-width", width.to_string())
        .header("x-height", height.to_string())
        .body(body)
        .unwrap()
}

fn png(body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header("Access-Control-Allow-Origin", "*")
        .header("Content-Type", "image/png")
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap()
}

/// History files never change once written, so the webview may cache them.
fn file(path: std::path::PathBuf, content_type: &str) -> Response<Vec<u8>> {
    match std::fs::read(path) {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header("Access-Control-Allow-Origin", "*")
            .header("Content-Type", content_type)
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(body)
            .unwrap(),
        Err(_) => error(StatusCode::NOT_FOUND, "history item not found"),
    }
}

pub fn handle<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let app = ctx.app_handle();
    let state = app.state::<AppState>();
    let path = request.uri().path().trim_start_matches('/').to_string();
    let mut parts = path.splitn(2, '/');
    let kind = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");

    match kind {
        "monitor" => {
            let Ok(id) = rest.parse::<u32>() else {
                return error(StatusCode::BAD_REQUEST, "bad monitor id");
            };
            let guard = state.session.lock().unwrap();
            let Some(session) = guard.as_ref() else {
                return error(StatusCode::NOT_FOUND, "no capture session");
            };
            match session.frames.iter().find(|f| f.monitor.id == id) {
                Some(f) => raw(f.image.width(), f.image.height(), f.image.as_raw().clone()),
                None => error(StatusCode::NOT_FOUND, "no frame for monitor"),
            }
        }
        "capture" => {
            let want_png = rest.ends_with(".png");
            let id_str = rest.trim_end_matches(".png");
            let Ok(id) = id_str.parse::<u64>() else {
                return error(StatusCode::BAD_REQUEST, "bad capture id");
            };
            let Some(capture) = state.capture(id) else {
                return error(StatusCode::NOT_FOUND, "capture expired");
            };
            if want_png {
                match encode_png(&capture.image) {
                    Ok(bytes) => png(bytes),
                    Err(e) => error(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
                }
            } else {
                raw(
                    capture.image.width(),
                    capture.image.height(),
                    capture.image.as_raw().clone(),
                )
            }
        }
        "history" => {
            // ids are parsed as numbers, so a path can never escape the history folder
            let (id_str, thumb) = match rest.strip_suffix(".jpg") {
                Some(id) => (id, true),
                None => (rest.trim_end_matches(".png"), false),
            };
            let Ok(id) = id_str.parse::<u64>() else {
                return error(StatusCode::BAD_REQUEST, "bad history id");
            };
            let path = if thumb {
                history::thumb_path(app, id)
            } else {
                history::image_path(app, id)
            };
            match path {
                Ok(p) => file(p, if thumb { "image/jpeg" } else { "image/png" }),
                Err(e) => error(StatusCode::NOT_FOUND, &e.to_string()),
            }
        }
        _ => error(StatusCode::NOT_FOUND, "unknown path"),
    }
}
