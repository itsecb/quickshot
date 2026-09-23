//! GIF screen recording of a region: xcap's monitor recorder delivers frames (only when the
//! screen changes); a sampler turns that into a steady 12 fps, merges identical frames into
//! longer ones, and an encoder thread writes the GIF. Saved to the screenshots folder and shown
//! in the floating thumbnail (copy as a file, drag, show in folder).

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, AppResult};
use crate::geom::Rect;
use crate::state::AppState;
use crate::{output, settings};

const FPS: u64 = 12;
const MAX_SECONDS: u64 = 60;
const MAX_WIDTH: u32 = 960;
/// Fastest NeuQuant setting: fine for UI recordings, keeps encoding real-time.
const GIF_SPEED: i32 = 30;

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

pub fn is_recording() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
}

/// Record `rect` (global physical px) after a short countdown. `name` names the file.
pub fn start(app: &AppHandle, rect: Rect, name: String) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    STOP.store(false, Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        let result = crate::capture::countdown(&app, 3).and_then(|go| {
            if go {
                record(&app, rect, &name).map(Some)
            } else {
                Ok(None)
            }
        });
        crate::bar::close(&app);
        RUNNING.store(false, Ordering::SeqCst);
        match result {
            Ok(Some((path, secs))) => announce(&app, &path, secs),
            Ok(None) => {}
            Err(e) => {
                log::error!("GIF recording failed: {e}");
                crate::windows::toast(&app, "Recording failed", &e.to_string());
            }
        }
    });
}

/// Monitor containing the centre of `rect`, with its physical rect.
fn monitor_for(rect: Rect) -> AppResult<(xcap::Monitor, Rect)> {
    let (cx, cy) = (
        rect.x + rect.width as i32 / 2,
        rect.y + rect.height as i32 / 2,
    );
    for m in xcap::Monitor::all()? {
        let scale = m.scale_factor().map(|s| s as f64).unwrap_or(1.0).max(0.1);
        let (x, y, w, h) = (m.x()?, m.y()?, m.width()?, m.height()?);
        let r = if crate::capture::monitors::COORDS_ARE_LOGICAL {
            Rect::new(
                (x as f64 * scale) as i32,
                (y as f64 * scale) as i32,
                (w as f64 * scale) as u32,
                (h as f64 * scale) as u32,
            )
        } else {
            Rect::new(x, y, w, h)
        };
        if r.contains(cx, cy) {
            return Ok((m, r));
        }
    }
    Err(AppError::Capture("the selection isn't on a monitor".into()))
}

/// Crop the region out of a monitor frame and shrink it to at most `MAX_WIDTH`.
fn crop_frame(frame: &xcap::Frame, mon: Rect, region: Rect) -> Option<RgbaImage> {
    let full = RgbaImage::from_raw(frame.width, frame.height, frame.raw.clone())?;
    // the frame may be at a different scale than our physical monitor rect
    let fx = frame.width as f64 / mon.width.max(1) as f64;
    let fy = frame.height as f64 / mon.height.max(1) as f64;
    let x = (((region.x - mon.x) as f64) * fx).max(0.0) as u32;
    let y = (((region.y - mon.y) as f64) * fy).max(0.0) as u32;
    let w = ((region.width as f64 * fx) as u32).min(frame.width.saturating_sub(x));
    let h = ((region.height as f64 * fy) as u32).min(frame.height.saturating_sub(y));
    if w == 0 || h == 0 {
        return None;
    }
    let cropped = image::imageops::crop_imm(&full, x, y, w, h).to_image();
    if w <= MAX_WIDTH {
        return Some(cropped);
    }
    let nh = ((h as f64 * MAX_WIDTH as f64 / w as f64).round() as u32).max(1);
    Some(image::imageops::resize(
        &cropped,
        MAX_WIDTH,
        nh,
        image::imageops::FilterType::Triangle,
    ))
}

