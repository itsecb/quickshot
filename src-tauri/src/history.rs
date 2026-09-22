//! Capture history: every capture is kept on disk so it can be browsed, reopened, pinned or
//! dragged out later.
//!
//! Layout (in the app's local data folder, never roamed):
//!   history/<id>/<file name>.png   full image, named like a saved screenshot so drag-out looks right
//!   history/<id>/thumb.jpg         thumbnail for the history window
//!   history/<id>/meta.json         written last; an entry without it is incomplete
//! `<id>` is a millisecond timestamp, so ids sort by age and double as the capture time.
//!
//! A background worker OCRs each capture (newest first, backfilling older ones) and stores the
//! text in meta.json so the history window can search what is *in* the screenshots.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::error::{AppError, AppResult};
use crate::image_util::{decode_png, encode_jpeg, encode_png};
use crate::settings::{self, Settings};
use crate::state::{AppState, Capture, CaptureSource};

pub const CHANGED_EVENT: &str = "history://changed";
/// Payload `{ id, text }` when background OCR finishes one entry (cheaper than a full reload).
pub const OCR_EVENT: &str = "history://ocr";
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
    /// Text read from the image: `None` = not processed yet, `""` = no text found.
    #[serde(default)]
    pub ocr_text: Option<String>,
    /// Starred entries are never pruned.
    #[serde(default)]
    pub starred: bool,
    /// Free-text note; `#words` are shown as tags.
    #[serde(default)]
    pub note: String,
}

#[derive(Serialize, Clone)]
struct OcrDone {
    id: u64,
    text: String,
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
        ocr_text: None,
        starred: false,
        note: String::new(),
    };
    write_meta(&dir, &entry)?;
    prune(app, &settings);
    let _ = app.emit(CHANGED_EVENT, ());
    queue_ocr(id, true);
    Ok(())
}

/// meta.json is replaced atomically so a crash never leaves a half-written file.
fn write_meta(dir: &Path, entry: &HistoryEntry) -> AppResult<()> {
    let tmp = dir.join("meta.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(entry)?)?;
    std::fs::rename(tmp, dir.join(META))?;
    Ok(())
}

/// Read-modify-write one entry's metadata. Serialized so the OCR worker and the UI
/// never overwrite each other's changes.
fn update_quiet(
    app: &AppHandle,
    id: u64,
    f: impl FnOnce(&mut HistoryEntry),
) -> AppResult<HistoryEntry> {
    static META_LOCK: Mutex<()> = Mutex::new(());
    let _guard = META_LOCK.lock().unwrap();
    let dir = entry_dir(app, id)?;
    let mut entry =
        read_entry(&dir).ok_or_else(|| AppError::NotFound(format!("history item {id}")))?;
    f(&mut entry);
    write_meta(&dir, &entry)?;
    Ok(entry)
}

/// Update metadata and tell history windows to reload.
pub fn update(
    app: &AppHandle,
    id: u64,
    f: impl FnOnce(&mut HistoryEntry),
) -> AppResult<HistoryEntry> {
    let entry = update_quiet(app, id, f)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(entry)
}

// ---- background OCR ----

struct OcrQueue {
    ids: Mutex<VecDeque<u64>>,
    ready: Condvar,
}

fn ocr_queue() -> &'static OcrQueue {
    static QUEUE: OnceLock<OcrQueue> = OnceLock::new();
    QUEUE.get_or_init(|| OcrQueue {
        ids: Mutex::new(VecDeque::new()),
        ready: Condvar::new(),
    })
}

/// New captures jump the queue (`urgent`); backfill of older entries goes to the back.
fn queue_ocr(id: u64, urgent: bool) {
    let q = ocr_queue();
    let mut ids = q.ids.lock().unwrap();
    if ids.contains(&id) {
        return;
    }
    if urgent {
        ids.push_front(id);
    } else {
        ids.push_back(id);
    }
    q.ready.notify_one();
}

/// Queue every entry that has not been OCR'd yet, newest first.
pub fn queue_backfill(app: &AppHandle) {
    if let Ok(entries) = list(app) {
        for e in entries.iter().filter(|e| e.ocr_text.is_none()) {
            queue_ocr(e.id, false);
        }
    }
}

