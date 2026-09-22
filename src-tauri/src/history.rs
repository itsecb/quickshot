//! Capture history: every capture is kept on disk so it can be browsed, reopened, pinned or
//! dragged out later.
//!
//! Layout (in the app's local data folder, never roamed):
//!   history/<id>/<file name>.png   full image, named like a saved screenshot so drag-out looks right
//!   history/<id>/thumb.jpg         thumbnail for the history window
//!   history/<id>/meta.json         written last; an entry without it is incomplete
//! `<id>` is a millisecond timestamp, so ids sort by age and double as the capture time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::error::{AppError, AppResult};
use crate::image_util::{decode_png, encode_jpeg, encode_png};
use crate::settings::{self, Settings};
use crate::state::{AppState, Capture, CaptureSource};

pub const CHANGED_EVENT: &str = "history://changed";
const META: &str = "meta.json";
pub const THUMB: &str = "thumb.jpg";
const THUMB_MAX: u32 = 480;
const DAY_MS: u64 = 24 * 3600 * 1000;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: u64,
    pub created: String,
    pub width: u32,
    pub height: u32,
    pub file_name: String,
    pub source: CaptureSource,
}

pub fn dir<R: Runtime>(app: &AppHandle<R>) -> AppResult<PathBuf> {
    let dir = app.path().app_local_data_dir()?.join("history");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn entry_dir<R: Runtime>(app: &AppHandle<R>, id: u64) -> AppResult<PathBuf> {
    Ok(dir(app)?.join(id.to_string()))
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

/// Millisecond timestamp, bumped when two captures land in the same millisecond.
fn new_id() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = now_ms();
    let mut prev = LAST.load(Ordering::Relaxed);
    loop {
        let next = now.max(prev + 1);
        match LAST.compare_exchange_weak(prev, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(p) => prev = p,
        }
    }
}

fn thumbnail(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let longest = w.max(h);
    if longest <= THUMB_MAX {
        return img.clone();
    }
    let scale = THUMB_MAX as f64 / longest as f64;
    let tw = ((w as f64 * scale).round() as u32).max(1);
    let th = ((h as f64 * scale).round() as u32).max(1);
    image::imageops::thumbnail(img, tw, th)
}

/// Record a capture in the background so encoding never delays the editor or clipboard.
pub fn spawn_record(app: &AppHandle, capture: Arc<Capture>) {
    if !app.state::<AppState>().settings().history_enabled {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = record(&app, &capture) {
            log::error!("history: could not record capture: {e}");
        }
    });
}

