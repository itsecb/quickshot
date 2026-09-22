//! Routing after a capture: editor, clipboard, file, pin, OCR.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::error::{AppError, AppResult};
use crate::image_util::{encode_jpeg, encode_png_small};
use crate::settings::{self, AfterCapture, ImageFormat, Settings};
use crate::state::{AppState, Capture, CaptureMode, CaptureSource};
use crate::{ocr, windows};

pub fn after_capture(app: &AppHandle, capture: Arc<Capture>, mode: CaptureMode) -> AppResult<()> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    match mode {
        CaptureMode::Ocr => {
            let result = ocr::recognize(&capture.image, settings.ocr_language.as_deref());
            state.remove_capture(capture.id);
            match result {
                Ok(out) if out.text.trim().is_empty() => windows::toast(
                    app,
                    "No text found",
                    "OCR did not find any text in the selection.",
                ),
                Ok(out) => {
                    app.clipboard().write_text(out.text.clone())?;
                    let preview: String = out.text.chars().take(80).collect();
                    windows::toast(app, "Text copied", &preview);
                }
                Err(e) => windows::toast(app, "OCR failed", &e.to_string()),
            }
            Ok(())
        }
        CaptureMode::Pin => {
            let (x, y) = (capture.source.rect.x, capture.source.rect.y);
            let app2 = app.clone();
            app.run_on_main_thread(move || {
                if let Err(e) = windows::open_pin(&app2, &capture, x, y) {
                    log::error!("pin failed: {e}");
                }
            })?;
            Ok(())
        }
        CaptureMode::Color => Ok(()),
        _ => match settings.after_capture {
            AfterCapture::Editor => {
                let app2 = app.clone();
                app.run_on_main_thread(move || {
                    if let Err(e) = windows::open_editor(&app2, &capture) {
                        log::error!("editor failed: {e}");
                    }
                })?;
                Ok(())
            }
            AfterCapture::Copy => {
                copy_to_clipboard(app, &capture.image)?;
                state.remove_capture(capture.id);
                windows::toast(
                    app,
                    "Copied to clipboard",
                    &format!("{}×{}", capture.image.width(), capture.image.height()),
                );
                Ok(())
            }
            AfterCapture::Save | AfterCapture::CopyAndSave => {
                let path = save_to_folder(app, &settings, &capture.image, &capture.source, None)?;
                if settings.after_capture == AfterCapture::CopyAndSave || settings.copy_on_save {
                    copy_to_clipboard(app, &capture.image)?;
                }
                state.remove_capture(capture.id);
                windows::toast(app, "Saved", &path.display().to_string());
                Ok(())
            }
        },
    }
}

pub fn copy_to_clipboard(app: &AppHandle, img: &RgbaImage) -> AppResult<()> {
    let image = tauri::image::Image::new(img.as_raw(), img.width(), img.height());
    app.clipboard().write_image(&image)?;
    Ok(())
}

/// Pick a unique path in `dir` for `stem.ext`, appending " (2)", " (3)" … on collision.
pub fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    for n in 2..10_000 {
        let p = dir.join(format!("{stem} ({n}).{ext}"));
        if !p.exists() {
            return p;
        }
    }
    dir.join(format!(
        "{stem}-{}.{ext}",
        chrono::Local::now().timestamp_millis()
    ))
}

pub fn encode_for(img: &RgbaImage, format: ImageFormat, jpeg_quality: u8) -> AppResult<Vec<u8>> {
    match format {
        ImageFormat::Png => encode_png_small(img),
        ImageFormat::Jpeg => encode_jpeg(img, jpeg_quality),
    }
}

pub fn save_to_folder(
    app: &AppHandle,
    settings: &Settings,
    img: &RgbaImage,
    source: &CaptureSource,
    format: Option<ImageFormat>,
) -> AppResult<PathBuf> {
    let dir = settings::save_dir(app, settings)?;
    let format = format.unwrap_or(settings.image_format);
    let stem = settings::expand_pattern(
        &settings.file_pattern,
        &source.app_name,
        &source.title,
        img.width(),
        img.height(),
    );
    let path = unique_path(&dir, &stem, format.extension());
    std::fs::write(&path, encode_for(img, format, settings.jpeg_quality)?)?;
    Ok(path)
}

/// Write already-encoded PNG bytes, transcoding to JPEG when the target extension asks for it.
pub fn write_encoded(path: &Path, png_bytes: &[u8], jpeg_quality: u8) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        let img = crate::image_util::decode_png(png_bytes)?;
        std::fs::write(path, encode_jpeg(&img, jpeg_quality)?)?;
    } else {
        std::fs::write(path, png_bytes)?;
    }
    Ok(())
}

fn temp_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().temp_dir()?.join("QuickShot");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Write a PNG into the temp folder (used for drag-out) and remember it for cleanup.
pub fn temp_png(app: &AppHandle, png_bytes: &[u8], stem: &str) -> AppResult<PathBuf> {
    let dir = temp_dir(app)?;
    let path = unique_path(&dir, stem, "png");
    std::fs::write(&path, png_bytes)?;
    app.state::<AppState>()
        .temp_files
        .lock()
        .unwrap()
        .push(path.clone());
    Ok(path)
}

/// Remove drag-out temp files older than a day.
pub fn cleanup_temp(app: &AppHandle) {
    let Ok(dir) = temp_dir(app) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(24 * 3600);
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if meta.modified().map(|m| m < cutoff).unwrap_or(false) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

pub fn ensure_png(bytes: &[u8]) -> AppResult<()> {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(AppError::Other("expected PNG bytes".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_path_appends_counter() {
        let dir = std::env::temp_dir().join(format!("qs-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p1 = unique_path(&dir, "a", "png");
        assert_eq!(p1.file_name().unwrap(), "a.png");
        std::fs::write(&p1, b"x").unwrap();
        let p2 = unique_path(&dir, "a", "png");
        assert_eq!(p2.file_name().unwrap(), "a (2).png");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ensure_png_checks_signature() {
        assert!(ensure_png(b"\x89PNG\r\n\x1a\n....").is_ok());
        assert!(ensure_png(b"GIF89a").is_err());
    }
}
