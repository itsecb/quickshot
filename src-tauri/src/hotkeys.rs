use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::settings::Settings;
use crate::state::CaptureMode;
use crate::{capture, windows};

/// Older settings could store the shifted symbol of a digit key ("Alt+Shift+$" for Alt+Shift+4),
/// which the shortcut parser rejects: map those back to the digit (US layout).
fn normalize(accel: &str) -> String {
    let accel = accel.trim();
    let Some((mods, key)) = accel.rsplit_once('+') else {
        return accel.to_string();
    };
    const SHIFTED: &str = ")!@#$%^&*(";
    match key.chars().next().and_then(|c| SHIFTED.find(c)) {
        Some(digit) if key.chars().count() == 1 => format!("{mods}+{digit}"),
        _ => accel.to_string(),
    }
}

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
        (settings.hotkeys.qr.as_str(), CaptureMode::Qr),
        (settings.hotkeys.watch.as_str(), CaptureMode::Watch),
        (settings.hotkeys.scroll.as_str(), CaptureMode::Scroll),
        (settings.hotkeys.record_gif.as_str(), CaptureMode::Record),
    ];
    let mut conflicts = Vec::new();
    for (accel, mode) in bindings {
        let accel = normalize(accel);
        let accel = accel.as_str();
        if accel.is_empty() {
            continue;
        }
        let result = gs.on_shortcut(accel, move |app, _shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            // the same hotkey stops a GIF recording / scrolling capture that's running
            match mode {
                CaptureMode::Record if crate::record::is_recording() => crate::record::stop(),
                CaptureMode::Scroll if crate::scroll::is_running() => crate::scroll::stop(),
                _ => capture::trigger(app, mode),
            }
        });
        if let Err(e) = result {
            log::warn!("hotkey {accel} ({mode:?}) not registered: {e}");
            conflicts.push(format!("{accel}: {e}"));
        }
    }
    let delayed = normalize(&settings.hotkeys.delayed_region);
    let delayed = delayed.as_str();
    if !delayed.is_empty() {
        let secs = settings.capture_delay_secs.clamp(1, 60);
        let result = gs.on_shortcut(delayed, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                capture::trigger_delayed(app, CaptureMode::Region, secs);
            }
        });
        if let Err(e) = result {
            log::warn!("hotkey {delayed} (delayed region) not registered: {e}");
            conflicts.push(format!("{delayed}: {e}"));
        }
    }
    let record = normalize(&settings.hotkeys.record_steps);
    let record = record.as_str();
    if !record.is_empty() {
        let result = gs.on_shortcut(record, |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                crate::steps::toggle(app);
            }
        });
        if let Err(e) = result {
            log::warn!("hotkey {record} (step recorder) not registered: {e}");
            conflicts.push(format!("{record}: {e}"));
        }
    }
    let history = normalize(&settings.hotkeys.history);
    let history = history.as_str();
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
