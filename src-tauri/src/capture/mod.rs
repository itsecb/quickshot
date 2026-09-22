pub mod monitors;

use std::collections::HashSet;

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Manager};

pub use monitors::MonitorInfo;

use crate::error::{AppError, AppResult};
use crate::geom::Rect;
use crate::image_util::blit;
use crate::state::{AppState, CaptureMode, CaptureSession, CaptureSource};
use crate::{overlay, protocol};

/// Frozen screenshot of one monitor.
pub struct CaptureFrame {
    pub monitor: MonitorInfo,
    pub image: RgbaImage,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app_name: String,
    /// Global physical pixels.
    pub rect: Rect,
    /// Higher = closer to the top of the stack.
    pub z: i32,
}

const OWN_APP_NAMES: &[&str] = &["QuickShot", "quickshot"];

/// Capture every monitor. Runs on the calling thread (~30-80 ms per monitor).
pub fn capture_all_monitors() -> AppResult<Vec<CaptureFrame>> {
    let monitors = xcap::Monitor::all()?;
    if monitors.is_empty() {
        return Err(AppError::Capture("no monitors found".into()));
    }
    let mut frames = Vec::with_capacity(monitors.len());
    for m in monitors {
        let id = m.id()?;
        let image = match m.capture_image() {
            Ok(img) => img,
            Err(e) => {
                log::warn!("monitor {id} capture failed: {e}");
                continue;
            }
        };
        let raw = monitors::RawGeometry {
            x: m.x()?,
            y: m.y()?,
            width: m.width()?,
            height: m.height()?,
            scale: m.scale_factor().map(|s| s as f64).unwrap_or(1.0),
        };
        let rect = monitors::to_physical(raw, image.dimensions(), monitors::COORDS_ARE_LOGICAL);
        let name = m
            .friendly_name()
            .or_else(|_| m.name())
            .unwrap_or_else(|_| format!("Display {id}"));
        frames.push(CaptureFrame {
            monitor: MonitorInfo {
                id,
                name,
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                scale: if raw.scale > 0.0 { raw.scale } else { 1.0 },
                is_primary: m.is_primary().unwrap_or(false),
                frame_url: protocol::monitor_url(id),
            },
            image,
        });
    }
    if frames.is_empty() {
        return Err(AppError::Capture(
            "could not capture any monitor (on macOS, grant Screen Recording permission)".into(),
        ));
    }
    Ok(frames)
}

/// Visible top-level windows in physical pixels, topmost first.
pub fn list_windows(frames: &[CaptureFrame]) -> Vec<WindowInfo> {
    let windows = match xcap::Window::all() {
        Ok(w) => w,
        Err(e) => {
            log::warn!("window enumeration failed: {e}");
            return Vec::new();
        }
    };
    let screen = Rect::union_all(frames.iter().map(|f| f.monitor.rect())).unwrap_or_default();
    let mut out = Vec::new();
    for w in windows {
        if w.is_minimized().unwrap_or(false) {
            continue;
        }
        let (Ok(x), Ok(y), Ok(width), Ok(height)) = (w.x(), w.y(), w.width(), w.height()) else {
            continue;
        };
        if width < 20 || height < 20 {
            continue;
        }
        let app_name = w.app_name().unwrap_or_default();
        let title = w.title().unwrap_or_default();
        if OWN_APP_NAMES
            .iter()
            .any(|n| app_name == *n || title.starts_with(n))
        {
            continue;
        }
        let mut rect = Rect::new(x, y, width, height);
        if monitors::COORDS_ARE_LOGICAL {
            let scale = w
                .current_monitor()
                .and_then(|m| m.scale_factor())
                .map(|s| s as f64)
                .unwrap_or(1.0);
            let raw = monitors::RawGeometry {
                x,
                y,
                width,
                height,
                scale,
            };
            let phys_size = (
                (width as f64 * scale).round() as u32,
                (height as f64 * scale).round() as u32,
            );
            rect = monitors::to_physical(raw, phys_size, true);
        }
        // ignore windows fully off-screen
        if rect.intersect(&screen).is_none() {
            continue;
        }
        out.push(WindowInfo {
            id: w.id().unwrap_or(0),
            title,
            app_name,
            rect,
            z: w.z().unwrap_or(0),
        });
    }
    out.sort_by(|a, b| b.z.cmp(&a.z));
    out
}

