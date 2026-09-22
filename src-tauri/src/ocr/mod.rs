//! Text recognition using the OS engine: Windows.Media.Ocr on Windows, Vision on macOS,
//! and the `tesseract` CLI on Linux when it is installed.

use image::RgbaImage;
use serde::Serialize;

use crate::error::AppResult;
use crate::geom::Rect;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OcrWord {
    pub text: String,
    pub bbox: Rect,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OcrLine {
    pub text: String,
    /// Bounding box in pixels of the recognised image.
    pub bbox: Rect,
    /// Per-word boxes where the engine provides them (Windows); empty elsewhere.
    pub words: Vec<OcrWord>,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct OcrOutput {
    pub text: String,
    pub lines: Vec<OcrLine>,
}

/// Longest image side we hand to the engines. Larger inputs are downscaled.
const MAX_SIDE: u32 = 4000;

pub fn recognize(img: &RgbaImage, language: Option<&str>) -> AppResult<OcrOutput> {
    let (w, h) = img.dimensions();
    let longest = w.max(h);
    let scaled;
    let input = if longest > MAX_SIDE {
        let f = MAX_SIDE as f64 / longest as f64;
        scaled = image::imageops::resize(
            img,
            ((w as f64 * f) as u32).max(1),
            ((h as f64 * f) as u32).max(1),
            image::imageops::FilterType::Triangle,
        );
        &scaled
    } else {
        img
    };
    let mut out = platform_recognize(input, language)?;
    if input.width() != w {
        let f = w as f64 / input.width() as f64;
        let scale = |r: Rect| {
            Rect::new(
                (r.x as f64 * f) as i32,
                (r.y as f64 * f) as i32,
                (r.width as f64 * f) as u32,
                (r.height as f64 * f) as u32,
            )
        };
        for line in &mut out.lines {
            line.bbox = scale(line.bbox);
            for word in &mut line.words {
                word.bbox = scale(word.bbox);
            }
        }
    }
    Ok(out)
}

#[cfg(windows)]
fn platform_recognize(img: &RgbaImage, language: Option<&str>) -> AppResult<OcrOutput> {
    windows::recognize(img, language)
}

#[cfg(target_os = "macos")]
fn platform_recognize(img: &RgbaImage, language: Option<&str>) -> AppResult<OcrOutput> {
    macos::recognize(img, language)
}

#[cfg(target_os = "linux")]
fn platform_recognize(img: &RgbaImage, language: Option<&str>) -> AppResult<OcrOutput> {
    linux::recognize(img, language)
}

/// Join lines into text the way people expect to paste it: one line per visual line.
pub fn join_lines(lines: &[OcrLine]) -> String {
    lines
        .iter()
        .map(|l| l.text.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}
