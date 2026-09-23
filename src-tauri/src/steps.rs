//! Step recorder: while recording, every click captures the clicked window with the click spot
//! marked and a title like "Click the “Save” button in Notepad". On stop the steps become a new
//! guide. Windows only for now (global mouse hook + UI Automation). No keystrokes are recorded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::RgbaImage;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::capture_rect_live;
use crate::error::{AppError, AppResult};
use crate::geom::Rect;
use crate::state::AppState;
use crate::step_marks::{manual_title, mark_click, step_title, window_label};

pub const STATE_EVENT: &str = "steps://state";

pub struct Step {
    pub image: RgbaImage,
    pub title: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecorderState {
    pub recording: bool,
    pub paused: bool,
    pub count: usize,
}

struct Session {
    steps: Arc<Mutex<Vec<Step>>>,
    paused: Arc<AtomicBool>,
    started: chrono::DateTime<chrono::Local>,
    handle: imp::Handle,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// A click reported by the hook, in global physical pixels.
#[derive(Clone, Copy, Debug)]
pub struct Click {
    pub x: i32,
    pub y: i32,
    pub right: bool,
}

/// The top-level window under a point.
pub struct WindowUnder {
    pub rect: Rect,
    pub title: String,
    pub pid: u32,
}

pub fn is_recording() -> bool {
    SESSION.lock().unwrap().is_some()
}

pub fn state() -> RecorderState {
    match SESSION.lock().unwrap().as_ref() {
        Some(s) => RecorderState {
            recording: true,
            paused: s.paused.load(Ordering::SeqCst),
            count: s.steps.lock().unwrap().len(),
        },
        None => RecorderState {
            recording: false,
            paused: false,
            count: 0,
        },
    }
}

fn announce(app: &AppHandle) {
    let _ = app.emit(STATE_EVENT, state());
}

/// Hotkey / tray entry point. Those handlers run on the main thread, and creating the control
/// bar from inside them deadlocks on Windows (the webview build waits on the very event loop
/// that's busy running the handler), freezing the whole app. So do the work elsewhere.
pub fn toggle(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        if is_recording() {
            stop(&app);
        } else if let Err(e) = start(&app) {
            crate::windows::toast(&app, "Step recorder", &e.to_string());
        }
    });
}

pub fn start(app: &AppHandle) -> AppResult<()> {
    if is_recording() {
        return Ok(());
    }
    let steps = Arc::new(Mutex::new(Vec::new()));
    let paused = Arc::new(AtomicBool::new(false));
    let (app2, steps2, paused2) = (app.clone(), steps.clone(), paused.clone());
    let mut last: Option<Instant> = None;
    let handle = imp::start(move |click| {
        if paused2.load(Ordering::SeqCst) {
            return;
        }
        // a double-click is one step
        if last.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) {
            return;
        }
        last = Some(Instant::now());
        match record_click(click) {
            Ok(Some(step)) => {
                steps2.lock().unwrap().push(step);
                announce(&app2);
            }
            Ok(None) => {}
            Err(e) => log::warn!("step capture failed: {e}"),
        }
    })?;
    *SESSION.lock().unwrap() = Some(Session {
        steps,
        paused,
        started: chrono::Local::now(),
        handle,
    });
    crate::bar::open(app, "steps", "");
    refresh_tray(app);
    announce(app);
    Ok(())
}

/// Stop recording and hand the steps to Guides as a new guide. Returns the step count.
pub fn stop(app: &AppHandle) -> usize {
    let Some(session) = SESSION.lock().unwrap().take() else {
        return 0;
    };
    imp::stop(session.handle);
    crate::bar::close(app);
    let steps = std::mem::take(&mut *session.steps.lock().unwrap());
    let count = steps.len();
    if count > 0 {
        let project = format!("Steps — {}", session.started.format("%Y-%m-%d %H:%M"));
        crate::commands::guide::queue_steps(
            app,
            steps.into_iter().map(|s| (s.image, s.title)).collect(),
            Some(project),
        );
    } else {
        crate::windows::toast(app, "Step recorder", "No steps were recorded.");
    }
    refresh_tray(app);
    announce(app);
    count
}

pub fn set_paused(app: &AppHandle, paused: bool) {
    if let Some(s) = SESSION.lock().unwrap().as_ref() {
        s.paused.store(paused, Ordering::SeqCst);
    }
    announce(app);
}

/// "Add step now": the window under the pointer, without a click marker.
pub fn add_now(app: &AppHandle) -> AppResult<()> {
    let steps = SESSION
        .lock()
        .unwrap()
        .as_ref()
        .map(|s| s.steps.clone())
        .ok_or_else(|| AppError::Other("not recording".into()))?;
    let pos = app.cursor_position()?;
    let (x, y) = (pos.x as i32, pos.y as i32);
    let win = imp::window_under(x, y).filter(|w| w.pid != std::process::id());
    let (rect, title) = match win {
        Some(w) => (w.rect, manual_title(&window_label(&w.title))),
        None => (monitor_rect_at(x, y)?, manual_title("")),
    };
    let image = capture_rect_live(rect)?;
    steps.lock().unwrap().push(Step { image, title });
    announce(app);
    Ok(())
}

fn refresh_tray(app: &AppHandle) {
    let settings = app.state::<AppState>().settings();
    crate::tray::refresh(app, &settings);
}

fn monitor_rect_at(x: i32, y: i32) -> AppResult<Rect> {
    let frames = crate::capture::capture_all_monitors()?;
    frames
        .iter()
        .map(|f| f.monitor.rect())
        .find(|r| r.contains(x, y))
        .ok_or_else(|| AppError::Capture("no monitor under the click".into()))
}

