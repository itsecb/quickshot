use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::AppResult;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    pub region: String,
    pub window: String,
    pub fullscreen: String,
    pub repeat_last: String,
    pub ocr: String,
    pub pin: String,
    pub color: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            region: "Ctrl+Shift+1".into(),
            window: "Ctrl+Shift+2".into(),
            fullscreen: "Ctrl+Shift+3".into(),
            repeat_last: "Ctrl+Shift+4".into(),
            ocr: "Ctrl+Shift+O".into(),
            pin: "Ctrl+Shift+P".into(),
            color: "Ctrl+Shift+C".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum AfterCapture {
    #[default]
    Editor,
    Copy,
    Save,
    CopyAndSave,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorDefaults {
    pub stroke_color: String,
    pub stroke_width: u32,
    pub font_size: u32,
    pub font_family: String,
    pub palette: Vec<String>,
    pub blur_amount: u32,
    pub badge_size: u32,
    pub shadow: bool,
    /// tool id -> key (single character or key name), e.g. "arrow" -> "a"
    pub shortcuts: BTreeMap<String, String>,
}

impl Default for EditorDefaults {
    fn default() -> Self {
        let shortcuts: BTreeMap<String, String> = [
            ("select", "v"),
            ("arrow", "a"),
            ("line", "l"),
            ("rect", "r"),
            ("ellipse", "o"),
            ("pen", "p"),
            ("text", "t"),
            ("highlighter", "h"),
            ("blur", "b"),
            ("badge", "n"),
            ("crop", "c"),
            ("measure", "m"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        Self {
            stroke_color: "#ff3b30".into(),
            stroke_width: 4,
            font_size: 22,
            font_family: "Segoe UI, -apple-system, Helvetica, Arial, sans-serif".into(),
            palette: vec![
                "#ff3b30".into(),
                "#ff9500".into(),
                "#ffcc00".into(),
                "#34c759".into(),
                "#007aff".into(),
                "#af52de".into(),
                "#ffffff".into(),
                "#000000".into(),
                "#8e8e93".into(),
            ],
            blur_amount: 12,
            badge_size: 28,
            shadow: true,
            shortcuts,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkeys: Hotkeys,
    /// Folder for saved screenshots. `None` = Pictures/QuickShot.
    pub save_dir: Option<String>,
    /// Filename pattern. Tokens: {date} {time} {datetime} {app} {title} {w} {h}
    pub file_pattern: String,
    pub image_format: ImageFormat,
    pub jpeg_quality: u8,
    pub after_capture: AfterCapture,
    pub copy_on_save: bool,
    pub show_magnifier: bool,
    pub play_sound: bool,
    pub autostart: bool,
    pub ocr_language: Option<String>,
    pub guides_dir: Option<String>,
    pub editor: EditorDefaults,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys: Hotkeys::default(),
            save_dir: None,
            file_pattern: "Screenshot {date} {time}".into(),
            image_format: ImageFormat::Png,
            jpeg_quality: 90,
            after_capture: AfterCapture::Editor,
            copy_on_save: true,
            show_magnifier: true,
            play_sound: false,
            autostart: false,
            ocr_language: None,
            guides_dir: None,
            editor: EditorDefaults::default(),
        }
    }
}

fn settings_path(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app.path().app_config_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    match settings_path(app).and_then(|p| Ok(std::fs::read_to_string(p)?)) {
        Ok(text) => match serde_json::from_str::<Settings>(&text) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("settings.json is invalid, using defaults: {e}");
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    }
}

pub fn save(app: &AppHandle, settings: &Settings) -> AppResult<()> {
    let path = settings_path(app)?;
    let text = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, text)?;
    Ok(())
}

/// Resolved folder where screenshots are saved.
pub fn save_dir(app: &AppHandle, settings: &Settings) -> AppResult<PathBuf> {
    let dir = match &settings.save_dir {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d),
        _ => app.path().picture_dir()?.join("QuickShot"),
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Resolved folder where guide projects live.
pub fn guides_dir(app: &AppHandle, settings: &Settings) -> AppResult<PathBuf> {
    let dir = match &settings.guides_dir {
        Some(d) if !d.trim().is_empty() => PathBuf::from(d),
        _ => app.path().document_dir()?.join("QuickShot Guides"),
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Expand the filename pattern. Never includes an extension.
pub fn expand_pattern(pattern: &str, app_name: &str, title: &str, w: u32, h: u32) -> String {
    let now = chrono::Local::now();
    let mut out = pattern
        .replace("{date}", &now.format("%Y-%m-%d").to_string())
        .replace("{time}", &now.format("%H-%M-%S").to_string())
        .replace("{datetime}", &now.format("%Y%m%d-%H%M%S").to_string())
        .replace("{app}", app_name)
        .replace("{title}", title)
        .replace("{w}", &w.to_string())
        .replace("{h}", &h.to_string());
    // strip characters that are illegal in Windows filenames
    out.retain(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'));
    let trimmed = out.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        "Screenshot".into()
    } else {
        trimmed.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pattern_strips_illegal_chars() {
        let s = expand_pattern("{title} {w}x{h}", "x", "Server: prod/01?", 10, 20);
        assert_eq!(s, "Server prod01 10x20");
    }

    #[test]
    fn pattern_empty_falls_back() {
        assert_eq!(expand_pattern("???", "", "", 1, 1), "Screenshot");
    }

    #[test]
    fn settings_roundtrip_with_missing_fields() {
        let s: Settings = serde_json::from_str(r#"{"filePattern":"x"}"#).unwrap();
        assert_eq!(s.file_pattern, "x");
        assert_eq!(s.hotkeys.region, "Ctrl+Shift+1");
        assert_eq!(s.editor.shortcuts.get("arrow").unwrap(), "a");
    }
}
