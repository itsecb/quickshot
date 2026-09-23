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
use image::RgbaImage;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PendingStep {
    pub id: u64,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub png_url: String,
    /// Title for the step itself (the recorder's "Click the “Save” button in Notepad").
    pub step_title: String,
    /// Put these steps into a new guide with this name instead of the open one.
    pub project: Option<String>,
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
    let title = header(&request, "x-title").unwrap_or_default();
    let source = CaptureSource {
        kind: "guide".into(),
        title: title.clone(),
        rect: Rect::new(0, 0, img.width(), img.height()),
        ..Default::default()
    };
    let capture = state.insert_capture(&app, img, source);
    let step = PendingStep {
        id: capture.id,
        title,
        width: capture.image.width(),
        height: capture.image.height(),
        png_url: protocol::capture_png_url(capture.id),
        step_title: String::new(),
        project: None,
    };
    state.pending_steps.lock().unwrap().push(step);
    notify_guide(&app);
    Ok(capture.id)
}

/// Queue several steps at once (the step recorder), optionally as a new guide.
pub fn queue_steps(app: &AppHandle, steps: Vec<(RgbaImage, String)>, project: Option<String>) {
    let state = app.state::<AppState>();
    for (img, step_title) in steps {
        let source = CaptureSource {
            kind: "guide".into(),
            title: step_title.clone(),
            rect: Rect::new(0, 0, img.width(), img.height()),
            ..Default::default()
        };
        let capture = state.insert_capture(app, img, source);
        state.pending_steps.lock().unwrap().push(PendingStep {
            id: capture.id,
            title: String::new(),
            width: capture.image.width(),
            height: capture.image.height(),
            png_url: protocol::capture_png_url(capture.id),
            step_title,
            project: project.clone(),
        });
    }
    notify_guide(app);
}

/// Tell an open Guides window about new steps, or open it (it pulls them on load).
fn notify_guide(app: &AppHandle) {
    if app.get_webview_window("guide").is_some() {
        let _ = app.emit_to("guide", "guide://step-added", ());
    } else {
        // Window creation must not happen inside a synchronous command (deadlocks on Windows).
        let app2 = app.clone();
        std::thread::spawn(move || {
            let app3 = app2.clone();
            let _ = app2.run_on_main_thread(move || windows::open_guide(&app3));
        });
    }
}

/// Drain queued steps (the guide window calls this on load and on each notification).
#[tauri::command]
pub fn guide_pull_steps(state: State<'_, AppState>) -> Vec<PendingStep> {
    std::mem::take(&mut *state.pending_steps.lock().unwrap())
}
