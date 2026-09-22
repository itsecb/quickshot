//! Hand-off of rendered captures from editor windows to the guide window.

use serde::Serialize;
use tauri::ipc::Request;
use tauri::{AppHandle, Emitter, Manager, State};

use super::{header, raw_body};
use crate::error::AppResult;
use crate::geom::Rect;
use crate::image_util::decode_png;
use crate::state::{AppState, CaptureSource};
use crate::{protocol, windows};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingStep {
    pub id: u64,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub png_url: String,
}

/// Body: PNG bytes. Header `x-title`. Stores the image as a capture, queues it for the
/// guide window, opens that window if needed and notifies it.
#[tauri::command]
pub fn guide_push_step(
    app: AppHandle,
    state: State<'_, AppState>,
    request: Request<'_>,
) -> AppResult<u64> {
    let bytes = raw_body(&request)?;
    let img = decode_png(&bytes)?;
    let title = header(&request, "x-title").unwrap_or("").to_string();
    let source = CaptureSource {
        kind: "guide".into(),
        title: title.clone(),
        rect: Rect::new(0, 0, img.width(), img.height()),
        ..Default::default()
    };
    let capture = state.insert_capture(img, source);
    let step = PendingStep {
        id: capture.id,
        title,
        width: capture.image.width(),
        height: capture.image.height(),
        png_url: protocol::capture_png_url(capture.id),
    };
    state.pending_steps.lock().unwrap().push(step.clone());
    if app.get_webview_window("guide").is_some() {
        let _ = app.emit_to("guide", "guide://step-added", &step);
    } else {
        windows::open_guide(&app);
    }
    Ok(capture.id)
}

/// Drain queued steps (the guide window calls this on load and on each notification).
#[tauri::command]
pub fn guide_pull_steps(state: State<'_, AppState>) -> Vec<PendingStep> {
    std::mem::take(&mut *state.pending_steps.lock().unwrap())
}
