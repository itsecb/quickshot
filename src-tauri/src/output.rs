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
use crate::{history, ocr, qr, redact, rules, ticket, windows};

pub fn after_capture(app: &AppHandle, capture: Arc<Capture>, mode: CaptureMode) -> AppResult<()> {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let produces_image = !matches!(
        mode,
        CaptureMode::Ocr | CaptureMode::Color | CaptureMode::Qr | CaptureMode::Watch
    );
    let rule = produces_image
        .then(|| {
            rules::effective(
                &settings.rules,
                &capture.source.app_name,
                &capture.source.title,
            )
        })
        .flatten();
    if let Some(r) = &rule {
        log::info!("capture matched rule(s): {}", r.name);
    }
    let auto_redact = rule.as_ref().is_some_and(|r| r.auto_redact);
    let auto_copy = rule.as_ref().is_some_and(|r| r.auto_copy);
    // Everything that leaves the editor (history, pins, direct copy/save, rule folders) gets
    // redacted pixels; the editor gets the original plus editable redaction boxes.
    let shared = if auto_redact {
        Arc::new(redacted_capture(&settings, &capture))
    } else {
        capture.clone()
    };
    if produces_image && !rule.as_ref().is_some_and(|r| r.skip_history) {
        history::spawn_record(app, shared.clone());
    }
    if let Some(dir) = rule.as_ref().and_then(|r| r.save_dir.clone()) {
        let mut to_folder = settings.clone();
        to_folder.save_dir = Some(dir);
        if let Err(e) = save_to_folder(app, &to_folder, &shared.image, &shared.source, None) {
            log::warn!("rule save failed: {e}");
            windows::toast(app, "Rule could not save the capture", &e.to_string());
        }
    }
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
            if auto_redact {
                state.replace_capture(shared.clone());
            }
            if auto_copy {
                copy_to_clipboard(app, &shared.image)?;
            }
            let app2 = app.clone();
            app.run_on_main_thread(move || {
                if let Err(e) = windows::open_pin(&app2, &shared, x, y) {
                    log::error!("pin failed: {e}");
                }
            })?;
            Ok(())
        }
        CaptureMode::Color | CaptureMode::Watch => Ok(()),
        CaptureMode::Qr => {
            let codes = qr::decode(&capture.image);
            state.remove_capture(capture.id);
            match codes.as_slice() {
                [] => windows::toast(
                    app,
                    "No code found",
                    "No QR code or barcode was found in the selection.",
                ),
                [one] => {
                    app.clipboard().write_text(one.text.clone())?;
                    let preview: String = one.text.chars().take(80).collect();
                    windows::toast(app, &format!("Copied {}", one.format), &preview);
                }
                many => {
                    let text = many
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    app.clipboard().write_text(text)?;
                    windows::toast(
                        app,
                        &format!("Copied {} codes", many.len()),
                        "One per line.",
                    );
                }
            }
            Ok(())
        }
        _ => match settings.after_capture {
            AfterCapture::Editor | AfterCapture::EditorAndCopy => {
                if settings.after_capture == AfterCapture::EditorAndCopy || auto_copy {
                    if let Err(e) = copy_to_clipboard(app, &shared.image) {
                        log::warn!("copy after capture failed: {e}");
                    }
                }
                if auto_redact {
                    state.auto_redact.lock().unwrap().insert(capture.id);
                }
                let app2 = app.clone();
                app.run_on_main_thread(move || {
                    if let Err(e) = windows::open_editor(&app2, &capture) {
                        log::error!("editor failed: {e}");
                    }
                })?;
                Ok(())
            }
            AfterCapture::Copy => {
                copy_to_clipboard(app, &shared.image)?;
                let size = format!("{}×{}", shared.image.width(), shared.image.height());
                confirm(app, &settings, shared, "Copied to clipboard", &size);
                Ok(())
            }
            AfterCapture::Save | AfterCapture::CopyAndSave => {
                let path = save_to_folder(app, &settings, &shared.image, &shared.source, None)?;
                if settings.after_capture == AfterCapture::CopyAndSave
                    || settings.copy_on_save
                    || auto_copy
                {
                    copy_to_clipboard(app, &shared.image)?;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                confirm(app, &settings, shared, "Saved", &name);
                Ok(())
            }
        },
    }
}

