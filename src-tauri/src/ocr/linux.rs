use std::process::Command;

use image::RgbaImage;

use super::{join_lines, OcrLine, OcrOutput};
use crate::error::{AppError, AppResult};
use crate::geom::Rect;

pub fn recognize(img: &RgbaImage, language: Option<&str>) -> AppResult<OcrOutput> {
    let dir = std::env::temp_dir().join("QuickShot");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("ocr-{}.png", std::process::id()));
    img.save(&path)?;
    let mut cmd = Command::new("tesseract");
    cmd.arg(&path).arg("stdout");
    if let Some(l) = language.filter(|l| !l.trim().is_empty()) {
        cmd.arg("-l").arg(l);
    }
    let output = cmd.output();
    let _ = std::fs::remove_file(&path);
    let output = output.map_err(|_| {
        AppError::Ocr("tesseract is not installed (apt install tesseract-ocr)".into())
    })?;
    if !output.status.success() {
        return Err(AppError::Ocr(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();
    let lines: Vec<OcrLine> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| OcrLine {
            text: l.to_string(),
            bbox: Rect::default(),
            words: Vec::new(),
        })
        .collect();
    let text = join_lines(&lines);
    Ok(OcrOutput { text, lines })
}
