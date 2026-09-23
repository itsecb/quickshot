//! "Watch a region": keep an eye on part of the screen (a dashboard tile, a status page, a log
//! window) and alert when it changes, or when some text appears or disappears.
//!
//! Each watch runs on its own thread: capture the region live, compare with the last state,
//! and on an alert record before/after into History and show an alert card.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::capture_rect_live;
use crate::diff::{self, DiffOptions};
use crate::error::AppResult;
use crate::geom::Rect;
use crate::state::{AppState, CaptureSource};
use crate::watch_rules::{should_alert, Condition, Observation};
use crate::{history, ocr, windows};

pub const CHANGED_EVENT: &str = "watches://changed";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WatchInfo {
    pub id: u64,
    pub name: String,
    pub rect: Rect,
    pub interval_secs: u32,
    pub condition: Condition,
    pub started: String,
    pub last_check: Option<String>,
    pub checks: u32,
    pub alerts: u32,
    pub paused: bool,
    /// Latest problem (e.g. the region went off-screen), shown in the Watches window.
    pub error: Option<String>,
}

struct Watch {
    info: Mutex<WatchInfo>,
    stop: AtomicBool,
    snooze_until: Mutex<Option<Instant>>,
}

fn watches() -> &'static Mutex<Vec<Arc<Watch>>> {
    static WATCHES: OnceLock<Mutex<Vec<Arc<Watch>>>> = OnceLock::new();
    WATCHES.get_or_init(|| Mutex::new(Vec::new()))
}

fn find(id: u64) -> Option<Arc<Watch>> {
    watches()
        .lock()
        .unwrap()
        .iter()
        .find(|w| w.info.lock().unwrap().id == id)
        .cloned()
}

pub fn list() -> Vec<WatchInfo> {
    watches()
        .lock()
        .unwrap()
        .iter()
        .map(|w| w.info.lock().unwrap().clone())
        .collect()
}

/// Tell the Watches window; `tray` also rebuilds the tray menu (only when the set of watches
/// changes, not on every check, or the tray icon would flicker).
fn changed(app: &AppHandle, tray: bool) {
    let _ = app.emit(CHANGED_EVENT, list());
    if tray {
        let settings = app.state::<AppState>().settings();
        crate::tray::refresh(app, &settings);
    }
}

/// Start watching `rect` (global physical px). Returns the watch id.
pub fn start(app: &AppHandle, rect: Rect, name: String) -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let watch = Arc::new(Watch {
        info: Mutex::new(WatchInfo {
            id,
            name: if name.trim().is_empty() {
                format!("Region {}×{}", rect.width, rect.height)
            } else {
                name
            },
            rect,
            interval_secs: 10,
            condition: Condition::default(),
            started: chrono::Local::now().to_rfc3339(),
            last_check: None,
            checks: 0,
            alerts: 0,
            paused: false,
            error: None,
        }),
        stop: AtomicBool::new(false),
        snooze_until: Mutex::new(None),
    });
    watches().lock().unwrap().push(watch.clone());
    let app2 = app.clone();
    std::thread::Builder::new()
        .name(format!("watch-{id}"))
        .spawn(move || run(&app2, &watch))
        .ok();
    changed(app, true);
    id
}

pub fn stop(app: &AppHandle, id: u64) {
    if let Some(w) = find(id) {
        w.stop.store(true, Ordering::SeqCst);
    }
    watches()
        .lock()
        .unwrap()
        .retain(|w| w.info.lock().unwrap().id != id);
    changed(app, true);
}

pub fn stop_all(app: &AppHandle) {
    let ids: Vec<u64> = list().iter().map(|w| w.id).collect();
    for id in ids {
        stop(app, id);
    }
}

pub fn update(app: &AppHandle, id: u64, interval_secs: u32, condition: Condition, paused: bool) {
    if let Some(w) = find(id) {
        let mut info = w.info.lock().unwrap();
        info.interval_secs = interval_secs.clamp(2, 3600);
        info.condition = condition;
        info.paused = paused;
    }
    changed(app, false);
}

pub fn snooze(app: &AppHandle, id: u64, minutes: u32) {
    if let Some(w) = find(id) {
        *w.snooze_until.lock().unwrap() =
            Some(Instant::now() + Duration::from_secs(minutes as u64 * 60));
    }
    changed(app, false);
}

