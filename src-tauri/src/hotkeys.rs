use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::settings::Settings;
use crate::state::CaptureMode;
use crate::{capture, windows};

/// (Re)register every global hotkey from settings. Returns human-readable conflicts.
pub fn register_all(app: &AppHandle, settings: &Settings) -> Vec<String> {
    let gs = app.global_shortcut();
    if let Err(e) = gs.unregister_all() {
        log::warn!("unregister_all failed: {e}");
    }
    let bindings = [
        (settings.hotkeys.region.as_str(), CaptureMode::Region),
        (settings.hotkeys.window.as_str(), CaptureMode::Window),
        (
            settings.hotkeys.fullscreen.as_str(),
            CaptureMode::Fullscreen,
        ),
        (
            settings.hotkeys.repeat_last.as_str(),
            CaptureMode::RepeatLast,
        ),
        (settings.hotkeys.ocr.as_str(), CaptureMode::Ocr),
        (settings.hotkeys.pin.as_str(), CaptureMode::Pin),
        (settings.hotkeys.color.as_str(), CaptureMode::Color),
    ];
    let mut conflicts = Vec::new();
    for (accel, mode) in bindings {
        let accel = accel.trim();
        if accel.is_empty() {
            continue;
        }
        let result = gs.on_shortcut(accel, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                capture::trigger(app, mode);
            }
        });
        if let Err(e) = result {
            log::warn!("hotkey {accel} ({mode:?}) not registered: {e}");
            conflicts.push(format!("{accel}: {e}"));
        }
    }
    let history = settings.hotkeys.history.trim();
    if !history.is_empty() {
        let result = gs.on_shortcut(history, |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                windows::open_history(app);
            }
        });
        if let Err(e) = result {
            log::warn!("hotkey {history} (history) not registered: {e}");
            conflicts.push(format!("{history}: {e}"));
        }
    }
    conflicts
}
