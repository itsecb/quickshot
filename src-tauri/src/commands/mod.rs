pub mod capture;
pub mod files;
pub mod guide;
pub mod history;
pub mod output;
pub mod settings;

use tauri::ipc::{InvokeBody, Request};

use crate::error::{AppError, AppResult};

/// Raw bytes of an `invoke(cmd, new Uint8Array(...))` call.
pub fn raw_body(request: &Request<'_>) -> AppResult<Vec<u8>> {
    match request.body() {
        InvokeBody::Raw(bytes) => Ok(bytes.clone()),
        InvokeBody::Json(_) => Err(AppError::Other("expected a binary request body".into())),
    }
}

/// Header values are percent-encoded by the frontend so non-ASCII paths and titles survive.
pub fn header(request: &Request<'_>, name: &str) -> Option<String> {
    let raw = request.headers().get(name)?.to_str().ok()?;
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .ok()?;
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
