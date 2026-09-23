use serde::Serialize;
use tauri::{AppHandle, State};

use crate::capture::{self, MonitorInfo, WindowInfo};
use crate::error::{AppError, AppResult};
use crate::geom::Rect;
use crate::overlay;
use crate::state::{AppState, CaptureMode};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OverlayInit {
    pub mode: CaptureMode,
    pub monitor: MonitorInfo,
    pub monitors: Vec<MonitorInfo>,
    pub windows: Vec<WindowInfo>,
    pub last_region: Option<Rect>,
    pub show_magnifier: bool,
    pub play_sound: bool,
}

#[tauri::command]
pub fn overlay_init(state: State<'_, AppState>, label: String) -> AppResult<OverlayInit> {
    let id = overlay::monitor_id_from_label(&label)
        .ok_or_else(|| AppError::Other("bad overlay label".into()))?;
    let guard = state.session.lock().unwrap();
    let session = guard
        .as_ref()
        .ok_or_else(|| AppError::Other("no capture session".into()))?;
    let monitor = session
        .frames
        .iter()
        .find(|f| f.monitor.id == id)
        .map(|f| f.monitor.clone())
        .ok_or_else(|| AppError::NotFound(format!("monitor {id}")))?;
    Ok(OverlayInit {
        mode: session.mode,
        monitor,
        monitors: session.frames.iter().map(|f| f.monitor.clone()).collect(),
        windows: session.windows.clone(),
        last_region: *state.last_region.lock().unwrap(),
        show_magnifier: state.settings().show_magnifier,
        play_sound: state.settings().play_sound,
    })
}

#[tauri::command]
pub fn overlay_ready(app: AppHandle, label: String) -> AppResult<()> {
    capture::overlay_ready(&app, &label)
}

#[tauri::command(async)]
pub fn finish_capture(app: AppHandle, rect: Rect, window_id: Option<u32>) -> AppResult<u64> {
    capture::finish(&app, rect, window_id)
}

#[tauri::command]
pub fn cancel_capture(app: AppHandle) {
    capture::cancel(&app);
}

#[tauri::command]
pub fn trigger_capture(app: AppHandle, mode: CaptureMode, delay: Option<u32>) {
    capture::trigger_delayed(&app, mode, delay.unwrap_or(0).min(60));
}

/// Cancel button in the countdown window.
#[tauri::command]
pub fn cancel_countdown(state: State<'_, AppState>) {
    state
        .countdown_cancel
        .store(true, std::sync::atomic::Ordering::SeqCst);
}
