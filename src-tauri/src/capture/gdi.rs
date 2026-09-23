//! Fast screen grab on Windows: a GDI BitBlt of each monitor from the desktop DC, all monitors
//! at once. Windows Graphics Capture (xcap's path) sets up a capture session per monitor and
//! waits for a frame, which took ~450 ms for three monitors; BitBlt returns in a few tens of ms.
//! Windows marked "exclude from capture" (our floating bars) are left out either way.

use image::RgbaImage;

use crate::geom::Rect;

/// Grab each rect (global physical px) in parallel. `None` where GDI failed (the caller falls
/// back to xcap for those).
pub fn grab_all(rects: &[Rect]) -> Vec<Option<RgbaImage>> {
    std::thread::scope(|scope| {
        let jobs: Vec<_> = rects
            .iter()
            .map(|r| scope.spawn(move || grab(*r)))
            .collect();
        jobs.into_iter().map(|j| j.join().ok().flatten()).collect()
    })
}

#[cfg(windows)]
fn grab(r: Rect) -> Option<RgbaImage> {
    use std::ffi::c_void;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HGDIOBJ,
        SRCCOPY,
    };

    let (w, h) = (r.width as i32, r.height as i32);
    if w <= 0 || h <= 0 {
        return None;
    }
    // SAFETY: every GDI object created here is selected out and released before returning;
    // `bits` points to w*h*4 bytes owned by the DIB section, read before it's deleted.
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let mut out = None;
        if let Ok(dib) = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            let old = SelectObject(mem, HGDIOBJ(dib.0));
            // CAPTUREBLT includes layered windows: menus, tooltips, toasts
            if BitBlt(
                mem,
                0,
                0,
                w,
                h,
                Some(screen),
                r.x,
                r.y,
                SRCCOPY | CAPTUREBLT,
            )
            .is_ok()
                && !bits.is_null()
            {
                let len = w as usize * h as usize * 4;
                let bgra = std::slice::from_raw_parts(bits as *const u8, len);
                let mut rgba = Vec::with_capacity(len);
                for px in bgra.chunks_exact(4) {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                }
                out = RgbaImage::from_raw(w as u32, h as u32, rgba);
            }
            SelectObject(mem, old);
            let _ = DeleteObject(HGDIOBJ(dib.0));
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        out
    }
}

#[cfg(not(windows))]
fn grab(_r: Rect) -> Option<RgbaImage> {
    None
}
