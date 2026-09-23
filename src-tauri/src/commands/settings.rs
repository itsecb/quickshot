use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

use crate::error::AppResult;
use crate::settings::{self, Settings};
use crate::state::AppState;
use crate::{history, hotkeys, tray, windows};

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub hotkey_conflicts: Vec<String>,
}

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> AppResult<ApplyResult> {
    settings::save(&app, &settings)?;
    *state.settings.write().unwrap() = settings.clone();
    let hotkey_conflicts = hotkeys::register_all(&app, &settings);
    tray::refresh(&app, &settings);
    apply_autostart(&app, settings.autostart);
    // apply new retention limits right away
    let (prune_app, prune_settings) = (app.clone(), settings.clone());
    std::thread::spawn(move || {
        history::prune(&prune_app, &prune_settings);
        // picks up entries captured while history OCR was switched off
        if prune_settings.history_ocr {
            history::queue_backfill(&prune_app);
        }
    });
    let _ = app.emit("settings://changed", &settings);
    Ok(ApplyResult { hotkey_conflicts })
}

pub fn apply_autostart(app: &AppHandle, enabled: bool) {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    if let Err(e) = result {
        log::warn!("autostart update failed: {e}");
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub save_dir: String,
    pub guides_dir: String,
    pub config_dir: String,
    pub version: String,
    pub platform: String,
}

#[tauri::command]
pub fn app_paths(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppPaths> {
    let settings = state.settings();
    Ok(AppPaths {
        save_dir: settings::save_dir(&app, &settings)?.display().to_string(),
        guides_dir: settings::guides_dir(&app, &settings)?.display().to_string(),
        config_dir: app.path().app_config_dir()?.display().to_string(),
        version: app.package_info().version.to_string(),
        platform: std::env::consts::OS.to_string(),
    })
}

#[tauri::command(async)]
pub fn open_window(app: AppHandle, name: String) {
    match name.as_str() {
        "guide" => windows::open_guide(&app),
        "history" => windows::open_history(&app),
        "watches" => windows::open_watches(&app),
        _ => windows::show_main(&app),
    }
}

#[tauri::command]
pub fn hide_main(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

/// Windows 11's "Use the Print Screen key to open screen capture" (Snipping Tool). While it's
/// on, Print Screen can't be a QuickShot hotkey without both opening. `None` when the setting
/// isn't stored (older Windows, or never changed: then it follows the Windows default).
#[tauri::command]
pub fn print_screen_snipping() -> Option<bool> {
    snipping::get()
}

/// Turn that Windows setting off for the current user.
#[tauri::command]
pub fn disable_print_screen_snipping() -> AppResult<()> {
    snipping::set(false)
}

#[cfg(windows)]
mod snipping {
    use std::ffi::c_void;

    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD,
    };

    use crate::error::{AppError, AppResult};

    pub fn get() -> Option<bool> {
        let (mut data, mut size) = (0u32, 4u32);
        // SAFETY: reads a DWORD into a 4-byte buffer whose size is passed along.
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Control Panel\\Keyboard"),
                w!("PrintScreenKeyForSnippingEnabled"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut data as *mut u32 as *mut c_void),
                Some(&mut size),
            )
        };
        result.is_ok().then_some(data != 0)
    }

    pub fn set(enabled: bool) -> AppResult<()> {
        let data = enabled as u32;
        // SAFETY: writes a 4-byte DWORD from a live local.
        let result = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                w!("Control Panel\\Keyboard"),
                w!("PrintScreenKeyForSnippingEnabled"),
                REG_DWORD.0,
                Some(&data as *const u32 as *const c_void),
                4,
            )
        };
        result
            .ok()
            .map_err(|e| AppError::Other(format!("couldn't change the Windows setting: {e}")))
    }
}

#[cfg(not(windows))]
mod snipping {
    use crate::error::{AppError, AppResult};

    pub fn get() -> Option<bool> {
        None
    }

    pub fn set(_enabled: bool) -> AppResult<()> {
        Err(AppError::Other("only on Windows".into()))
    }
}
