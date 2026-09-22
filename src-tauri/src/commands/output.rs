use serde::Serialize;
use tauri::ipc::{Request, Response};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

use super::{header, raw_body};
use crate::error::{AppError, AppResult};
use crate::geom::Rect;
use crate::image_util::decode_png;
use crate::ocr::{self, OcrOutput};
use crate::output;
use crate::settings::{self, Settings};
use crate::state::{AppState, CaptureSource};
use crate::{protocol, windows};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorInit {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub frame_url: String,
    pub png_url: String,
    pub source: CaptureSource,
    pub created: String,
    pub settings: Settings,
}

fn capture_id_from_label(label: &str) -> Option<u64> {
    label.rsplit('-').next()?.parse().ok()
}

#[tauri::command]
pub fn editor_init(state: State<'_, AppState>, label: String) -> AppResult<EditorInit> {
    let id =
        capture_id_from_label(&label).ok_or_else(|| AppError::Other("bad editor label".into()))?;
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    Ok(EditorInit {
        id,
        width: capture.image.width(),
        height: capture.image.height(),
        frame_url: protocol::capture_url(id),
        png_url: protocol::capture_png_url(id),
        source: capture.source.clone(),
        created: capture.created.to_rfc3339(),
        settings: state.settings(),
    })
}

#[tauri::command]
pub fn release_capture(state: State<'_, AppState>, id: u64) {
    state.remove_capture(id);
}

/// Body: PNG bytes rendered by the editor.
#[tauri::command]
pub fn copy_image(app: AppHandle, request: Request<'_>) -> AppResult<()> {
    let bytes = raw_body(&request)?;
    output::ensure_png(&bytes)?;
    let img = decode_png(&bytes)?;
    output::copy_to_clipboard(&app, &img)
}

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> AppResult<()> {
    app.clipboard().write_text(text)?;
    Ok(())
}

/// Body: PNG bytes. Headers: `x-path` (explicit target) or `x-stem` (name without extension)
/// and optional `x-format` (png|jpeg). Returns the saved path.
#[tauri::command]
pub fn save_image(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Request<'_>,
) -> AppResult<String> {
    let bytes = raw_body(&request)?;
    output::ensure_png(&bytes)?;
    let settings = state.settings();
    let path = if let Some(p) = header(&request, "x-path") {
        std::path::PathBuf::from(p)
    } else {
        let format = match header(&request, "x-format") {
            Some("jpeg") | Some("jpg") => settings::ImageFormat::Jpeg,
            Some("png") => settings::ImageFormat::Png,
            _ => settings.image_format,
        };
        let dir = settings::save_dir(&app, &settings)?;
        let stem = header(&request, "x-stem")
            .map(str::to_string)
            .unwrap_or_else(|| settings::expand_pattern(&settings.file_pattern, "", "", 0, 0));
        output::unique_path(&dir, &stem, format.extension())
    };
    output::write_encoded(&path, &bytes, settings.jpeg_quality)?;
    if settings.copy_on_save {
        if let Ok(img) = decode_png(&bytes) {
            let _ = output::copy_to_clipboard(&app, &img);
        }
    }
    Ok(path.display().to_string())
}

/// Body: PNG bytes. Writes a temp file for drag-out and returns its path.
#[tauri::command]
pub fn export_temp_png(app: AppHandle, request: Request<'_>) -> AppResult<String> {
    let bytes = raw_body(&request)?;
    output::ensure_png(&bytes)?;
    let stem = header(&request, "x-stem").unwrap_or("Screenshot");
    Ok(output::temp_png(&app, &bytes, stem)?.display().to_string())
}

/// Body: PNG bytes. Headers: optional `x-x`, `x-y` physical position. Opens a pin window.
#[tauri::command]
pub fn pin_image(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Request<'_>,
) -> AppResult<u64> {
    let bytes = raw_body(&request)?;
    output::ensure_png(&bytes)?;
    let img = decode_png(&bytes)?;
    let x = header(&request, "x-x")
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);
    let y = header(&request, "x-y")
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);
    let source = CaptureSource {
        kind: "pin".into(),
        rect: Rect::new(x, y, img.width(), img.height()),
        ..Default::default()
    };
    let capture = state.insert_capture(img, source);
    let id = capture.id;
    windows::open_pin(&app, &capture, x, y)?;
    Ok(id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinInit {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub png_url: String,
}

#[tauri::command]
pub fn pin_init(state: State<'_, AppState>, label: String) -> AppResult<PinInit> {
    let id =
        capture_id_from_label(&label).ok_or_else(|| AppError::Other("bad pin label".into()))?;
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    Ok(PinInit {
        id,
        width: capture.image.width(),
        height: capture.image.height(),
        png_url: protocol::capture_png_url(id),
    })
}

/// Copy a capture (optionally cropped) to the clipboard without the editor.
#[tauri::command]
pub fn copy_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    id: u64,
    rect: Option<Rect>,
) -> AppResult<()> {
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    match rect {
        Some(r) => {
            let sub = image::imageops::crop_imm(
                &capture.image,
                r.x.max(0) as u32,
                r.y.max(0) as u32,
                r.width,
                r.height,
            )
            .to_image();
            output::copy_to_clipboard(&app, &sub)
        }
        None => output::copy_to_clipboard(&app, &capture.image),
    }
}

#[tauri::command]
pub fn save_capture(app: AppHandle, state: State<'_, AppState>, id: u64) -> AppResult<String> {
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    let settings = state.settings();
    let path = output::save_to_folder(&app, &settings, &capture.image, &capture.source, None)?;
    Ok(path.display().to_string())
}

/// Raw RGBA bytes of a capture region (x-width / x-height headers), for tools that need pixels.
#[tauri::command]
pub fn capture_pixels(state: State<'_, AppState>, id: u64) -> AppResult<Response> {
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    Ok(Response::new(capture.image.as_raw().clone()))
}

#[tauri::command]
pub async fn ocr_capture(app: AppHandle, id: u64, rect: Option<Rect>) -> AppResult<OcrOutput> {
    let state = app.state::<AppState>();
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    let language = state.settings().ocr_language;
    tauri::async_runtime::spawn_blocking(move || {
        let img = match rect {
            Some(r) if !r.is_empty() => {
                let x = r.x.clamp(0, capture.image.width() as i32 - 1) as u32;
                let y = r.y.clamp(0, capture.image.height() as i32 - 1) as u32;
                let w = r.width.min(capture.image.width() - x);
                let h = r.height.min(capture.image.height() - y);
                image::imageops::crop_imm(&capture.image, x, y, w, h).to_image()
            }
            _ => capture.image.clone(),
        };
        ocr::recognize(&img, language.as_deref())
    })
    .await
    .map_err(|e| AppError::Ocr(e.to_string()))?
}

/// OCR of PNG bytes uploaded from the editor (already-rendered image).
#[tauri::command]
pub fn ocr_png(state: State<'_, AppState>, request: Request<'_>) -> AppResult<OcrOutput> {
    let bytes = raw_body(&request)?;
    let img = decode_png(&bytes)?;
    ocr::recognize(&img, state.settings().ocr_language.as_deref())
}