/// Start the single OCR worker thread. Call once at startup.
pub fn start_ocr_worker(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("history-ocr".into())
        .spawn(move || {
            queue_backfill(&app);
            loop {
                let id = {
                    let q = ocr_queue();
                    let mut ids = q.ids.lock().unwrap();
                    loop {
                        if let Some(id) = ids.pop_front() {
                            break id;
                        }
                        ids = q.ready.wait(ids).unwrap();
                    }
                };
                if let Err(e) = ocr_one(&app, id) {
                    log::debug!("history OCR skipped {id}: {e}");
                }
                // stay a polite background task while backfilling hundreds of entries
                std::thread::sleep(Duration::from_millis(50));
            }
        });
    if let Err(e) = spawned {
        log::error!("could not start history OCR worker: {e}");
    }
}

fn ocr_one(app: &AppHandle, id: u64) -> AppResult<()> {
    let settings = app.state::<AppState>().settings();
    if !settings.history_ocr {
        return Ok(());
    }
    match read_entry(&entry_dir(app, id)?) {
        Some(e) if e.ocr_text.is_none() => {}
        _ => return Ok(()), // deleted, or already done
    }
    let image = load_image(app, id)?;
    let text = match crate::ocr::recognize(&image, settings.ocr_language.as_deref()) {
        Ok(out) => out.text,
        Err(e) => {
            // record "no text" so a missing OCR engine doesn't retry every entry on every start
            log::warn!("history OCR failed for {id}: {e}");
            String::new()
        }
    };
    let stored = text.clone();
    update_quiet(app, id, move |e| e.ocr_text = Some(stored))?;
    let _ = app.emit(OCR_EVENT, OcrDone { id, text });
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

/// Delete everything except starred entries.
pub fn clear(app: &AppHandle) -> AppResult<()> {
    let ids: Vec<u64> = list(app)?
        .iter()
        .filter(|e| !e.starred)
        .map(|e| e.id)
        .collect();
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

/// Ids to prune from `(id, starred)` pairs: starred entries are exempt and don't count
/// towards the item limit.
fn to_prune(entries: &[(u64, bool)], now: u64, max_items: u32, max_days: u32) -> Vec<u64> {
    let mut ids: Vec<u64> = entries
        .iter()
        .filter(|(_, starred)| !starred)
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable_by_key(|id| std::cmp::Reverse(*id));
    over_limits(&ids, now, max_items, max_days)
}

/// Enforce the retention settings and drop entries left incomplete by a crash.
pub fn prune(app: &AppHandle, settings: &Settings) {
    let Ok(dir) = dir(app) else { return };
    let now = now_ms();
    let mut complete: Vec<(u64, bool)> = Vec::new();
    for id in ids_on_disk(&dir) {
        match read_entry(&dir.join(id.to_string())) {
            Some(e) => complete.push((id, e.starred)),
            None if now.saturating_sub(id) > 3600 * 1000 => {
                // an hour old and still no meta.json: a write that never finished
                let _ = std::fs::remove_dir_all(dir.join(id.to_string()));
            }
            None => {}
        }
    }
    for id in to_prune(
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
    fn starred_entries_are_never_pruned() {
        let now = 100 * DAY_MS;
        let old_starred = now - 90 * DAY_MS;
        let entries = [
            (now - 1, false),
            (now - 2, false),
            (old_starred, true),
            (now - 3, true),
        ];
        // limit 1: the newest unstarred stays; starred ones don't use up the limit
        assert_eq!(to_prune(&entries, now, 1, 30), vec![now - 2]);
        assert!(to_prune(&entries, now, 0, 0).is_empty());
    }

    #[test]
    fn meta_without_new_fields_still_loads() {
        let json = r#"{"id":1,"created":"2026-09-22T10:00:00+00:00","width":10,"height":10,
            "fileName":"a.png","source":{"kind":"region","appName":"","title":"",
            "monitorName":"","rect":{"x":0,"y":0,"width":10,"height":10}}}"#;
        let e: HistoryEntry = serde_json::from_str(json).unwrap();
        assert_eq!(e.ocr_text, None);
        assert!(!e.starred);
        assert!(e.note.is_empty());
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
