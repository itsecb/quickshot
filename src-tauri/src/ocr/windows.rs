use image::RgbaImage;
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;

use super::{join_lines, OcrLine, OcrOutput};
use crate::error::{AppError, AppResult};
use crate::geom::Rect;
use crate::image_util::rgba_to_bgra;

fn werr(e: windows::core::Error) -> AppError {
    AppError::Ocr(e.message().to_string())
}

fn create_engine(language: Option<&str>) -> AppResult<OcrEngine> {
    if let Some(tag) = language.filter(|t| !t.trim().is_empty()) {
        let lang = Language::CreateLanguage(&HSTRING::from(tag)).map_err(werr)?;
        if OcrEngine::IsLanguageSupported(&lang).map_err(werr)? {
            return OcrEngine::TryCreateFromLanguage(&lang).map_err(werr);
        }
        log::warn!("OCR language {tag} not installed; falling back to profile languages");
    }
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        AppError::Ocr(
            "no OCR language pack is installed. Windows Settings → Time & Language → Language → \
             add a language and install its 'Optical character recognition' feature."
                .into(),
        )
    })
}

pub fn recognize(img: &RgbaImage, language: Option<&str>) -> AppResult<OcrOutput> {
    let max_dim = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let (w, h) = img.dimensions();
    let resized;
    let img = if w > max_dim || h > max_dim {
        let f = max_dim as f64 / w.max(h) as f64;
        resized = image::imageops::resize(
            img,
            ((w as f64 * f) as u32).max(1),
            ((h as f64 * f) as u32).max(1),
            image::imageops::FilterType::Triangle,
        );
        &resized
    } else {
        img
    };
    let scale_back = w as f64 / img.width() as f64;

    let bgra = rgba_to_bgra(img);
    let buffer = CryptographicBuffer::CreateFromByteArray(&bgra).map_err(werr)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        img.width() as i32,
        img.height() as i32,
        BitmapAlphaMode::Premultiplied,
    )
    .map_err(werr)?;

    let engine = create_engine(language)?;
    let result = engine
        .RecognizeAsync(&bitmap)
        .map_err(werr)?
        .get()
        .map_err(werr)?;

    let mut lines = Vec::new();
    for line in result.Lines().map_err(werr)? {
        let text = line.Text().map_err(werr)?.to_string();
        let mut bbox: Option<Rect> = None;
        for word in line.Words().map_err(werr)? {
            let r = word.BoundingRect().map_err(werr)?;
            let wr = Rect::new(
                (r.X as f64 * scale_back) as i32,
                (r.Y as f64 * scale_back) as i32,
                (r.Width as f64 * scale_back) as u32,
                (r.Height as f64 * scale_back) as u32,
            );
            bbox = Some(match bbox {
                Some(b) => Rect::union_all([b, wr]).unwrap_or(wr),
                None => wr,
            });
        }
        lines.push(OcrLine {
            text,
            bbox: bbox.unwrap_or_default(),
        });
    }
    let text = join_lines(&lines);
    Ok(OcrOutput { text, lines })
}