/// Tell the user a capture went to the clipboard/folder: the floating thumbnail (which keeps
/// the capture alive for its quick actions), or an OS notification when that's switched off.
fn confirm(app: &AppHandle, settings: &Settings, capture: Arc<Capture>, title: &str, detail: &str) {
    let state = app.state::<AppState>();
    if !settings.show_thumbnail {
        state.remove_capture(capture.id);
        windows::toast(app, title, detail);
        return;
    }
    // the thumbnail's actions work on exactly what was copied/saved (e.g. the redacted pixels)
    state.replace_capture(capture.clone());
    let status = format!("{title} · {detail}");
    let app2 = app.clone();
    let result = app.run_on_main_thread(move || {
        if let Err(e) = windows::open_thumb(&app2, &capture, &status) {
            log::error!("thumbnail failed: {e}");
        }
    });
    if let Err(e) = result {
        log::error!("thumbnail failed: {e}");
    }
}

/// A copy of `capture` with everything `redact` finds pixelated (for rules with auto-redact).
fn redacted_capture(settings: &Settings, capture: &Capture) -> Capture {
    let mut image = capture.image.clone();
    match ocr::recognize(&capture.image, settings.ocr_language.as_deref()) {
        Ok(out) => {
            for m in redact::find_sensitive(&out.lines, &settings.redact_patterns) {
                redact::pixelate(&mut image, m.rect, 12);
            }
        }
        Err(e) => log::warn!("auto-redact OCR failed: {e}"),
    }
    Capture {
        id: capture.id,
        image,
        source: capture.source.clone(),
        created: capture.created,
    }
}

pub fn copy_to_clipboard(app: &AppHandle, img: &RgbaImage) -> AppResult<()> {
    let image = tauri::image::Image::new(img.as_raw(), img.width(), img.height());
    app.clipboard().write_image(&image)?;
    Ok(())
}

/// Copy the image with a context caption for tickets and chats: PNG + bitmap + HTML
/// (caption above an embedded image) + plain-text caption, all in one clipboard write so
/// each app picks the richest format it understands.
pub fn copy_rich(app: &AppHandle, img: &RgbaImage, png: &[u8], caption: &str) -> AppResult<()> {
    let html = ticket::html(caption, png);
    #[cfg(windows)]
    {
        let _ = app;
        write_clipboard_windows(Some((img, png)), Some(&html), Some(caption))
    }
    #[cfg(not(windows))]
    {
        let _ = (img, png);
        app.clipboard()
            .write_html(html, Some(caption.to_string()))?;
        Ok(())
    }
}

/// One clipboard write with an optional image (PNG + CF_DIB), HTML and plain text, so every
/// app finds a format it likes. Also used by headless `--copy`, which has no Tauri runtime.
#[cfg(windows)]
pub fn write_clipboard_windows(
    image: Option<(&RgbaImage, &[u8])>,
    html: Option<&str>,
    text: Option<&str>,
) -> AppResult<()> {
    use clipboard_win::options::NoClear;
    use clipboard_win::{formats, raw, Clipboard};
    let err = |e: clipboard_win::ErrorCode| AppError::Other(format!("clipboard: {e}"));
    let _open = Clipboard::new_attempts(10).map_err(err)?;
    raw::empty().map_err(err)?;
    if let Some((img, png)) = image {
        if let Some(png_format) = raw::register_format("PNG") {
            raw::set_without_clear(png_format.get(), png).map_err(err)?;
        }
        raw::set_without_clear(formats::CF_DIB, &ticket::dib(img)).map_err(err)?;
    }
    if let (Some(html), Some(html_format)) = (html, raw::register_format("HTML Format")) {
        raw::set_html_with(html_format.get(), html, NoClear).map_err(err)?;
    }
    if let Some(text) = text.filter(|t| !t.is_empty()) {
        raw::set_string_with(text, NoClear).map_err(err)?;
    }
    Ok(())
}

/// Rich text for the clipboard: HTML (tables keep their cells in Excel/Outlook) plus a plain
/// text fallback (TSV pastes into cells too).
pub fn copy_html_text(app: &AppHandle, html: &str, text: &str) -> AppResult<()> {
    #[cfg(windows)]
    {
        let _ = app;
        write_clipboard_windows(None, Some(html), Some(text))
    }
    #[cfg(not(windows))]
    {
        app.clipboard()
            .write_html(html.to_string(), Some(text.to_string()))?;
        Ok(())
    }
}

/// Caption for a capture from the configured template.
pub fn ticket_caption(
    settings: &Settings,
    source: &CaptureSource,
    created: chrono::DateTime<chrono::Local>,
    width: u32,
    height: u32,
) -> String {
    ticket::expand_caption(
        &settings.ticket_caption,
        &ticket::CaptionInfo {
            title: &source.title,
            app: &source.app_name,
            created,
            width,
            height,
        },
        &ticket::host_name(),
        &ticket::user_name(),
    )
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
