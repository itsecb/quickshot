//! Scrolling capture: capture the selected area, scroll it with the mouse wheel, capture again,
//! and stitch the frames (see `stitch.rs`) until the page stops moving, the user stops it, or
//! it gets too tall. The result goes through the normal after-capture flow.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::capture::capture_rect_live;
use crate::error::AppResult;
use crate::geom::Rect;
use crate::state::{AppState, CaptureMode, CaptureSource};
use crate::stitch::Stitcher;

const MAX_HEIGHT: u32 = 20_000;
const MAX_STEPS: usize = 120;
/// Time for smooth-scrolling animations to finish before the next frame.
const SETTLE: Duration = Duration::from_millis(380);

static RUNNING: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);

pub fn is_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

/// Ask a running scrolling capture to finish with what it has.
pub fn stop() {
    STOP.store(true, Ordering::SeqCst);
}

pub fn start(app: &AppHandle, rect: Rect, source: CaptureSource) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    STOP.store(false, Ordering::SeqCst);
    let app = app.clone();
    std::thread::spawn(move || {
        // Esc stops it (only while it runs; released right after)
        let esc = app.global_shortcut().on_shortcut("Escape", |_, _, e| {
            if e.state() == ShortcutState::Pressed {
                stop();
            }
        });
        crate::bar::open(&app, "scroll", "Scrolling capture…");
        let result = run(&app, rect, source);
        crate::bar::close(&app);
        if esc.is_ok() {
            let _ = app.global_shortcut().unregister("Escape");
        }
        RUNNING.store(false, Ordering::SeqCst);
        if let Err(e) = result {
            log::error!("scrolling capture failed: {e}");
            crate::windows::toast(&app, "Scrolling capture failed", &e.to_string());
        }
    });
}

fn run(app: &AppHandle, rect: Rect, mut source: CaptureSource) -> AppResult<()> {
    // let the overlay disappear before the first frame
    std::thread::sleep(Duration::from_millis(250));
    let (cx, cy) = (
        rect.x + rect.width as i32 / 2,
        rect.y + rect.height as i32 / 2,
    );
    // bigger areas scroll further per step; the overlap between frames stays generous
    let notches = (rect.height / 320).clamp(1, 5);
    let mut stitcher = Stitcher::new(capture_rect_live(rect)?);
    for _ in 0..MAX_STEPS {
        if STOP.load(Ordering::SeqCst) || stitcher.height() >= MAX_HEIGHT {
            break;
        }
        if !imp::scroll_down(cx, cy, notches) {
            break; // not supported here
        }
        std::thread::sleep(SETTLE);
        if STOP.load(Ordering::SeqCst) {
            break;
        }
        if !stitcher.push(capture_rect_live(rect)?) {
            break; // reached the end
        }
        crate::bar::update(
            app,
            &format!(
                "Scrolling capture · {} px · Esc or Done to finish",
                stitcher.height()
            ),
        );
    }
    log::info!(
        "scrolling capture: {} frames, {} px",
        stitcher.frame_count(),
        stitcher.height()
    );
    let image = stitcher.finish();
    source.kind = "scroll".into();
    source.rect = Rect::new(rect.x, rect.y, image.width(), image.height());
    let capture = app.state::<AppState>().insert_capture(app, image, source);
    crate::output::after_capture(app, capture, CaptureMode::Region)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SetCursorPos, WHEEL_DELTA};

    /// Wheel `notches` down over (x, y); the wheel scrolls whatever is under the pointer.
    pub fn scroll_down(x: i32, y: i32, notches: u32) -> bool {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: (-(WHEEL_DELTA as i32) * notches as i32) as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        // SAFETY: plain input synthesis with a correctly sized INPUT array.
        unsafe {
            let _ = SetCursorPos(x, y);
            SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 1
        }
    }
}

#[cfg(not(windows))]
mod imp {
    /// Not implemented yet on this platform: capture stops after the first frame.
    pub fn scroll_down(_x: i32, _y: i32, _notches: u32) -> bool {
        false
    }
}
