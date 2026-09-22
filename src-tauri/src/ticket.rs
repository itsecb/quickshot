//! "Copy for ticket": the image plus a context caption (window, app, time, host) as rich
//! clipboard content. Pure helpers here; the clipboard write lives in `output::copy_rich`.

use base64::Engine;
use chrono::{DateTime, Local};
use image::RgbaImage;

pub struct CaptionInfo<'a> {
    pub title: &'a str,
    pub app: &'a str,
    pub created: DateTime<Local>,
    pub width: u32,
    pub height: u32,
}

pub fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_string())
        .unwrap_or_default()
}

pub fn user_name() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default()
}

/// Expand `{title} {app} {date} {time} {datetime} {host} {user} {w} {h}`. The template is split
/// on ` · `; parts that end up empty are dropped along with their separator, and dangling
/// dashes are trimmed, so a missing app or title never leaves "Title —  · ".
pub fn expand_caption(template: &str, info: &CaptionInfo, host: &str, user: &str) -> String {
    let expand = |part: &str| {
        part.replace("{title}", info.title)
            .replace("{app}", info.app)
            .replace("{date}", &info.created.format("%Y-%m-%d").to_string())
            .replace("{time}", &info.created.format("%H:%M").to_string())
            .replace(
                "{datetime}",
                &info.created.format("%Y-%m-%d %H:%M").to_string(),
            )
            .replace("{host}", host)
            .replace("{user}", user)
            .replace("{w}", &info.width.to_string())
            .replace("{h}", &info.height.to_string())
    };
    template
        .split(" · ")
        .map(|part| {
            expand(part)
                .trim_matches(|c: char| c.is_whitespace() || matches!(c, '—' | '–' | '-' | '|'))
                .to_string()
        })
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// HTML fragment: caption line above the image, embedded as a data URI so it pastes into
/// web apps (ServiceNow, Jira, Teams, Gmail) with no file attached separately.
pub fn html(caption: &str, png: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let cap = escape_html(caption);
    let mut out = String::with_capacity(b64.len() + cap.len() * 2 + 200);
    if !cap.is_empty() {
        out.push_str(&format!(
            "<p style=\"font-family:Segoe UI,Arial,sans-serif;font-size:12px;color:#555;margin:0 0 6px\">{cap}</p>"
        ));
    }
    out.push_str(&format!(
        "<img src=\"data:image/png;base64,{b64}\" alt=\"{cap}\">"
    ));
    out
}

/// CF_DIB payload: BITMAPINFOHEADER + bottom-up 32-bit BGRA rows, for apps that ignore PNG.
/// Only the Windows clipboard path uses it; the tests run everywhere.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn dib(img: &RgbaImage) -> Vec<u8> {
    let (w, h) = img.dimensions();
    let stride = w as usize * 4;
    let mut out = Vec::with_capacity(40 + stride * h as usize);
    out.extend_from_slice(&40u32.to_le_bytes()); // biSize
    out.extend_from_slice(&(w as i32).to_le_bytes()); // biWidth
    out.extend_from_slice(&(h as i32).to_le_bytes()); // biHeight > 0: bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&((stride * h as usize) as u32).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&[0u8; 16]); // resolution + palette fields
    for row in img.rows().rev() {
        for px in row {
            let [r, g, b, a] = px.0;
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn info<'a>(title: &'a str, app: &'a str) -> CaptionInfo<'a> {
        CaptionInfo {
            title,
            app,
            created: Local.with_ymd_and_hms(2026, 9, 22, 14, 5, 0).unwrap(),
            width: 800,
            height: 600,
        }
    }

    const T: &str = "{title} — {app} · {datetime} · {host}";

    #[test]
    fn caption_full() {
        assert_eq!(
            expand_caption(T, &info("Event Viewer", "mmc"), "WS-042", "ecb"),
            "Event Viewer — mmc · 2026-09-22 14:05 · WS-042"
        );
    }

    #[test]
    fn caption_drops_empty_parts() {
        assert_eq!(
            expand_caption(T, &info("Event Viewer", ""), "", ""),
            "Event Viewer · 2026-09-22 14:05"
        );
        assert_eq!(
            expand_caption(T, &info("", ""), "WS-042", ""),
            "2026-09-22 14:05 · WS-042"
        );
        assert_eq!(expand_caption("", &info("a", "b"), "h", "u"), "");
    }

    #[test]
    fn html_escapes_caption_and_embeds_png() {
        let h = html("a <b> & \"c\"", b"PNG");
        assert!(h.contains("a &lt;b&gt; &amp; &quot;c&quot;"));
        assert!(h.contains("src=\"data:image/png;base64,UE5H\""));
        assert!(!html("", b"x").contains("<p"));
    }

    #[test]
    fn dib_is_bottom_up_bgra() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, image::Rgba([1, 2, 3, 4])); // top-left
        img.put_pixel(1, 1, image::Rgba([5, 6, 7, 8])); // bottom-right
        let d = dib(&img);
        assert_eq!(d.len(), 40 + 16);
        assert_eq!(&d[0..4], &40u32.to_le_bytes());
        // first stored row is the bottom one: [0,0,0,0] then bottom-right in BGRA
        assert_eq!(&d[40 + 4..40 + 8], &[7, 6, 5, 8]);
        // last row is the top one, starting with top-left in BGRA
        assert_eq!(&d[48..52], &[3, 2, 1, 4]);
    }
}
