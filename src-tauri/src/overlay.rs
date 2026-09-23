//! One opaque, undecorated, always-on-top window per monitor showing the frozen frame.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::capture::MonitorInfo;
use crate::error::AppResult;
use crate::geom::Rect;
use crate::state::AppState;

pub fn label_for(monitor_id: u32) -> String {
    format!("overlay-{monitor_id}")
}

pub fn monitor_id_from_label(label: &str) -> Option<u32> {
    label.strip_prefix("overlay-")?.parse().ok()
}

/// Overlay windows kept loaded (hidden) between captures, by label, with the monitor rect they
/// were built for. Building a webview and loading the page is most of the time between the
/// hotkey and a ready overlay, so after the first capture (or the warm-up at startup) a capture
/// only has to hand the page a new frame. A label is here only while its page is idle and
/// listening for the next start.
static POOL: Mutex<Option<HashMap<String, Rect>>> = Mutex::new(None);
/// Rect each overlay window was built for, whatever state its page is in.
static BUILT: Mutex<Option<HashMap<String, Rect>>> = Mutex::new(None);

pub const START_EVENT: &str = "overlay://start";
pub const RESET_EVENT: &str = "overlay://reset";
/// The window list for hover/click detection, sent once it's built.
pub const WINDOWS_EVENT: &str = "overlay://windows";

fn with<T>(
    map: &Mutex<Option<HashMap<String, Rect>>>,
    f: impl FnOnce(&mut HashMap<String, Rect>) -> T,
) -> T {
    f(map.lock().unwrap().get_or_insert_with(HashMap::new))
}

/// Make overlays ready for `monitors` and start them on the current session: reuse an idle
/// pooled window built for the same monitor rect, otherwise build a fresh one (which starts
/// itself once loaded). Must run on the main thread, without the session lock held.
pub fn open(app: &AppHandle, monitors: &[MonitorInfo]) -> AppResult<()> {
    // overlays for monitors that are gone (display unplugged or rearranged)
    let wanted: Vec<String> = monitors.iter().map(|m| label_for(m.id)).collect();
    for (label, w) in app.webview_windows() {
        if label.starts_with("overlay-") && !wanted.contains(&label) {
            forget(&label);
            let _ = w.destroy();
        }
    }
    for m in monitors {
        let label = label_for(m.id);
        let reusable = with(&POOL, |p| p.remove(&label)) == Some(m.rect());
        match app.get_webview_window(&label) {
            Some(w) if reusable => {
                let _ = w.emit_to(label.as_str(), START_EVENT, ());
            }
            existing => {
                if let Some(stale) = existing {
                    forget(&label);
                    let _ = stale.destroy();
                }
                build(app, m, false)?;
            }
        }
    }
    Ok(())
}

/// Build hidden overlays for these monitors ahead of the first capture.
pub fn prewarm(app: &AppHandle, monitors: &[MonitorInfo]) {
    for m in monitors {
        if app.get_webview_window(&label_for(m.id)).is_none() {
            if let Err(e) = build(app, m, true) {
                log::warn!("overlay warm-up failed: {e}");
            }
        }
    }
}

/// `warm`: built ahead of time, so the page waits for a start instead of starting itself.
fn build(app: &AppHandle, m: &MonitorInfo, warm: bool) -> AppResult<()> {
    let label = label_for(m.id);
    let scale = if m.scale > 0.0 { m.scale } else { 1.0 };
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("overlay.html".into()))
        .title("QuickShot Capture")
        .initialization_script(format!("window.__QS_OVERLAY_WARM = {warm};"))
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
        // see-through: the page shows a dim layer over the live screen at once and draws the
        // frozen frame when it has loaded
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
        .position(m.x as f64 / scale, m.y as f64 / scale)
        .inner_size(m.width as f64 / scale, m.height as f64 / scale)
        .build()?;
    // Re-apply in physical pixels: the builder only accepts logical values and the
    // logical->physical conversion before the window exists uses an ambiguous DPI.
    // (and keep it there: moving it onto a monitor with other scaling makes Windows
    // rescale it, sometimes only after it's shown, which used to spill it onto the next
    // screen and scramble hit-testing)
    crate::windows::pin_rect(&window, m.x, m.y, m.width, m.height);
    with(&BUILT, |b| b.insert(label, m.rect()));
    Ok(())
}

fn forget(label: &str) {
    with(&POOL, |p| p.remove(label));
    with(&BUILT, |b| b.remove(label));
}

/// The page is idle and listening (just loaded warm, or reset after a capture): it can be
/// reused for the next capture.
fn mark_idle(label: &str) {
    if let Some(rect) = with(&BUILT, |b| b.get(label).copied()) {
        with(&POOL, |p| p.insert(label.to_string(), rect));
    }
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

/// End of a capture: hide the overlays at once and let their pages reset for the next one.
///
/// The page clears itself first (it's see-through, so that's invisible) and then reports idle,
/// which hides the window: hidden with the old picture still in it, a reused overlay could
/// flash the previous capture (a "phantom outline") when shown again.
pub fn release(app: &AppHandle, labels: &[String]) {
    for label in labels {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.emit_to(label.as_str(), RESET_EVENT, ());
        }
    }
    // a page that doesn't answer still gets out of the way
    let (app, labels) = (app.clone(), labels.to_vec());
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(400));
        for label in &labels {
            hide_unless_in_use(&app, label);
        }
    });
}

/// The page has cleared itself and is listening for the next capture.
pub fn idle(app: &AppHandle, label: &str) {
    mark_idle(label);
    hide_unless_in_use(app, label);
}

/// Hide an overlay, unless a newer capture is already using it.
fn hide_unless_in_use(app: &AppHandle, label: &str) {
    let in_use = app
        .state::<AppState>()
        .session
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|s| s.labels.iter().any(|l| l == label));
    if !in_use {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.hide();
        }
    }
}

/// Destroy overlays outright (a failed start).
pub fn close_labels(app: &AppHandle, labels: &[String]) {
    for label in labels {
        forget(label);
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.hide();
            let _ = w.destroy();
        }
    }
}
