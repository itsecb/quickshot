//! `shot://` custom protocol serving frozen frames and captures as raw RGBA or PNG.
//!
//! Paths:
//!   /monitor/<id>        raw RGBA of the current session frame for that monitor
//!   /monitor/<id>.bmp    the same as a BMP (the overlay decodes it natively, off its main thread)
//!   /capture/<id>        raw RGBA of a finished capture
//!   /capture/<id>.png    PNG of a finished capture (for <img> and pins)
//!   /history/<id>.jpg    history thumbnail
//!   /history/<id>.png    full history image
//! Raw responses carry `x-width` / `x-height` headers.

use tauri::http::{Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeContext, UriSchemeResponder};

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

pub fn file_url(token: u64) -> String {
    format!("{}/file/{token}", base_url())
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

/// 32-bit top-down BMP: a 54-byte header in front of the pixels (BGRA order).
fn bmp(image: &image::RgbaImage) -> Response<Vec<u8>> {
    let (w, h) = image.dimensions();
    let pixels = w as usize * h as usize * 4;
    let mut body = Vec::with_capacity(54 + pixels);
    let u32le = |v: u32| v.to_le_bytes();
    // BITMAPFILEHEADER
    body.extend_from_slice(b"BM");
    body.extend_from_slice(&u32le((54 + pixels) as u32));
    body.extend_from_slice(&[0; 4]);
    body.extend_from_slice(&u32le(54));
    // BITMAPINFOHEADER: negative height = rows top-down
    body.extend_from_slice(&u32le(40));
    body.extend_from_slice(&(w as i32).to_le_bytes());
    body.extend_from_slice(&(-(h as i32)).to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&32u16.to_le_bytes());
    body.extend_from_slice(&u32le(0)); // BI_RGB
    body.extend_from_slice(&u32le(pixels as u32));
    body.extend_from_slice(&[0; 16]);
    for px in image.as_raw().chunks_exact(4) {
        body.extend_from_slice(&[px[2], px[1], px[0], 255]);
    }
    Response::builder()
        .status(StatusCode::OK)
        .header("Access-Control-Allow-Origin", "*")
        .header("Content-Type", "image/bmp")
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

/// Requests are answered on a worker thread: frames are tens of MB, and building them on the
/// main thread (where synchronous handlers run) held up every window while overlays loaded.
pub fn handle_async<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    std::thread::spawn(move || responder.respond(handle(&app, request)));
}

fn handle<R: Runtime>(app: &AppHandle<R>, request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let state = app.state::<AppState>();
    let path = request.uri().path().trim_start_matches('/').to_string();
    let mut parts = path.splitn(2, '/');
    let kind = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("");

    match kind {
        "monitor" => {
            let (id_str, as_bmp) = match rest.strip_suffix(".bmp") {
                Some(id) => (id, true),
                None => (rest, false),
            };
            let Ok(id) = id_str.parse::<u32>() else {
                return error(StatusCode::BAD_REQUEST, "bad monitor id");
            };
            let image = {
                let guard = state.session.lock().unwrap();
                let Some(session) = guard.as_ref() else {
                    return error(StatusCode::NOT_FOUND, "no capture session");
                };
                match session.frames.iter().find(|f| f.monitor.id == id) {
                    Some(f) => f.image.clone(),
                    None => return error(StatusCode::NOT_FOUND, "no frame for monitor"),
                }
            };
            if as_bmp {
                bmp(&image)
            } else {
                raw(image.width(), image.height(), image.into_raw())
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
        "file" => {
            // only files registered with `share_file`, looked up by token
            let path = rest
                .parse::<u64>()
                .ok()
                .and_then(|t| state.shared_files.lock().unwrap().get(&t).cloned());
            let Some(path) = path else {
                return error(StatusCode::NOT_FOUND, "unknown file");
            };
            let mime = match path.extension().and_then(|e| e.to_str()) {
                Some("gif") => "image/gif",
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                _ => "application/octet-stream",
            };
            file(path, mime)
        }
        _ => error(StatusCode::NOT_FOUND, "unknown path"),
    }
}
