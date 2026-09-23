//! Step recorder helpers (pure, unit-tested): mark where the user clicked on a step's
//! screenshot, and write the step's title from what was clicked.

use image::RgbaImage;

const ACCENT: [u8; 3] = [76, 141, 255];
const RING_RADIUS: f64 = 18.0;
const RING_WIDTH: f64 = 4.0;
const HALO_WIDTH: f64 = 2.0;
const DOT_RADIUS: f64 = 4.0;

fn blend(img: &mut RgbaImage, x: u32, y: u32, color: [u8; 3], alpha: f64) {
    if alpha <= 0.0 {
        return;
    }
    let a = alpha.min(1.0);
    let p = img.get_pixel_mut(x, y);
    for (channel, target) in p.0.iter_mut().zip(color) {
        *channel = (*channel as f64 * (1.0 - a) + target as f64 * a).round() as u8;
    }
    p.0[3] = p.0[3].max((a * 255.0) as u8);
}

/// Coverage (0..1) of a band `[inner, outer]` at distance `d`, with a 1 px soft edge.
fn band(d: f64, inner: f64, outer: f64) -> f64 {
    ((d - inner + 0.5).min(outer - d + 0.5)).clamp(0.0, 1.0)
}

/// Draw a click marker centred on (`cx`, `cy`): a white halo, an accent ring and a centre dot.
/// Points outside the image are ignored; partly visible markers are clipped.
pub fn mark_click(img: &mut RgbaImage, cx: i32, cy: i32) {
    let reach = (RING_RADIUS + RING_WIDTH / 2.0 + HALO_WIDTH + 1.0).ceil() as i32;
    let (w, h) = (img.width() as i32, img.height() as i32);
    for y in (cy - reach).max(0)..(cy + reach + 1).min(h) {
        for x in (cx - reach).max(0)..(cx + reach + 1).min(w) {
            let d = (((x - cx) as f64).powi(2) + ((y - cy) as f64).powi(2)).sqrt();
            let (inner, outer) = (
                RING_RADIUS - RING_WIDTH / 2.0,
                RING_RADIUS + RING_WIDTH / 2.0,
            );
            // halo just outside and inside the ring keeps it visible on any background
            let halo = band(d, inner - HALO_WIDTH, outer + HALO_WIDTH);
            blend(img, x as u32, y as u32, [255, 255, 255], halo * 0.9);
            blend(img, x as u32, y as u32, ACCENT, band(d, inner, outer));
            blend(img, x as u32, y as u32, ACCENT, band(d, -1.0, DOT_RADIUS));
        }
    }
}

/// Control types that don't help a reader ("Click the “Inbox” pane" reads worse than
/// "Click “Inbox”").
const VAGUE_CONTROLS: &[&str] = &[
    "pane", "window", "custom", "group", "document", "text", "image", "client", "thumb",
];

fn shorten(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        s
    } else {
        format!(
            "{}…",
            s.chars().take(max - 1).collect::<String>().trim_end()
        )
    }
}

/// What to call a window in a step: Windows titles usually end with the app name
/// ("Untitled - Notepad", "Inbox - me@contoso.com - Outlook"), so take the last part.
pub fn window_label(title: &str) -> String {
    shorten(title.rsplit(" - ").next().unwrap_or(title), 40)
}

/// "Click the “Save” button in Notepad", from the UI Automation name and control type of what
/// was clicked, and the window label (see `window_label`).
pub fn step_title(right_click: bool, name: &str, control: &str, window: &str) -> String {
    let verb = if right_click { "Right-click" } else { "Click" };
    let name = shorten(name, 60);
    let control = control.trim().to_lowercase();
    let window = shorten(window, 40);
    let mut out = if name.is_empty() {
        verb.to_string()
    } else if control.is_empty() || VAGUE_CONTROLS.contains(&control.as_str()) {
        format!("{verb} “{name}”")
    } else {
        format!("{verb} the “{name}” {control}")
    };
    if !window.is_empty() && !name.eq_ignore_ascii_case(&window) {
        out.push_str(" in ");
        out.push_str(&window);
    }
    out
}

/// A plain screenshot step (the recorder's "Add step now").
pub fn manual_title(window: &str) -> String {
    let window = shorten(window, 40);
    if window.is_empty() {
        "Screenshot".into()
    } else {
        format!("Screenshot of {window}")
    }
}

/// Test helper: is this pixel close to the accent colour?
#[cfg(test)]
fn is_accent(p: &image::Rgba<u8>) -> bool {
    p.0[0].abs_diff(ACCENT[0]) < 30
        && p.0[1].abs_diff(ACCENT[1]) < 30
        && p.0[2].abs_diff(ACCENT[2]) < 30
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn marker_draws_ring_dot_and_leaves_the_rest() {
        let mut img = RgbaImage::from_pixel(100, 100, Rgba([10, 10, 10, 255]));
        mark_click(&mut img, 50, 50);
        assert!(is_accent(img.get_pixel(50, 50)), "centre dot");
        assert!(is_accent(img.get_pixel(50 + 18, 50)), "ring");
        // between dot and ring: untouched (dark)
        assert_eq!(img.get_pixel(50 + 10, 50).0, [10, 10, 10, 255]);
        // far away: untouched
        assert_eq!(img.get_pixel(5, 5).0, [10, 10, 10, 255]);
        // halo just outside the ring is light
        assert!(img.get_pixel(50 + 21, 50).0[0] > 150);
    }

    #[test]
    fn marker_clips_at_the_edges() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0, 0, 0, 255]));
        mark_click(&mut img, 0, 0);
        mark_click(&mut img, -500, 900); // off-image: nothing, no panic
        assert!(is_accent(img.get_pixel(0, 0)));
    }

    #[test]
    fn titles_read_naturally() {
        assert_eq!(
            step_title(false, "Save", "button", &window_label("Untitled - Notepad")),
            "Click the “Save” button in Notepad"
        );
        assert_eq!(window_label("Inbox - me@contoso.com - Outlook"), "Outlook");
        assert_eq!(window_label("Event Viewer"), "Event Viewer");
        assert_eq!(
            step_title(true, "Inbox", "pane", "Outlook"),
            "Right-click “Inbox” in Outlook"
        );
        assert_eq!(
            step_title(false, "", "", "Event Viewer"),
            "Click in Event Viewer"
        );
        assert_eq!(step_title(false, "", "", ""), "Click");
        // don't repeat the window name when the thing clicked *is* the window
        assert_eq!(
            step_title(false, "Calculator", "window", "Calculator"),
            "Click “Calculator”"
        );
        let long = "x".repeat(200);
        assert!(step_title(false, &long, "edit", "App").chars().count() < 90);
        assert_eq!(
            manual_title(&window_label("Local - Services")),
            "Screenshot of Services"
        );
        assert_eq!(manual_title(""), "Screenshot");
    }
}
