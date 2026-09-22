use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::diff;
use crate::error::AppResult;
use crate::history::{self, HistoryEntry};
use crate::state::{AppState, Capture};
use crate::{output, protocol, windows};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    #[serde(flatten)]
    pub entry: HistoryEntry,
    pub thumb_url: String,
    pub png_url: String,
    /// Full path of the image on disk, for native drag-out.
    pub path: String,
    pub thumb_path: String,
}

#[tauri::command(async)]
pub fn history_list(app: AppHandle) -> AppResult<Vec<HistoryItem>> {
    let dir = history::dir(&app)?;
    Ok(history::list(&app)?
        .into_iter()
        .map(|entry| {
            let entry_dir = dir.join(entry.id.to_string());
            HistoryItem {
                thumb_url: protocol::history_thumb_url(entry.id),
                png_url: protocol::history_png_url(entry.id),
                path: entry_dir.join(&entry.file_name).display().to_string(),
                thumb_path: entry_dir.join(history::THUMB).display().to_string(),
                entry,
            }
        })
        .collect())
}

#[tauri::command(async)]
pub fn history_dir(app: AppHandle) -> AppResult<String> {
    Ok(history::dir(&app)?.display().to_string())
}

/// Load a history image back into memory as a live capture (without re-recording it).
fn restore(app: &AppHandle, id: u64) -> AppResult<std::sync::Arc<Capture>> {
    let entry = history::entry(app, id)?;
    let image = history::load_image(app, id)?;
    Ok(app
        .state::<AppState>()
        .insert_capture(app, image, entry.source))
}

#[tauri::command(async)]
pub fn history_open(app: AppHandle, id: u64) -> AppResult<()> {
    let capture = restore(&app, id)?;
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = windows::open_editor(&app2, &capture) {
            log::error!("editor failed: {e}");
        }
    })?;
    Ok(())
}

#[tauri::command(async)]
pub fn history_pin(app: AppHandle, id: u64) -> AppResult<()> {
    let capture = restore(&app, id)?;
    // back where it was captured, unless that spot is no longer on any monitor
    let (x, y) = (capture.source.rect.x, capture.source.rect.y);
    let on_screen = matches!(app.monitor_from_point(x as f64, y as f64), Ok(Some(_)));
    let (x, y) = if on_screen { (x, y) } else { (80, 80) };
    windows::open_pin_deferred(&app, capture, x, y);
    Ok(())
}

#[tauri::command(async)]
pub fn history_copy(app: AppHandle, id: u64) -> AppResult<()> {
    output::copy_to_clipboard(&app, &history::load_image(&app, id)?)
}

/// Save a copy into the normal screenshots folder. Returns the saved path.
#[tauri::command(async)]
pub fn history_save(app: AppHandle, id: u64) -> AppResult<String> {
    let entry = history::entry(&app, id)?;
    let image = history::load_image(&app, id)?;
    let settings = app.state::<AppState>().settings();
    let path = output::save_to_folder(&app, &settings, &image, &entry.source, None)?;
    Ok(path.display().to_string())
}

#[tauri::command(async)]
pub fn history_delete(app: AppHandle, ids: Vec<u64>) -> AppResult<()> {
    history::delete(&app, &ids)
}

#[tauri::command(async)]
pub fn history_clear(app: AppHandle) -> AppResult<()> {
    history::clear(&app)
}

#[tauri::command(async)]
pub fn history_set_star(app: AppHandle, id: u64, starred: bool) -> AppResult<()> {
    history::update(&app, id, |e| e.starred = starred).map(|_| ())
}

#[tauri::command(async)]
pub fn history_set_note(app: AppHandle, id: u64, note: String) -> AppResult<()> {
    let note: String = note.trim().chars().take(2000).collect();
    history::update(&app, id, move |e| e.note = note).map(|_| ())
}

/// Copy with a context caption (see "Copy for ticket"). Returns the caption used.
#[tauri::command(async)]
pub fn history_copy_rich(app: AppHandle, id: u64) -> AppResult<String> {
    let entry = history::entry(&app, id)?;
    let png = std::fs::read(history::image_path(&app, id)?)?;
    let image = crate::image_util::decode_png(&png)?;
    let created = chrono::DateTime::parse_from_rfc3339(&entry.created)
        .map(|d| d.with_timezone(&chrono::Local))
        .unwrap_or_else(|_| chrono::Local::now());
    let settings = app.state::<AppState>().settings();
    let caption =
        output::ticket_caption(&settings, &entry.source, created, entry.width, entry.height);
    output::copy_rich(&app, &image, &png, &caption)?;
    Ok(caption)
}

/// Changed areas between two history captures (`a` = before, `b` = after).
#[tauri::command(async)]
pub fn history_diff(app: AppHandle, a: u64, b: u64) -> AppResult<diff::DiffResult> {
    let before = history::load_image(&app, a)?;
    let after = history::load_image(&app, b)?;
    Ok(diff::diff(&before, &after, &diff::DiffOptions::default()))
}

/// Open the compare window, older capture as "before".
#[tauri::command(async)]
pub fn history_compare(app: AppHandle, a: u64, b: u64) -> AppResult<()> {
    let (before, after) = (a.min(b), a.max(b));
    let app2 = app.clone();
    app.run_on_main_thread(move || windows::open_compare(&app2, before, after))?;
    Ok(())
}
