use tauri::AppHandle;

use crate::error::AppResult;
use crate::steps::{self, RecorderState};

#[tauri::command(async)]
pub fn steps_start(app: AppHandle) -> AppResult<()> {
    steps::start(&app)
}

#[tauri::command(async)]
pub fn steps_stop(app: AppHandle) -> usize {
    steps::stop(&app)
}

#[tauri::command(async)]
pub fn steps_pause(app: AppHandle, paused: bool) {
    steps::set_paused(&app, paused);
}

#[tauri::command(async)]
pub fn steps_add_now(app: AppHandle) -> AppResult<()> {
    steps::add_now(&app)
}

#[tauri::command]
pub fn steps_state() -> RecorderState {
    steps::state()
}
