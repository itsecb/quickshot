//! Unifies xcap monitor geometry into global physical pixels.
//!
//! * Windows / Linux: xcap reports physical pixels for position and size.
//! * macOS: xcap reports logical points (CGDisplayBounds) while the captured
//!   image is in physical pixels, so position must be multiplied by the scale.

use serde::Serialize;

use crate::geom::Rect;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    /// Global physical pixel position of the top-left corner.
    pub x: i32,
    pub y: i32,
    /// Physical pixel size (matches the captured image).
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub is_primary: bool,
    /// URL the overlay fetches the frozen frame from.
    pub frame_url: String,
}

impl MonitorInfo {
    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }
}

/// Raw geometry as reported by xcap, before platform normalisation.
#[derive(Clone, Copy, Debug)]
pub struct RawGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// Convert xcap geometry into physical pixels.
/// `image_size` is the size of the captured image and is always physical.
pub fn to_physical(raw: RawGeometry, image_size: (u32, u32), coords_are_logical: bool) -> Rect {
    if coords_are_logical {
        let s = if raw.scale > 0.0 { raw.scale } else { 1.0 };
        Rect::new(
            (raw.x as f64 * s).round() as i32,
            (raw.y as f64 * s).round() as i32,
            image_size.0,
            image_size.1,
        )
    } else {
        // Trust the image for size; xcap width/height should match but rotation
        // edge cases exist, and the image is what the overlay actually paints.
        let _ = raw.width;
        let _ = raw.height;
        Rect::new(raw.x, raw.y, image_size.0, image_size.1)
    }
}

pub const COORDS_ARE_LOGICAL: bool = cfg!(target_os = "macos");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_style_physical_passthrough() {
        let raw = RawGeometry {
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        };
        assert_eq!(
            to_physical(raw, (1920, 1080), false),
            Rect::new(-1920, 0, 1920, 1080)
        );
    }

    #[test]
    fn windows_mixed_dpi_uses_image_size() {
        // 150% monitor: xcap reports physical already; image is physical.
        let raw = RawGeometry {
            x: 2560,
            y: 0,
            width: 2880,
            height: 1620,
            scale: 1.5,
        };
        assert_eq!(
            to_physical(raw, (2880, 1620), false),
            Rect::new(2560, 0, 2880, 1620)
        );
    }

    #[test]
    fn macos_logical_scaled() {
        // Retina 2x secondary display placed at logical (1728, -200)
        let raw = RawGeometry {
            x: 1728,
            y: -200,
            width: 1440,
            height: 900,
            scale: 2.0,
        };
        assert_eq!(
            to_physical(raw, (2880, 1800), true),
            Rect::new(3456, -400, 2880, 1800)
        );
    }

    #[test]
    fn macos_zero_scale_is_safe() {
        let raw = RawGeometry {
            x: 10,
            y: 10,
            width: 100,
            height: 100,
            scale: 0.0,
        };
        assert_eq!(
            to_physical(raw, (100, 100), true),
            Rect::new(10, 10, 100, 100)
        );
    }
}
