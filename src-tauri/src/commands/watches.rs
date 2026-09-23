use tauri::AppHandle;

use crate::watch::{self, WatchInfo};
use crate::watch_rules::Condition;

#[tauri::command]
pub fn watch_list() -> Vec<WatchInfo> {
    watch::list()
}

#[tauri::command(async)]
pub fn watch_update(
    app: AppHandle,
    id: u64,
    interval_secs: u32,
    condition: Condition,
    paused: bool,
) {
    watch::update(&app, id, interval_secs, condition, paused);
}

#[tauri::command(async)]
pub fn watch_stop(app: AppHandle, id: u64) {
    watch::stop(&app, id);
}

#[tauri::command(async)]
pub fn watch_snooze(app: AppHandle, id: u64, minutes: u32) {
    watch::snooze(&app, id, minutes);
}

#[tauri::command(async)]
pub fn watch_stop_all(app: AppHandle) {
    watch::stop_all(&app);
}
