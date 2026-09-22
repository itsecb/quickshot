//! QR code and barcode reading (QR, DataMatrix, Aztec, PDF417, Code 128/39, EAN/UPC, …).

use image::RgbaImage;
use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Code {
    pub text: String,
    pub format: String,
}

fn scan(luma: Vec<u8>, width: u32, height: u32) -> Vec<Code> {
    match rxing::helpers::detect_multiple_in_luma(luma, width, height) {
        Ok(results) => results
            .iter()
            .map(|r| Code {
                text: r.getText().to_string(),
                format: r.getBarcodeFormat().to_string(),
            })
            .collect(),
        Err(_) => Vec::new(), // "not found" is reported as an error
    }
}

/// Every code in the image, de-duplicated, in reading order of detection.
/// Small on-screen codes (2–3 px per module) are retried at 2× so the detector can lock on.
pub fn decode(img: &RgbaImage) -> Vec<Code> {
    let gray = image::imageops::grayscale(img);
    let (w, h) = gray.dimensions();
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let mut codes = scan(gray.clone().into_raw(), w, h);
    if codes.is_empty() && w.max(h) <= 2000 {
        let big =
            image::imageops::resize(&gray, w * 2, h * 2, image::imageops::FilterType::Nearest);
        codes = scan(big.into_raw(), w * 2, h * 2);
    }
    let mut seen = std::collections::HashSet::new();
    codes.retain(|c| seen.insert(c.text.clone()));
    codes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_image_has_no_codes() {
        let img = RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 255]));
        assert!(decode(&img).is_empty());
        assert!(decode(&RgbaImage::new(0, 0)).is_empty());
    }
}