/// Capture the clicked window with the click marked, and title the step.
/// `None` for clicks on QuickShot itself (e.g. the recorder bar).
fn record_click(click: Click) -> AppResult<Option<Step>> {
    let win = imp::window_under(click.x, click.y);
    if win.as_ref().is_some_and(|w| w.pid == std::process::id()) {
        return Ok(None);
    }
    let (rect, label) = match &win {
        Some(w) if !w.rect.is_empty() => (w.rect, window_label(&w.title)),
        _ => (monitor_rect_at(click.x, click.y)?, String::new()),
    };
    // what was clicked, asked before the app has had much time to react
    let (name, control) = crate::uia::element_label(click.x, click.y).unwrap_or_default();
    let mut image = capture_rect_live(rect)?;
    mark_click(&mut image, click.x - rect.x, click.y - rect.y);
    Ok(Some(Step {
        image,
        title: step_title(click.right, &name, &control, &label),
    }))
}

#[cfg(not(windows))]
mod imp {
    use super::{Click, WindowUnder};
    use crate::error::{AppError, AppResult};

    pub struct Handle;

    pub fn start(_on_click: impl FnMut(Click) + Send + 'static) -> AppResult<Handle> {
        Err(AppError::Other(
            "The step recorder is Windows-only for now.".into(),
        ))
    }

    pub fn stop(_handle: Handle) {}

    pub fn window_under(_x: i32, _y: i32) -> Option<WindowUnder> {
        None
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetAncestor, GetMessageW, GetWindowTextW, GetWindowThreadProcessId,
        PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, WindowFromPoint, GA_ROOT, MSG,
        MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_QUIT, WM_RBUTTONDOWN,
    };

    use super::{Click, WindowUnder};
    use crate::error::{AppError, AppResult};
    use crate::geom::Rect;

    pub struct Handle {
        hook_thread: u32,
    }

    /// Where the hook sends clicks. The hook itself must return fast, so it only forwards.
    fn clicks() -> &'static Mutex<Option<Sender<Click>>> {
        static CLICKS: OnceLock<Mutex<Option<Sender<Click>>>> = OnceLock::new();
        CLICKS.get_or_init(|| Mutex::new(None))
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let msg = wparam.0 as u32;
        if code >= 0 && (msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN) {
            // SAFETY: for WH_MOUSE_LL, lparam points to an MSLLHOOKSTRUCT for this call.
            let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
            if let Some(tx) = clicks().lock().ok().and_then(|g| g.clone()) {
                let _ = tx.send(Click {
                    x: info.pt.x,
                    y: info.pt.y,
                    right: msg == WM_RBUTTONDOWN,
                });
            }
        }
        // SAFETY: passing the event on is required for every hook.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    pub fn start(mut on_click: impl FnMut(Click) + Send + 'static) -> AppResult<Handle> {
        let (tx, rx) = channel::<Click>();
        *clicks().lock().unwrap() = Some(tx);
        std::thread::Builder::new()
            .name("step-recorder".into())
            .spawn(move || {
                // ends when the sender is dropped in stop()
                for click in rx {
                    on_click(click);
                }
            })
            .map_err(|e| AppError::Other(e.to_string()))?;
        let (ready_tx, ready_rx) = channel::<Result<u32, String>>();
        std::thread::Builder::new()
            .name("mouse-hook".into())
            .spawn(move || {
                // SAFETY: a low-level hook needs a message loop on the thread that installed it;
                // this thread does nothing else and removes the hook before exiting.
                unsafe {
                    let module = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
                    match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0) {
                        Ok(hook) => {
                            let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                            let mut msg = MSG::default();
                            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}
                            let _ = UnhookWindowsHookEx(hook);
                        }
                        Err(e) => {
                            let _ = ready_tx.send(Err(e.to_string()));
                        }
                    }
                }
            })
            .map_err(|e| AppError::Other(e.to_string()))?;
        let hook_thread = ready_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| AppError::Other("mouse hook did not start".into()))?
            .map_err(|e| AppError::Other(format!("mouse hook failed: {e}")))?;
        Ok(Handle { hook_thread })
    }

    pub fn stop(handle: Handle) {
        // SAFETY: posting WM_QUIT to our own hook thread ends its message loop.
        unsafe {
            let _ = PostThreadMessageW(handle.hook_thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        *clicks().lock().unwrap() = None;
    }

    pub fn window_under(x: i32, y: i32) -> Option<WindowUnder> {
        // SAFETY: read-only window queries.
        unsafe {
            let hwnd = WindowFromPoint(POINT { x, y });
            if hwnd.0.is_null() {
                return None;
            }
            let root: HWND = GetAncestor(hwnd, GA_ROOT);
            let root = if root.0.is_null() { hwnd } else { root };
            let mut pid = 0u32;
            GetWindowThreadProcessId(root, Some(&mut pid as *mut u32));
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(root, &mut buf).max(0) as usize;
            let mut r = RECT::default();
            let rect = DwmGetWindowAttribute(
                root,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut r as *mut RECT as *mut core::ffi::c_void,
                std::mem::size_of::<RECT>() as u32,
            )
            .ok()
            .map(|_| {
                Rect::new(
                    r.left,
                    r.top,
                    (r.right - r.left).max(0) as u32,
                    (r.bottom - r.top).max(0) as u32,
                )
            })
            .unwrap_or_default();
            Some(WindowUnder {
                rect,
                title: String::from_utf16_lossy(&buf[..len]),
                pid,
            })
        }
    }
}
