use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::capture::{CaptureFrame, WindowInfo};
use crate::commands::guide::PendingStep;
use crate::geom::Rect;
use crate::settings::Settings;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CaptureMode {
    Region,
    Window,
    Fullscreen,
    RepeatLast,
    Ocr,
    Pin,
    Color,
    Qr,
    Watch,
}

impl CaptureMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "region" => Self::Region,
            "window" => Self::Window,
            "fullscreen" | "full" | "screen" => Self::Fullscreen,
            "repeat" | "repeat-last" | "repeatLast" => Self::RepeatLast,
            "ocr" => Self::Ocr,
            "pin" => Self::Pin,
            "color" => Self::Color,
            "qr" | "barcode" => Self::Qr,
            "watch" => Self::Watch,
            _ => return None,
        })
    }
}

/// Where a capture came from; shown in the editor title and used in filename patterns.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSource {
    pub kind: String, // "region" | "window" | "monitor"
    pub app_name: String,
    pub title: String,
    pub monitor_name: String,
    pub rect: Rect,
}

/// A live overlay session: frozen frames for each monitor plus the mode the user picked.
pub struct CaptureSession {
    pub mode: CaptureMode,
    pub frames: Vec<CaptureFrame>,
    pub windows: Vec<WindowInfo>,
    pub labels: Vec<String>,
    pub ready: HashSet<String>,
    pub shown: bool,
}

/// A finished capture kept in memory while an editor / pin window references it.
pub struct Capture {
    pub id: u64,
    pub image: RgbaImage,
    pub source: CaptureSource,
    pub created: chrono::DateTime<chrono::Local>,
}

/// `editor-<id>` / `pin-<id>` / `thumb-<id>` -> id. Other labels (overlay-<monitor>) never match.
pub fn capture_id_from_label(label: &str) -> Option<u64> {
    let rest = label
        .strip_prefix("editor-")
        .or_else(|| label.strip_prefix("pin-"))
        .or_else(|| label.strip_prefix("thumb-"))?;
    rest.parse().ok()
}

pub struct AppState {
    pub settings: RwLock<Settings>,
    pub session: Mutex<Option<CaptureSession>>,
    pub captures: Mutex<HashMap<u64, Arc<Capture>>>,
    pub last_region: Mutex<Option<Rect>>,
    pub temp_files: Mutex<Vec<PathBuf>>,
    pub pending_steps: Mutex<Vec<PendingStep>>,
    /// A capture was triggered and has not reached the overlay/output yet (incl. countdown).
    pub capture_pending: AtomicBool,
    /// Set to abort a running countdown.
    pub countdown_cancel: AtomicBool,
    /// Captures whose editor should auto-redact on open (per-app rules).
    pub auto_redact: Mutex<HashSet<u64>>,
    next_id: AtomicU64,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: RwLock::new(settings),
            session: Mutex::new(None),
            captures: Mutex::new(HashMap::new()),
            last_region: Mutex::new(None),
            temp_files: Mutex::new(Vec::new()),
            pending_steps: Mutex::new(Vec::new()),
            capture_pending: AtomicBool::new(false),
            countdown_cancel: AtomicBool::new(false),
            auto_redact: Mutex::new(HashSet::new()),
            next_id: AtomicU64::new(1),
        }
    }

    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn insert_capture(
        &self,
        app: &AppHandle,
        image: RgbaImage,
        source: CaptureSource,
    ) -> Arc<Capture> {
        let capture = Arc::new(Capture {
            id: self.next_id(),
            image,
            source,
            created: chrono::Local::now(),
        });
        // keep memory bounded: drop the oldest captures that no editor or pin window still shows
        let in_use: Vec<u64> = app
            .webview_windows()
            .keys()
            .filter_map(|l| capture_id_from_label(l))
            .collect();
        let mut map = self.captures.lock().unwrap();
        if map.len() >= 24 {
            let mut ids: Vec<u64> = map
                .keys()
                .copied()
                .filter(|id| !in_use.contains(id))
                .collect();
            ids.sort_unstable();
            for id in ids.into_iter().take((map.len() + 1).saturating_sub(24)) {
                map.remove(&id);
            }
        }
        map.insert(capture.id, capture.clone());
        capture
    }

    pub fn capture(&self, id: u64) -> Option<Arc<Capture>> {
        self.captures.lock().unwrap().get(&id).cloned()
    }

    /// Swap in processed pixels (e.g. redacted) for a capture that windows already refer to.
    pub fn replace_capture(&self, capture: Arc<Capture>) {
        self.captures.lock().unwrap().insert(capture.id, capture);
    }

    pub fn remove_capture(&self, id: u64) {
        self.captures.lock().unwrap().remove(&id);
    }
}
