//! Helpers for the editor, pin, guide and main windows plus notifications.

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_notification::NotificationExt;

use std::sync::Arc;

use crate::error::AppResult;
use crate::state::Capture;

pub fn toast(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn open_guide(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("guide") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let result = WebviewWindowBuilder::new(app, "guide", WebviewUrl::App("guide.html".into()))
        .title("QuickShot Guides")
        .inner_size(1100.0, 760.0)
        .min_inner_size(760.0, 520.0)
        .center()
        .build();
    if let Err(e) = result {
        log::error!("guide window failed: {e}");
    }
}

pub fn open_history(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("history") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let result = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history.html".into()))
        .title("QuickShot History")
        .inner_size(1040.0, 720.0)
        .min_inner_size(560.0, 420.0)
        .center()
        .build();
    if let Err(e) = result {
        log::error!("history window failed: {e}");
    }
}

pub const COUNTDOWN_LABEL: &str = "countdown";

/// Small always-on-top countdown in the corner of the cursor's monitor. It never takes focus,
/// so hover and right-click menus the user is setting up stay open.
pub fn open_countdown(app: &AppHandle, secs: u32) -> AppResult<()> {
    if let Some(w) = app.get_webview_window(COUNTDOWN_LABEL) {
        let _ = w.destroy();
    }
    let (w, h) = (176.0, 64.0);
    let (cx, cy) = app
        .cursor_position()
        .map(|p| (p.x as i32, p.y as i32))
        .unwrap_or((0, 0));
    let (scale, work) = monitor_at(app, cx, cy);
    let builder = WebviewWindowBuilder::new(
        app,
        COUNTDOWN_LABEL,
        WebviewUrl::App("countdown.html".into()),
    )
    .title("QuickShot countdown")
    .initialization_script(format!("window.__QS_COUNTDOWN = {secs};"))
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .shadow(false)
    .focused(false)
    .focusable(false)
    .visible(false)
    .inner_size(w, h);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    let window = builder.build()?;
    if let Some(area) = work {
        let margin = 24.0 * scale;
        let px = area.position.x + area.size.width as i32 - (w * scale + margin) as i32;
        let py = area.position.y + area.size.height as i32 - (h * scale + margin) as i32;
        let _ = window.set_position(PhysicalPosition::new(px, py));
    }
    let _ = window.show();
    Ok(())
}

/// Scale factor and work area (physical) of the monitor containing a point.
fn monitor_at(app: &AppHandle, x: i32, y: i32) -> (f64, Option<tauri::PhysicalRect<i32, u32>>) {
    match app.monitor_from_point(x as f64, y as f64) {
        Ok(Some(m)) => (m.scale_factor(), Some(*m.work_area())),
        _ => match app.primary_monitor() {
            Ok(Some(m)) => (m.scale_factor(), Some(*m.work_area())),
            _ => (1.0, None),
        },
    }
}

pub fn open_editor(app: &AppHandle, capture: &Capture) -> AppResult<()> {
    let label = format!("editor-{}", capture.id);
    let (img_w, img_h) = capture.image.dimensions();
    let (scale, work) = monitor_at(app, capture.source.rect.x, capture.source.rect.y);
    // Chrome: toolbar + status bar; keep the whole window inside 90% of the work area.
    let (chrome_w, chrome_h) = (48.0, 112.0);
    let (max_w, max_h) = work
        .map(|w| {
            (
                w.size.width as f64 / scale * 0.92,
                w.size.height as f64 / scale * 0.92,
            )
        })
        .unwrap_or((1600.0, 1000.0));
    let want_w = (img_w as f64 / scale + chrome_w).clamp(720.0, max_w.max(720.0));
    let want_h = (img_h as f64 / scale + chrome_h).clamp(480.0, max_h.max(480.0));

    let title = if capture.source.title.is_empty() {
        format!("QuickShot — {}×{}", img_w, img_h)
    } else {
        format!("QuickShot — {} ({}×{})", capture.source.title, img_w, img_h)
    };
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("editor.html".into()))
        .title(title)
        .inner_size(want_w, want_h)
        .min_inner_size(640.0, 440.0)
        .visible(false)
        .build()?;
    if let Some(w) = work {
        let px = w.position.x + ((w.size.width as f64 - want_w * scale) / 2.0).max(0.0) as i32;
        let py = w.position.y + ((w.size.height as f64 - want_h * scale) / 2.0).max(0.0) as i32;
        let _ = window.set_position(PhysicalPosition::new(px, py));
    } else {
        let _ = window.center();
    }
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

pub fn open_pin(app: &AppHandle, capture: &Capture, x: i32, y: i32) -> AppResult<()> {
    let label = format!("pin-{}", capture.id);
    let (img_w, img_h) = capture.image.dimensions();
    let (scale, _) = monitor_at(app, x, y);
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("pin.html".into()));
    // transparent so the pin's opacity (mouse wheel) shows what is underneath
    #[cfg(not(target_os = "macos"))]
    let builder = builder.transparent(true);
    let window = builder
        .title("QuickShot Pin")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .maximizable(false)
        .shadow(true)
        .visible(false)
        .inner_size(img_w as f64 / scale, img_h as f64 / scale)
        .min_inner_size(48.0, 48.0)
        .build()?;
    let _ = window.set_position(PhysicalPosition::new(x, y));
    let _ = window.set_size(LogicalSize::new(img_w as f64 / scale, img_h as f64 / scale));
    let _ = window.show();
    let _ = window.set_focus();
    Ok(())
}

/// Open a pin window from a synchronous command without deadlocking on Windows:
/// hop to a worker thread, then back onto the main thread via the event loop.
pub fn open_pin_deferred(app: &AppHandle, capture: Arc<Capture>, x: i32, y: i32) {
    let app = app.clone();
    std::thread::spawn(move || {
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Err(e) = open_pin(&app2, &capture, x, y) {
                log::error!("pin failed: {e}");
            }
        });
    });
}
