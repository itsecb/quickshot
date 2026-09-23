//! The small floating status bar used by the step recorder, scrolling capture and GIF
//! recording. It's excluded from screen capture, so it never appears in what's being captured.

use tauri::{AppHandle, Emitter, Manager};

use crate::windows::RECORDER_LABEL;

pub const TEXT_EVENT: &str = "bar://text";

/// Show the bar for `kind` ("steps" | "scroll" | "gif") with an initial status line.
pub fn open(app: &AppHandle, kind: &str, text: &str) {
    let (app2, kind, text) = (app.clone(), kind.to_string(), text.to_string());
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = crate::windows::open_bar(&app2, &kind, &text) {
            log::error!("status bar failed: {e}");
        }
    });
}

pub fn update(app: &AppHandle, text: &str) {
    let _ = app.emit_to(RECORDER_LABEL, TEXT_EVENT, text);
}

pub fn close(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(RECORDER_LABEL) {
        let _ = w.destroy();
    }
}
