use image::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_core_foundation::{CFData, CFRetained};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
};
use objc2_foundation::{NSArray, NSDictionary, NSString};
use objc2_vision::{
    VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
    VNRequestTextRecognitionLevel,
};

use super::{join_lines, OcrLine, OcrOutput};
use crate::error::{AppError, AppResult};
use crate::geom::Rect;

fn make_cgimage(img: &RgbaImage) -> AppResult<CFRetained<CGImage>> {
    let (w, h) = img.dimensions();
    let data = CFData::from_bytes(img.as_raw());
    let provider = CGDataProvider::with_cf_data(Some(&data))
        .ok_or_else(|| AppError::Ocr("CGDataProvider failed".into()))?;
    let space =
        CGColorSpace::new_device_rgb().ok_or_else(|| AppError::Ocr("color space failed".into()))?;
    // non-premultiplied RGBA, default (big-endian) byte order
    let bitmap_info = CGBitmapInfo(CGImageAlphaInfo::Last.0);
    // SAFETY: decode is null; the provider retains the CFData for the image's lifetime.
    unsafe {
        CGImage::new(
            w as usize,
            h as usize,
            8,
            32,
            (w * 4) as usize,
            Some(&space),
            bitmap_info,
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
    .ok_or_else(|| AppError::Ocr("CGImage creation failed".into()))
}

pub fn recognize(img: &RgbaImage, language: Option<&str>) -> AppResult<OcrOutput> {
    let (w, h) = img.dimensions();
    let cg = make_cgimage(img)?;
    let options: Retained<NSDictionary<VNImageOption, AnyObject>> = NSDictionary::new();
    // SAFETY: the options dictionary is empty and correctly typed.
    let handler = unsafe {
        VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            &cg,
            &options,
        )
    };
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    if let Some(tag) = language.filter(|t| !t.trim().is_empty()) {
        let langs = NSArray::from_retained_slice(&[NSString::from_str(tag)]);
        request.setRecognitionLanguages(&langs);
    }
    let req_ref: &VNRequest = &request;
    let requests = NSArray::from_slice(&[req_ref]);
    handler
        .performRequests_error(&requests)
        .map_err(|e| AppError::Ocr(e.localizedDescription().to_string()))?;

    let mut lines: Vec<OcrLine> = Vec::new();
    if let Some(results) = request.results() {
        for obs in results.iter() {
            let candidates = obs.topCandidates(1);
            let Some(best) = candidates.firstObject() else {
                continue;
            };
            let text = best.string().to_string();
            // SAFETY: plain getter returning a CGRect by value.
            let bb = unsafe { obs.boundingBox() };
            // Vision boxes are normalised with a bottom-left origin.
            let x = bb.origin.x * w as f64;
            let y = (1.0 - bb.origin.y - bb.size.height) * h as f64;
            lines.push(OcrLine {
                text,
                bbox: Rect::new(
                    x.round() as i32,
                    y.round() as i32,
                    (bb.size.width * w as f64).round() as u32,
                    (bb.size.height * h as f64).round() as u32,
                ),
            });
        }
    }
    // top-to-bottom, then left-to-right
    lines.sort_by(|a, b| a.bbox.y.cmp(&b.bbox.y).then(a.bbox.x.cmp(&b.bbox.x)));
    let text = join_lines(&lines);
    Ok(OcrOutput { text, lines })
}
