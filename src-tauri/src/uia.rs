//! UI element detection for the capture overlay (Snagit-style): the nested parts of a window
//! under the pointer (panes, toolbars, dialogs, buttons…) via Windows UI Automation, or the
//! Accessibility API on macOS.
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

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use crate::geom::Rect;

    pub fn element_chain(_window_id: u32, _x: i32, _y: i32) -> Vec<Rect> {
        Vec::new()
    }

    pub fn element_label(_x: i32, _y: i32) -> Option<(String, String)> {
        None
    }
}

/// macOS: the Accessibility API. Hit-testing the *application* that owns the hovered window
/// (not the whole screen, where our overlay is on top) gives the innermost element under the
/// point; walking up its parents to the window gives the chain. Needs the Accessibility
/// permission, asked for once per run.
#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2_core_foundation::{CFBoolean, CFDictionary, CFString};

    use crate::capture::{quick_monitors, scale_at_physical};
    use crate::geom::Rect;

    type Ref = *const c_void;

    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: f64,
        y: f64,
    }

    const AX_VALUE_CGPOINT: u32 = 1;
    const AX_VALUE_CGSIZE: u32 = 2;
    const MAX_DEPTH: usize = 40;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: Ref) -> bool;
        fn AXUIElementCreateApplication(pid: i32) -> Ref;
        fn AXUIElementCopyElementAtPosition(app: Ref, x: f32, y: f32, element: *mut Ref) -> i32;
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> i32;
        fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> i32;
        fn AXValueGetValue(value: Ref, the_type: u32, out: *mut c_void) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: Ref);
    }

    /// A CoreFoundation object we own (released on drop).
    struct Owned(Ref);

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: we hold one reference from a Create/Copy call.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn attr(element: Ref, name: &str) -> Option<Owned> {
        let key = CFString::from_str(name);
        let mut value: Ref = std::ptr::null();
        // SAFETY: `element` is a live AXUIElement; on success we own `value`.
        let err = unsafe {
            AXUIElementCopyAttributeValue(element, &*key as *const CFString as Ref, &mut value)
        };
        (err == 0 && !value.is_null()).then_some(Owned(value))
    }

    fn role(element: Ref) -> String {
        attr(element, "AXRole")
            // SAFETY: AXRole is a CFString.
            .map(|v| unsafe { (*(v.0 as *const CFString)).to_string() })
            .unwrap_or_default()
    }

    /// Frame in logical points (global, top-left origin).
    fn frame(element: Ref) -> Option<(f64, f64, f64, f64)> {
        let (mut pos, mut size) = (Point::default(), Point::default());
        let p = attr(element, "AXPosition")?;
        let s = attr(element, "AXSize")?;
        // SAFETY: AXPosition / AXSize are AXValues holding a CGPoint / CGSize (two f64s).
        let ok = unsafe {
            AXValueGetValue(p.0, AX_VALUE_CGPOINT, &mut pos as *mut Point as *mut c_void)
                && AXValueGetValue(s.0, AX_VALUE_CGSIZE, &mut size as *mut Point as *mut c_void)
        };
        (ok && size.x > 0.0 && size.y > 0.0).then_some((pos.x, pos.y, size.x, size.y))
    }

    fn trusted() -> bool {
        static ASKED: AtomicBool = AtomicBool::new(false);
        // SAFETY: plain queries; the options dictionary lives across the call.
        unsafe {
            if AXIsProcessTrusted() {
                return true;
            }
            if !ASKED.swap(true, Ordering::SeqCst) {
                log::info!("Accessibility permission missing: asking (needed for window parts)");
                let key = CFString::from_static_str("AXTrustedCheckOptionPrompt");
                let options = CFDictionary::from_slices(&[&*key], &[CFBoolean::new(true)]);
                AXIsProcessTrustedWithOptions(&*options as *const _ as Ref);
            }
        }
        false
    }

    pub fn element_chain(window_id: u32, x: i32, y: i32) -> Vec<Rect> {
        if !trusted() {
            return Vec::new();
        }
        let Some(pid) = crate::capture::mac_window_pid(window_id) else {
            return Vec::new();
        };
        let monitors = quick_monitors();
        let scale = scale_at_physical(&monitors, x, y);
        let (lx, ly) = (x as f64 / scale, y as f64 / scale);
        // SAFETY: AX calls on objects we own for the duration of the walk.
        let mut rects = unsafe {
            let app = Owned(AXUIElementCreateApplication(pid));
            if app.0.is_null() {
                return Vec::new();
            }
            // an unresponsive app must not stall the overlay
            AXUIElementSetMessagingTimeout(app.0, 0.2);
            let mut hit: Ref = std::ptr::null();
            if AXUIElementCopyElementAtPosition(app.0, lx as f32, ly as f32, &mut hit) != 0
                || hit.is_null()
            {
                return Vec::new();
            }
            let mut current = Owned(hit);
            let mut rects = Vec::new();
            for _ in 0..MAX_DEPTH {
                let r = role(current.0);
                if r == "AXApplication" {
                    break;
                }
                if let Some(f) = frame(current.0) {
                    rects.push(f);
                }
                if r == "AXWindow" {
                    break;
                }
                match attr(current.0, "AXParent") {
                    Some(parent) => current = parent,
                    None => break,
                }
            }
            rects
        };
        rects.reverse(); // window first
        let mut out: Vec<Rect> = Vec::with_capacity(rects.len());
        for (fx, fy, fw, fh) in rects {
            let r = Rect::new(
                (fx * scale).round() as i32,
                (fy * scale).round() as i32,
                (fw * scale).round() as u32,
                (fh * scale).round() as u32,
            );
            // same rect as its parent (wrappers): one step of the wheel for nothing
            if out.last() != Some(&r) {
                out.push(r);
            }
        }
        out
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
