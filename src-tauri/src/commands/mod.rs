pub mod capture;
pub mod files;
pub mod guide;
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

pub fn header<'a>(request: &'a Request<'_>, name: &str) -> Option<&'a str> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}