/// Compose an arbitrary global rect from the frozen monitor frames (may span monitors).
pub fn compose_region(frames: &[CaptureFrame], rect: Rect) -> AppResult<RgbaImage> {
    if rect.is_empty() {
        return Err(AppError::Capture("empty region".into()));
    }
    let mut out = RgbaImage::from_pixel(rect.width, rect.height, image::Rgba([0, 0, 0, 255]));
    let mut any = false;
    for frame in frames {
        let mrect = frame.monitor.rect();
        if let Some(overlap) = rect.intersect(&mrect) {
            any = true;
            // crop from the frame in frame-local coordinates, then blit at region-local coordinates
            let sub = image::imageops::crop_imm(
                &frame.image,
                (overlap.x - mrect.x) as u32,
                (overlap.y - mrect.y) as u32,
                overlap.width,
                overlap.height,
            )
            .to_image();
            blit(
                &mut out,
                &sub,
                (overlap.x - rect.x) as i64,
                (overlap.y - rect.y) as i64,
            );
        }
    }
    if !any {
        return Err(AppError::Capture("region is outside every monitor".into()));
    }
    Ok(out)
}

/// Monitor containing the OS cursor, else the primary, else the first.
pub fn monitor_under_cursor<'a>(app: &AppHandle, frames: &'a [CaptureFrame]) -> &'a CaptureFrame {
    if let Ok(pos) = app.cursor_position() {
        let (cx, cy) = (pos.x.round() as i32, pos.y.round() as i32);
        if let Some(f) = frames.iter().find(|f| f.monitor.rect().contains(cx, cy)) {
            return f;
        }
    }
    frames
        .iter()
        .find(|f| f.monitor.is_primary)
        .unwrap_or(&frames[0])
}

/// Entry point for hotkeys, tray and CLI. Never blocks the caller.
pub fn trigger(app: &AppHandle, mode: CaptureMode) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = begin(&app, mode) {
            log::error!("capture ({mode:?}) failed: {e}");
            crate::windows::toast(&app, "Capture failed", &e.to_string());
        }
    });
}

fn begin(app: &AppHandle, mode: CaptureMode) -> AppResult<()> {
    let state = app.state::<AppState>();
    if state.session.lock().unwrap().is_some() {
        log::info!("capture already in progress; ignoring {mode:?}");
        return Ok(());
    }
    let frames = capture_all_monitors()?;

    match mode {
        CaptureMode::Fullscreen => {
            let frame = monitor_under_cursor(app, &frames);
            let source = CaptureSource {
                kind: "monitor".into(),
                monitor_name: frame.monitor.name.clone(),
                rect: frame.monitor.rect(),
                ..Default::default()
            };
            let capture = state.insert_capture(app, frame.image.clone(), source);
            return crate::output::after_capture(app, capture, CaptureMode::Region);
        }
        CaptureMode::RepeatLast => {
            let last = *state.last_region.lock().unwrap();
            let Some(rect) = last else {
                // nothing to repeat yet: fall back to a fresh region selection
                return begin_overlay(app, CaptureMode::Region, frames);
            };
            let image = compose_region(&frames, rect)?;
            let source = CaptureSource {
                kind: "region".into(),
                rect,
                ..Default::default()
            };
            let capture = state.insert_capture(app, image, source);
            return crate::output::after_capture(app, capture, CaptureMode::Region);
        }
        _ => begin_overlay(app, mode, frames),
    }
}

fn begin_overlay(app: &AppHandle, mode: CaptureMode, frames: Vec<CaptureFrame>) -> AppResult<()> {
    let state = app.state::<AppState>();
    let windows = list_windows(&frames);
    let monitors: Vec<MonitorInfo> = frames.iter().map(|f| f.monitor.clone()).collect();
    let labels: Vec<String> = monitors.iter().map(|m| overlay::label_for(m.id)).collect();
    let session = CaptureSession {
        mode,
        frames,
        windows,
        labels: labels.clone(),
        ready: HashSet::new(),
        shown: false,
    };
    *state.session.lock().unwrap() = Some(session);

    // The session lock must NOT be held while windows are created: on Windows the
    // webview build pumps messages, and the first overlay's IPC needs the same lock.
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = overlay::open(&app2, &monitors) {
            log::error!("failed to open overlays: {e}");
            let state = app2.state::<AppState>();
            state.session.lock().unwrap().take();
            overlay::close_labels(&app2, &labels);
        }
    })?;
    Ok(())
}

