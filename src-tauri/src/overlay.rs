//! One opaque, undecorated, always-on-top window per monitor showing the frozen frame.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::error::AppResult;
use crate::state::CaptureSession;

pub fn label_for(monitor_id: u32) -> String {
    format!("overlay-{monitor_id}")
}

pub fn monitor_id_from_label(label: &str) -> Option<u32> {
    label.strip_prefix("overlay-")?.parse().ok()
}

/// Create hidden overlay windows for every frame. Must run on the main thread.
pub fn open(app: &AppHandle, session: &mut CaptureSession) -> AppResult<()> {
    for frame in &session.frames {
        let m = &frame.monitor;
        let label = label_for(m.id);
        if let Some(stale) = app.get_webview_window(&label) {
            let _ = stale.destroy();
        }
        let scale = if m.scale > 0.0 { m.scale } else { 1.0 };
        let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("overlay.html".into()))
            .title("QuickShot Capture")
            .decorations(false)
            .resizable(false)
            .maximizable(false)
            .minimizable(false)
            .closable(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .focused(false)
            .shadow(false)
            .visible_on_all_workspaces(true)
            .background_color(tauri::window::Color(0, 0, 0, 255))
            .position(m.x as f64 / scale, m.y as f64 / scale)
            .inner_size(m.width as f64 / scale, m.height as f64 / scale)
            .build()?;
        // Re-apply in physical pixels: the builder only accepts logical values and the
        // logical->physical conversion before the window exists uses an ambiguous DPI.
        window.set_position(PhysicalPosition::new(m.x, m.y))?;
        window.set_size(PhysicalSize::new(m.width, m.height))?;
        session.labels.push(label);
    }
    Ok(())
}

/// Show every overlay; focus the one under the cursor so it receives keyboard input.
pub fn show_all(app: &AppHandle, labels: &[String], focus: Option<&str>) {
    for label in labels {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.show();
            let _ = w.set_always_on_top(true);
        }
    }
    let focus_label = focus.unwrap_or_else(|| labels.first().map(String::as_str).unwrap_or(""));
    if let Some(w) = app.get_webview_window(focus_label) {
        let _ = w.set_focus();
    }
}

pub fn close_labels(app: &AppHandle, labels: &[String]) {
    for label in labels {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.hide();
            let _ = w.destroy();
        }
    }
}