fn record(app: &AppHandle, rect: Rect, name: &str) -> AppResult<(PathBuf, u64)> {
    let (monitor, mon) = monitor_for(rect)?;
    let region = rect
        .intersect(&mon)
        .ok_or_else(|| AppError::Capture("the selection isn't on a monitor".into()))?;
    let (recorder, frames) = monitor.video_recorder()?;
    recorder.start()?;

    // keep only the newest frame; the recorder only sends when something changes
    let latest: Arc<Mutex<Option<xcap::Frame>>> = Arc::new(Mutex::new(None));
    let receiving = Arc::new(AtomicBool::new(true));
    let consumer = {
        let (latest, receiving) = (latest.clone(), receiving.clone());
        std::thread::spawn(move || receive(frames, latest, receiving))
    };

    let settings = app.state::<AppState>().settings();
    let dir = settings::save_dir(app, &settings)?;
    let stem = settings::expand_pattern(
        &settings.file_pattern,
        "",
        name,
        region.width,
        region.height,
    );
    let path = output::unique_path(&dir, &stem, "gif");

    let (tx, rx) = sync_channel::<(RgbaImage, u32)>(16);
    let encoder = {
        let path = path.clone();
        std::thread::spawn(move || -> AppResult<()> {
            let mut enc =
                GifEncoder::new_with_speed(BufWriter::new(File::create(&path)?), GIF_SPEED);
            enc.set_repeat(Repeat::Infinite)?;
            for (img, ms) in rx {
                enc.encode_frame(Frame::from_parts(
                    img,
                    0,
                    0,
                    Delay::from_numer_denom_ms(ms, 1),
                ))?;
            }
            Ok(())
        })
    };

    crate::bar::open(app, "gif", "Recording 0:00");
    let started = Instant::now();
    let tick = Duration::from_millis(1000 / FPS);
    let mut pending: Option<(RgbaImage, u32)> = None; // frame waiting for its duration
    let mut next_tick = Instant::now();
    let mut last_label = 0;
    while !STOP.load(Ordering::SeqCst) && started.elapsed() < Duration::from_secs(MAX_SECONDS) {
        next_tick += tick;
        std::thread::sleep(next_tick.saturating_duration_since(Instant::now()));
        let snapshot = latest
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|f| crop_frame(f, mon, region));
        let Some(img) = snapshot else { continue };
        let tick_ms = tick.as_millis() as u32;
        pending = match pending.take() {
            // unchanged screen: the previous frame just stays up longer
            Some((prev, ms)) if prev.as_raw() == img.as_raw() => Some((prev, ms + tick_ms)),
            Some((prev, ms)) => match tx.try_send((prev, ms)) {
                Ok(()) => Some((img, tick_ms)),
                // encoder behind: drop this frame rather than letting timing drift
                Err(TrySendError::Full((prev, ms))) => Some((prev, ms + tick_ms)),
                Err(TrySendError::Disconnected(_)) => break,
            },
            None => Some((img, tick_ms)),
        };
        let secs = started.elapsed().as_secs();
        if secs != last_label {
            last_label = secs;
            crate::bar::update(app, &format!("Recording {}:{:02}", secs / 60, secs % 60));
        }
    }
    let _ = recorder.stop();
    receiving.store(false, Ordering::SeqCst);
    if let Some(last) = pending {
        let _ = tx.send(last);
    }
    drop(tx);
    let _ = consumer.join();
    encoder
        .join()
        .map_err(|_| AppError::Other("GIF encoder crashed".into()))??;
    Ok((path, started.elapsed().as_secs()))
}

fn receive(
    frames: Receiver<xcap::Frame>,
    latest: Arc<Mutex<Option<xcap::Frame>>>,
    on: Arc<AtomicBool>,
) {
    while on.load(Ordering::SeqCst) {
        match frames.recv_timeout(Duration::from_millis(200)) {
            Ok(f) => *latest.lock().unwrap() = Some(f),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn announce(app: &AppHandle, path: &std::path::Path, secs: u64) {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let status = format!(
        "Saved GIF · {}:{:02} · {:.1} MB",
        secs / 60,
        secs % 60,
        size as f64 / 1_048_576.0
    );
    let app2 = app.clone();
    let path = path.to_path_buf();
    let _ = app.run_on_main_thread(move || {
        if let Err(e) = crate::windows::open_file_thumb(&app2, &path, &status) {
            log::error!("GIF thumbnail failed: {e}");
        }
    });
}
