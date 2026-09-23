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
use crate::qr;
use crate::redact::{self, RedactMatch};
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
    /// A per-app rule asked for automatic redaction.
    pub auto_redact: bool,
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
        auto_redact: state.auto_redact.lock().unwrap().remove(&id),
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
        let format = match header(&request, "x-format").as_deref() {
            Some("jpeg") | Some("jpg") => settings::ImageFormat::Jpeg,
            Some("png") => settings::ImageFormat::Png,
            _ => settings.image_format,
        };
        let dir = settings::save_dir(&app, &settings)?;
        let stem = header(&request, "x-stem")
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
    let stem = header(&request, "x-stem").unwrap_or_else(|| "Screenshot".into());
    Ok(output::temp_png(&app, &bytes, &stem)?.display().to_string())
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
    let capture = state.insert_capture(&app, img, source);
    let id = capture.id;
    windows::open_pin_deferred(&app, capture, x, y);
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

/// OCR the original capture and return everything that looks sensitive, in image pixels
/// (the editor's shape coordinates), for one-key redaction.
#[tauri::command]
pub async fn redact_capture(app: AppHandle, id: u64) -> AppResult<Vec<RedactMatch>> {
    let state = app.state::<AppState>();
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    let settings = state.settings();
    tauri::async_runtime::spawn_blocking(move || -> AppResult<Vec<RedactMatch>> {
        let out = ocr::recognize(&capture.image, settings.ocr_language.as_deref())?;
        Ok(redact::find_sensitive(
            &out.lines,
            &settings.redact_patterns,
        ))
    })
    .await
    .map_err(|e| AppError::Ocr(e.to_string()))?
}

/// QR codes and barcodes in the original capture.
#[tauri::command]
pub async fn scan_codes(app: AppHandle, id: u64) -> AppResult<Vec<qr::Code>> {
    let capture = app
        .state::<AppState>()
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    tauri::async_runtime::spawn_blocking(move || qr::decode(&capture.image))
        .await
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Body: PNG rendered by the editor. Header `x-id`: capture id, for the caption's context.
#[tauri::command]
pub fn copy_image_rich(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Request<'_>,
) -> AppResult<String> {
    let bytes = raw_body(&request)?;
    output::ensure_png(&bytes)?;
    let img = decode_png(&bytes)?;
    let settings = state.settings();
    let (source, created) = header(&request, "x-id")
        .and_then(|v| v.parse::<u64>().ok())
        .and_then(|id| state.capture(id))
        .map(|c| (c.source.clone(), c.created))
        .unwrap_or_else(|| (CaptureSource::default(), chrono::Local::now()));
    let caption = output::ticket_caption(&settings, &source, created, img.width(), img.height());
    output::copy_rich(&app, &img, &bytes, &caption)?;
    Ok(caption)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThumbInit {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub png_url: String,
}

#[tauri::command]
pub fn thumb_init(state: State<'_, AppState>, label: String) -> AppResult<ThumbInit> {
    let id = crate::state::capture_id_from_label(&label)
        .ok_or_else(|| AppError::Other("bad thumbnail label".into()))?;
    let capture = state
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    Ok(ThumbInit {
        id,
        width: capture.image.width(),
        height: capture.image.height(),
        png_url: protocol::capture_png_url(id),
    })
}

/// Opens a window on the main thread.
type OpenWindow = Box<dyn FnOnce(&AppHandle) -> AppResult<()> + Send>;

/// Thumbnail quick actions: "edit" | "pin". The page closes itself afterwards.
#[tauri::command(async)]
pub fn thumb_action(app: AppHandle, id: u64, action: String) -> AppResult<()> {
    let capture = app
        .state::<AppState>()
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    let open: OpenWindow = match action.as_str() {
        "edit" => Box::new(move |app| windows::open_editor(app, &capture)),
        "pin" => Box::new(move |app| {
            let (x, y) = (capture.source.rect.x, capture.source.rect.y);
            windows::open_pin(app, &capture, x, y)
        }),
        other => return Err(AppError::Other(format!("unknown thumbnail action {other}"))),
    };
    // Wait until the new window exists: the thumbnail closes itself right after this returns,
    // and closing the last window that uses a capture frees it.
    let (done, wait) = std::sync::mpsc::channel();
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        let _ = done.send(open(&app2));
    })?;
    wait.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| AppError::Other("timed out opening the window".into()))?
}

/// A temp PNG of the capture for native drag-out from the thumbnail.
#[tauri::command(async)]
pub fn thumb_drag_path(app: AppHandle, id: u64) -> AppResult<String> {
    let capture = app
        .state::<AppState>()
        .capture(id)
        .ok_or_else(|| AppError::NotFound("capture expired".into()))?;
    let stem = settings::expand_pattern(
        &app.state::<AppState>().settings().file_pattern,
        &capture.source.app_name,
        &capture.source.title,
        capture.image.width(),
        capture.image.height(),
    );
    let png = crate::image_util::encode_png(&capture.image)?;
    Ok(output::temp_png(&app, &png, &stem)?.display().to_string())
}