/// Called by an overlay once it painted its frame. Shows all overlays when every one is ready.
pub fn overlay_ready(app: &AppHandle, label: &str) -> AppResult<()> {
    let state = app.state::<AppState>();
    let mut guard = state.session.lock().unwrap();
    let Some(session) = guard.as_mut() else {
        return Ok(());
    };
    session.ready.insert(label.to_string());
    if !session.shown && session.labels.iter().all(|l| session.ready.contains(l)) {
        session.shown = true;
        let labels = session.labels.clone();
        let focus = focus_label(app, session);
        drop(guard);
        overlay::show_all(app, &labels, focus.as_deref());
    }
    Ok(())
}

fn focus_label(app: &AppHandle, session: &CaptureSession) -> Option<String> {
    let frame = monitor_under_cursor(app, &session.frames);
    Some(overlay::label_for(frame.monitor.id))
}

/// Finish a selection: build the capture and route it according to the session mode.
pub fn finish(app: &AppHandle, rect: Rect, window_id: Option<u32>) -> AppResult<u64> {
    let state = app.state::<AppState>();
    let session = state
        .session
        .lock()
        .unwrap()
        .take()
        .ok_or_else(|| AppError::Other("no capture in progress".into()))?;
    overlay::close_labels(app, &session.labels);

    let image = compose_region(&session.frames, rect)?;
    let mut source = CaptureSource {
        kind: "region".into(),
        rect,
        ..Default::default()
    };
    if let Some(wid) = window_id {
        if let Some(w) = session.windows.iter().find(|w| w.id == wid) {
            source.kind = "window".into();
            source.app_name = w.app_name.clone();
            source.title = w.title.clone();
        }
    }
    if let Some(frame) = session
        .frames
        .iter()
        .find(|f| f.monitor.rect().intersect(&rect).is_some())
    {
        source.monitor_name = frame.monitor.name.clone();
    }
    if matches!(session.mode, CaptureMode::Region | CaptureMode::Window) {
        *state.last_region.lock().unwrap() = Some(rect);
    }
    let capture = state.insert_capture(app, image, source);
    let id = capture.id;
    crate::output::after_capture(app, capture, session.mode)?;
    Ok(id)
}

pub fn cancel(app: &AppHandle) {
    let state = app.state::<AppState>();
    let session = state.session.lock().unwrap().take();
    if let Some(session) = session {
        overlay::close_labels(app, &session.labels);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(id: u32, x: i32, y: i32, w: u32, h: u32, color: [u8; 4]) -> CaptureFrame {
        CaptureFrame {
            monitor: MonitorInfo {
                id,
                name: format!("m{id}"),
                x,
                y,
                width: w,
                height: h,
                scale: 1.0,
                is_primary: id == 1,
                frame_url: String::new(),
            },
            image: RgbaImage::from_pixel(w, h, image::Rgba(color)),
        }
    }

    #[test]
    fn compose_within_single_monitor() {
        let frames = vec![frame(1, 0, 0, 100, 100, [10, 0, 0, 255])];
        let img = compose_region(&frames, Rect::new(10, 10, 20, 30)).unwrap();
        assert_eq!(img.dimensions(), (20, 30));
        assert_eq!(img.get_pixel(0, 0).0, [10, 0, 0, 255]);
    }

    #[test]
    fn compose_spanning_two_monitors_with_negative_origin() {
        let frames = vec![
            frame(2, -100, 0, 100, 100, [0, 20, 0, 255]),
            frame(1, 0, 0, 100, 100, [0, 0, 30, 255]),
        ];
        let img = compose_region(&frames, Rect::new(-10, 0, 20, 10)).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [0, 20, 0, 255]);
        assert_eq!(img.get_pixel(9, 9).0, [0, 20, 0, 255]);
        assert_eq!(img.get_pixel(10, 0).0, [0, 0, 30, 255]);
        assert_eq!(img.get_pixel(19, 9).0, [0, 0, 30, 255]);
    }

    #[test]
    fn compose_outside_is_error() {
        let frames = vec![frame(1, 0, 0, 100, 100, [1, 1, 1, 255])];
        assert!(compose_region(&frames, Rect::new(500, 500, 10, 10)).is_err());
        assert!(compose_region(&frames, Rect::new(0, 0, 0, 10)).is_err());
    }

    #[test]
    fn compose_partially_outside_pads_black() {
        let frames = vec![frame(1, 0, 0, 100, 100, [9, 9, 9, 255])];
        let img = compose_region(&frames, Rect::new(90, 90, 20, 20)).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(img.get_pixel(19, 19).0, [0, 0, 0, 255]);
    }
}