/// Sleep up to `secs`, waking early when the watch is stopped. False = stopped.
fn wait(watch: &Watch, secs: u32) -> bool {
    let until = Instant::now() + Duration::from_secs(secs as u64);
    while Instant::now() < until {
        if watch.stop.load(Ordering::SeqCst) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    !watch.stop.load(Ordering::SeqCst)
}

fn run(app: &AppHandle, watch: &Watch) {
    let language = app.state::<AppState>().settings().ocr_language;
    let read_text = |img: &RgbaImage| {
        ocr::recognize(img, language.as_deref())
            .map(|o| o.text)
            .unwrap_or_default()
    };
    let mut baseline: Option<RgbaImage> = None;
    let mut was_present = false;
    let mut last_cond: Option<Condition> = None;
    loop {
        let (interval, rect, cond, paused) = {
            let i = watch.info.lock().unwrap();
            (i.interval_secs, i.rect, i.condition.clone(), i.paused)
        };
        if last_cond.as_ref() != Some(&cond) {
            // new rule: look again first, so text that's already there doesn't alert at once
            baseline = None;
            was_present = false;
            last_cond = Some(cond.clone());
        }
        if !wait(watch, if baseline.is_none() { 0 } else { interval }) {
            return;
        }
        if paused {
            continue;
        }
        let current = match capture_rect_live(rect) {
            Ok(img) => img,
            Err(e) => {
                watch.info.lock().unwrap().error = Some(e.to_string());
                changed(app, false);
                continue;
            }
        };
        let Some(before) = baseline.take() else {
            // first look: establishes the baseline (and the text state)
            if !matches!(cond, Condition::Change { .. }) {
                let text = read_text(&current);
                was_present = should_alert(
                    &cond,
                    false,
                    &Observation {
                        changed_percent: 0.0,
                        has_boxes: false,
                        text: Some(&text),
                    },
                )
                .1;
            }
            baseline = Some(current);
            continue;
        };
        let d = diff::diff(
            &before,
            &current,
            &DiffOptions {
                max_shift: 0,
                ..Default::default()
            },
        );
        let text = match cond {
            Condition::Change { .. } => None,
            _ => Some(read_text(&current)),
        };
        let (alert, present) = should_alert(
            &cond,
            was_present,
            &Observation {
                changed_percent: d.changed_percent,
                has_boxes: !d.boxes.is_empty(),
                text: text.as_deref(),
            },
        );
        was_present = present;
        {
            let mut i = watch.info.lock().unwrap();
            i.checks += 1;
            i.last_check = Some(chrono::Local::now().to_rfc3339());
            i.error = None;
        }
        let snoozed = watch
            .snooze_until
            .lock()
            .unwrap()
            .is_some_and(|t| Instant::now() < t);
        if alert && !snoozed {
            if let Err(e) = raise(app, watch, &before, &current, &d.boxes, &cond) {
                log::warn!("watch alert failed: {e}");
            }
        }
        // compare against the latest state, so one change alerts once
        baseline = Some(current);
        changed(app, false);
    }
}

fn reason(cond: &Condition, changed_percent: f64) -> String {
    match cond {
        Condition::Change { .. } => format!("{changed_percent:.1}% of the region changed"),
        Condition::TextAppears { pattern } => format!("“{pattern}” appeared"),
        Condition::TextGone { pattern } => format!("“{pattern}” went away"),
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AlertInit {
    watch_id: u64,
    name: String,
    reason: String,
    before_id: u64,
    after_id: u64,
    after_url: String,
    width: u32,
    height: u32,
    boxes: Vec<Rect>,
}

fn raise(
    app: &AppHandle,
    watch: &Watch,
    before: &RgbaImage,
    after: &RgbaImage,
    boxes: &[Rect],
    cond: &Condition,
) -> AppResult<()> {
    let (id, name, rect) = {
        let mut i = watch.info.lock().unwrap();
        i.alerts += 1;
        (i.id, i.name.clone(), i.rect)
    };
    let changed_percent = if boxes.is_empty() {
        0.0
    } else {
        let area: u64 = boxes.iter().map(|b| b.width as u64 * b.height as u64).sum();
        area as f64 * 100.0 / (rect.width as u64 * rect.height as u64).max(1) as f64
    };
    let why = reason(cond, changed_percent);
    let source = CaptureSource {
        kind: "watch".into(),
        title: name.clone(),
        rect,
        ..Default::default()
    };
    let note = format!("#watch {name}: {why}");
    let before_id =
        history::record_image(app, before, &source, &format!("#watch {name} (before)"))?;
    let after_id = history::record_image(app, after, &source, &note)?;
    let init = AlertInit {
        watch_id: id,
        name: name.clone(),
        reason: why.clone(),
        before_id,
        after_id,
        after_url: crate::protocol::history_png_url(after_id),
        width: after.width(),
        height: after.height(),
        boxes: boxes.to_vec(),
    };
    let settings = app.state::<AppState>().settings();
    if settings.show_thumbnail {
        let app2 = app.clone();
        app.run_on_main_thread(move || {
            if let Err(e) =
                windows::open_alert(&app2, id, &serde_json::to_string(&init).unwrap_or_default())
            {
                log::error!("watch alert window failed: {e}");
            }
        })?;
    } else {
        windows::toast(app, &format!("Watch: {name}"), &why);
    }
    Ok(())
}
