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
    std::thread::spawn(move || history::prune(&prune_app, &prune_settings));
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
        _ => windows::show_main(&app),
    }
}

#[tauri::command]
pub fn hide_main(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}
