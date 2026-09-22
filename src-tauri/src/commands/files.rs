//! Small file helpers for guide projects and exports. Paths are on the user's own machine.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::ipc::{Request, Response};
use tauri::{AppHandle, State};

use super::{header, raw_body};
use crate::error::{AppError, AppResult};
use crate::settings;
use crate::state::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub modified: Option<String>,
}

fn check_path(p: &str) -> AppResult<PathBuf> {
    let path = PathBuf::from(p);
    if !path.is_absolute() {
        return Err(AppError::Other("path must be absolute".into()));
    }
    Ok(path)
}

/// Body: raw bytes. Header `x-path`: absolute destination. Creates parent folders.
#[tauri::command]
pub fn fs_write(request: Request<'_>) -> AppResult<String> {
    let path = check_path(
        &header(&request, "x-path")
            .ok_or_else(|| AppError::Other("x-path header missing".into()))?,
    )?;
    let bytes = raw_body(&request)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, bytes)?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn fs_write_text(path: String, text: String) -> AppResult<String> {
    let path = check_path(&path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, text)?;
    Ok(path.display().to_string())
}

#[tauri::command]
pub fn fs_read_text(path: String) -> AppResult<String> {
    Ok(std::fs::read_to_string(check_path(&path)?)?)
}

#[tauri::command]
pub fn fs_read_bytes(path: String) -> AppResult<Response> {
    Ok(Response::new(std::fs::read(check_path(&path)?)?))
}

#[tauri::command]
pub fn fs_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
pub fn fs_mkdir(path: String) -> AppResult<()> {
    std::fs::create_dir_all(check_path(&path)?)?;
    Ok(())
}

#[tauri::command]
pub fn fs_remove(path: String) -> AppResult<()> {
    let p = check_path(&path)?;
    if p.is_dir() {
        std::fs::remove_dir_all(p)?;
    } else if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

#[tauri::command]
pub fn fs_list(path: String) -> AppResult<Vec<DirEntry>> {
    let dir = check_path(&path)?;
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let meta = entry.metadata().ok();
        let modified = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).to_rfc3339());
        out.push(DirEntry {
            name: entry.file_name().to_string_lossy().to_string(),
            path: entry.path().display().to_string(),
            is_dir: meta.map(|m| m.is_dir()).unwrap_or(false),
            modified,
        });
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

#[tauri::command]
pub fn guides_dir(app: AppHandle, state: State<'_, AppState>) -> AppResult<String> {
    Ok(settings::guides_dir(&app, &state.settings())?
        .display()
        .to_string())
}
