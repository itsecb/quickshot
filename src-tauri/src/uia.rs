//! UI element detection for the capture overlay (Snagit-style): the nested parts of a window
//! under the pointer (panes, toolbars, dialogs, buttons…) via Windows UI Automation.
//!
//! We never hit-test the screen (that would find our own overlay): starting from the window
//! the user is hovering, we walk *down* its element tree, always into the smallest visible
//! child that contains the point. The result is the chain window → … → innermost element, in
//! global physical pixels; the overlay lets the user step through it with the mouse wheel.

use crate::geom::Rect;

/// Chain of nested rects containing (`x`, `y`), outermost (the window) first.
/// Empty when detection is unavailable (other platforms, or the app exposes nothing).
pub fn element_chain(window_id: u32, x: i32, y: i32) -> Vec<Rect> {
    imp::element_chain(window_id, x, y)
}

/// Name and (localized) control type of the element at a screen point, e.g. ("Save", "button").
/// Used by the step recorder while nothing of ours covers the screen.
pub fn element_label(x: i32, y: i32) -> Option<(String, String)> {
    imp::element_label(x, y)
}

#[cfg(not(windows))]
mod imp {
    use crate::geom::Rect;

    pub fn element_chain(_window_id: u32, _x: i32, _y: i32) -> Vec<Rect> {
        Vec::new()
    }

    pub fn element_label(_x: i32, _y: i32) -> Option<(String, String)> {
        None
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::mpsc::{channel, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationCacheRequest, IUIAutomationCondition,
        IUIAutomationElement, TreeScope_Children, UIA_BoundingRectanglePropertyId,
        UIA_IsOffscreenPropertyId,
    };

    use crate::geom::Rect;

    /// Deep trees (browsers) can be slow to walk; stop and return what we have.
    const BUDGET: Duration = Duration::from_millis(150);
    const MAX_DEPTH: usize = 40;

    enum Job {
        Chain {
            window_id: u32,
            x: i32,
            y: i32,
            reply: Sender<Vec<Rect>>,
        },
        Label {
            x: i32,
            y: i32,
            reply: Sender<Option<(String, String)>>,
        },
    }

    /// UI Automation objects live on one COM thread; callers talk to it through a channel.
    fn worker() -> &'static Mutex<Sender<Job>> {
        static WORKER: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();
        WORKER.get_or_init(|| {
            let (tx, rx) = channel::<Job>();
            std::thread::Builder::new()
                .name("ui-automation".into())
                .spawn(move || {
                    let probe = Probe::new();
                    if let Err(e) = &probe {
                        log::warn!("UI Automation unavailable: {e}");
                    }
                    for job in rx {
                        match job {
                            Job::Chain {
                                window_id,
                                x,
                                y,
                                reply,
                            } => {
                                let chain = match &probe {
                                    Ok(p) => p.chain(window_id, x, y).unwrap_or_else(|e| {
                                        log::debug!("element chain failed: {e}");
                                        Vec::new()
                                    }),
                                    Err(_) => Vec::new(),
                                };
                                let _ = reply.send(chain);
                            }
                            Job::Label { x, y, reply } => {
                                let label = probe.as_ref().ok().and_then(|p| p.label(x, y).ok());
                                let _ = reply.send(label);
                            }
                        }
                    }
                })
                .expect("spawn UI Automation thread");
            Mutex::new(tx)
        })
    }

    pub fn element_label(x: i32, y: i32) -> Option<(String, String)> {
        let (reply, answer) = channel();
        worker()
            .lock()
            .unwrap()
            .send(Job::Label { x, y, reply })
            .ok()?;
        answer.recv_timeout(Duration::from_secs(2)).ok().flatten()
    }

    pub fn element_chain(window_id: u32, x: i32, y: i32) -> Vec<Rect> {
        let (reply, answer) = channel();
        let sent = worker().lock().unwrap().send(Job::Chain {
            window_id,
            x,
            y,
            reply,
        });
        if sent.is_err() {
            return Vec::new();
        }
        answer
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_default()
    }

    struct Probe {
        automation: IUIAutomation,
        cache: IUIAutomationCacheRequest,
        condition: IUIAutomationCondition,
    }

    fn rect(r: RECT) -> Option<Rect> {
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        (w > 0 && h > 0).then(|| Rect::new(r.left, r.top, w as u32, h as u32))
    }

    fn area(r: &Rect) -> u64 {
        r.width as u64 * r.height as u64
    }

    impl Probe {
        fn new() -> windows::core::Result<Self> {
            // SAFETY: COM initialisation and object creation on this dedicated thread; every
            // interface created here is only used from it.
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
                let automation: IUIAutomation =
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
                // fetch rect + off-screen flag for all children in one cross-process call
                let cache = automation.CreateCacheRequest()?;
                cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
                cache.AddProperty(UIA_IsOffscreenPropertyId)?;
                let condition = automation.ControlViewCondition()?;
                Ok(Self {
                    automation,
                    cache,
                    condition,
                })
            }
        }

        fn label(&self, x: i32, y: i32) -> windows::core::Result<(String, String)> {
            // SAFETY: UI Automation calls on the thread that created these interfaces.
            unsafe {
                let el = self.automation.ElementFromPoint(POINT { x, y })?;
                let name = el.CurrentName().map(|b| b.to_string()).unwrap_or_default();
                let control = el
                    .CurrentLocalizedControlType()
                    .map(|b| b.to_string())
                    .unwrap_or_default();
                Ok((name, control))
            }
        }

        fn chain(&self, window_id: u32, x: i32, y: i32) -> windows::core::Result<Vec<Rect>> {
            // xcap ids are HWNDs truncated to 32 bits; HWNDs are sign-extended 32-bit values.
            let hwnd = HWND(window_id as i32 as isize as *mut core::ffi::c_void);
            let started = Instant::now();
            // SAFETY: UI Automation calls on the thread that created these interfaces.
            unsafe {
                let root = self.automation.ElementFromHandle(hwnd)?;
                let Some(window) = rect(root.CurrentBoundingRectangle()?) else {
                    return Ok(Vec::new());
                };
                let mut out = vec![window];
                let mut current: IUIAutomationElement = root;
                for _ in 0..MAX_DEPTH {
                    if started.elapsed() > BUDGET {
                        break;
                    }
                    let children = current.FindAllBuildCache(
                        TreeScope_Children,
                        &self.condition,
                        &self.cache,
                    )?;
                    let mut best: Option<(IUIAutomationElement, Rect)> = None;
                    for i in 0..children.Length()? {
                        let child = children.GetElement(i)?;
                        if child
                            .CachedIsOffscreen()
                            .map(|b| b.as_bool())
                            .unwrap_or(false)
                        {
                            continue;
                        }
                        let Some(r) = child.CachedBoundingRectangle().ok().and_then(rect) else {
                            continue;
                        };
                        if !r.contains(x, y) {
                            continue;
                        }
                        // overlapping siblings: the smallest one is the most specific
                        if best.as_ref().is_none_or(|(_, b)| area(&r) < area(b)) {
                            best = Some((child, r));
                        }
                    }
                    let Some((child, r)) = best else { break };
                    // children can report rects spilling outside a scrolled parent
                    let parent = *out.last().expect("chain starts with the window");
                    let clipped = r.intersect(&parent).unwrap_or(r);
                    if clipped != parent {
                        out.push(clipped);
                    }
                    current = child;
                }
                Ok(out)
            }
        }
    }
}
