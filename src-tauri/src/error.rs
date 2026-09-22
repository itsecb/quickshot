use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("capture failed: {0}")]
    Capture(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("{0}")]
    Tauri(#[from] tauri::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("OCR failed: {0}")]
    Ocr(String),
    #[error("{0}")]
    Other(String),
}

impl From<xcap::XCapError> for AppError {
    fn from(e: xcap::XCapError) -> Self {
        AppError::Capture(e.to_string())
    }
}

impl From<tauri_plugin_clipboard_manager::Error> for AppError {
    fn from(e: tauri_plugin_clipboard_manager::Error) -> Self {
        AppError::Other(format!("clipboard: {e}"))
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
