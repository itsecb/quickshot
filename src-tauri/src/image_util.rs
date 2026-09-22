use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, RgbaImage};

use crate::error::AppResult;

/// Fast PNG encode (larger files, much faster than default compression).
pub fn encode_png(img: &RgbaImage) -> AppResult<Vec<u8>> {
    let mut out = Vec::with_capacity((img.width() * img.height()) as usize);
    let enc = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Sub);
    enc.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

/// Best-compression PNG encode for files written to disk.
pub fn encode_png_small(img: &RgbaImage) -> AppResult<Vec<u8>> {
    let mut out = Vec::with_capacity((img.width() * img.height()) as usize);
    let enc =
        PngEncoder::new_with_quality(&mut out, CompressionType::Default, FilterType::Adaptive);
    enc.write_image(
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(out)
}

pub fn encode_jpeg(img: &RgbaImage, quality: u8) -> AppResult<Vec<u8>> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut out = Vec::new();
    let enc = JpegEncoder::new_with_quality(&mut out, quality.clamp(10, 100));
    enc.write_image(
        rgb.as_raw(),
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

pub fn decode_png(bytes: &[u8]) -> AppResult<RgbaImage> {
    Ok(image::load_from_memory(bytes)?.to_rgba8())
}

pub fn rgba_to_bgra(img: &RgbaImage) -> Vec<u8> {
    let mut out = img.as_raw().clone();
    for px in out.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    out
}

/// Copy `src` into `dst` at (dx, dy), clipping to `dst` bounds.
pub fn blit(dst: &mut RgbaImage, src: &RgbaImage, dx: i64, dy: i64) {
    let (dw, dh) = (dst.width() as i64, dst.height() as i64);
    let (sw, sh) = (src.width() as i64, src.height() as i64);
    let x0 = dx.max(0);
    let y0 = dy.max(0);
    let x1 = (dx + sw).min(dw);
    let y1 = (dy + sh).min(dh);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let row_len = ((x1 - x0) * 4) as usize;
    let src_raw = src.as_raw();
    // Index the flat subpixel buffer; ImageBuffer's own Index takes (x, y).
    let dst_raw: &mut [u8] = &mut **dst;
    for y in y0..y1 {
        let sy = (y - dy) as usize;
        let sx = (x0 - dx) as usize;
        let s_start = (sy * sw as usize + sx) * 4;
        let d_start = (y as usize * dw as usize + x0 as usize) * 4;
        dst_raw[d_start..d_start + row_len].copy_from_slice(&src_raw[s_start..s_start + row_len]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blit_clips() {
        let mut dst = RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 0, 255]));
        let src = RgbaImage::from_pixel(3, 3, image::Rgba([255, 0, 0, 255]));
        blit(&mut dst, &src, 2, 2);
        assert_eq!(dst.get_pixel(3, 3).0, [255, 0, 0, 255]);
        assert_eq!(dst.get_pixel(1, 1).0, [0, 0, 0, 255]);
        blit(&mut dst, &src, -2, -2);
        assert_eq!(dst.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(dst.get_pixel(1, 1).0, [0, 0, 0, 255]);
    }

    #[test]
    fn png_roundtrip() {
        let img = RgbaImage::from_pixel(5, 3, image::Rgba([1, 2, 3, 255]));
        let png = encode_png(&img).unwrap();
        let back = decode_png(&png).unwrap();
        assert_eq!(back.dimensions(), (5, 3));
        assert_eq!(back.get_pixel(4, 2).0, [1, 2, 3, 255]);
    }
}