fn record(app: &AppHandle, capture: &Capture) -> AppResult<()> {
    let settings = app.state::<AppState>().settings();
    let id = new_id();
    let dir = entry_dir(app, id)?;
    std::fs::create_dir_all(&dir)?;
    let (width, height) = capture.image.dimensions();
    let stem = settings::expand_pattern(
        &settings.file_pattern,
        &capture.source.app_name,
        &capture.source.title,
        width,
        height,
    );
    let file_name = format!("{stem}.png");
    std::fs::write(dir.join(&file_name), encode_png(&capture.image)?)?;
    std::fs::write(
        dir.join(THUMB),
        encode_jpeg(&thumbnail(&capture.image), 82)?,
    )?;
    let entry = HistoryEntry {
        id,
        created: capture.created.to_rfc3339(),
        width,
        height,
        file_name,
        source: capture.source.clone(),
    };
    std::fs::write(dir.join(META), serde_json::to_vec_pretty(&entry)?)?;
    prune(app, &settings);
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

fn read_entry(dir: &Path) -> Option<HistoryEntry> {
    let text = std::fs::read_to_string(dir.join(META)).ok()?;
    serde_json::from_str(&text).ok()
}

/// All complete entries, newest first.
pub fn list(app: &AppHandle) -> AppResult<Vec<HistoryEntry>> {
    let mut out: Vec<HistoryEntry> = std::fs::read_dir(dir(app)?)?
        .flatten()
        .filter_map(|e| read_entry(&e.path()))
        .collect();
    out.sort_by_key(|e| std::cmp::Reverse(e.id));
    Ok(out)
}

pub fn entry(app: &AppHandle, id: u64) -> AppResult<HistoryEntry> {
    read_entry(&entry_dir(app, id)?).ok_or_else(|| AppError::NotFound(format!("history item {id}")))
}

pub fn image_path<R: Runtime>(app: &AppHandle<R>, id: u64) -> AppResult<PathBuf> {
    let dir = entry_dir(app, id)?;
    let entry = read_entry(&dir).ok_or_else(|| AppError::NotFound(format!("history item {id}")))?;
    Ok(dir.join(entry.file_name))
}

pub fn thumb_path<R: Runtime>(app: &AppHandle<R>, id: u64) -> AppResult<PathBuf> {
    Ok(entry_dir(app, id)?.join(THUMB))
}

pub fn load_image(app: &AppHandle, id: u64) -> AppResult<RgbaImage> {
    decode_png(&std::fs::read(image_path(app, id)?)?)
}

pub fn delete(app: &AppHandle, ids: &[u64]) -> AppResult<()> {
    for id in ids {
        let dir = entry_dir(app, *id)?;
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
    }
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

pub fn clear(app: &AppHandle) -> AppResult<()> {
    let ids: Vec<u64> = list(app)?.iter().map(|e| e.id).collect();
    delete(app, &ids)
}

/// Ids of history folders on disk, complete or not.
fn ids_on_disk(dir: &Path) -> Vec<u64> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse::<u64>().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Which ids fall outside the retention limits (count and age). `ids` must be newest first.
fn over_limits(ids: &[u64], now: u64, max_items: u32, max_days: u32) -> Vec<u64> {
    let cutoff = (max_days > 0).then(|| now.saturating_sub(max_days as u64 * DAY_MS));
    ids.iter()
        .enumerate()
        .filter(|(i, id)| {
            (max_items > 0 && *i >= max_items as usize) || cutoff.is_some_and(|c| **id < c)
        })
        .map(|(_, id)| *id)
        .collect()
}

/// Enforce the retention settings and drop entries left incomplete by a crash.
pub fn prune(app: &AppHandle, settings: &Settings) {
    let Ok(dir) = dir(app) else { return };
    let now = now_ms();
    let mut complete: Vec<u64> = Vec::new();
    for id in ids_on_disk(&dir) {
        if dir.join(id.to_string()).join(META).exists() {
            complete.push(id);
        } else if now.saturating_sub(id) > 3600 * 1000 {
            // an hour old and still no meta.json: a write that never finished
            let _ = std::fs::remove_dir_all(dir.join(id.to_string()));
        }
    }
    complete.sort_unstable_by_key(|id| std::cmp::Reverse(*id));
    for id in over_limits(
        &complete,
        now,
        settings.history_max_items,
        settings.history_max_days,
    ) {
        let _ = std::fs::remove_dir_all(dir.join(id.to_string()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_by_count_and_age() {
        let now = 100 * DAY_MS;
        let ids = [now - 1, now - 2 * DAY_MS, now - 40 * DAY_MS];
        assert!(over_limits(&ids, now, 0, 0).is_empty());
        assert_eq!(over_limits(&ids, now, 2, 0), vec![now - 40 * DAY_MS]);
        assert_eq!(over_limits(&ids, now, 0, 30), vec![now - 40 * DAY_MS]);
        assert_eq!(
            over_limits(&ids, now, 1, 30),
            vec![now - 2 * DAY_MS, now - 40 * DAY_MS]
        );
    }

    #[test]
    fn ids_are_unique_and_increasing() {
        let a = new_id();
        let b = new_id();
        assert!(b > a);
    }

    #[test]
    fn thumbnail_fits_bounds() {
        let img = RgbaImage::new(1920, 1080);
        let t = thumbnail(&img);
        assert_eq!(t.width(), THUMB_MAX);
        assert_eq!(t.height(), 270);
        let small = RgbaImage::new(100, 50);
        assert_eq!(thumbnail(&small).dimensions(), (100, 50));
    }
}
