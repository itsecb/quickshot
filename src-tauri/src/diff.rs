//! Before/after comparison of two screenshots: optional auto-alignment (the same page captured
//! a few pixels apart), then a cell grid of changed areas merged into boxes.

use image::{GrayImage, RgbaImage};
use serde::Serialize;

use crate::geom::Rect;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    /// Content at (x, y) in A appears at (x + offset_x, y + offset_y) in B.
    pub offset_x: i32,
    pub offset_y: i32,
    /// Changed areas in B's pixel coordinates.
    pub boxes: Vec<Rect>,
    /// Share of compared cells that changed, 0–100.
    pub changed_percent: f64,
    pub same_size: bool,
}

pub struct DiffOptions {
    /// Grid cell size in pixels.
    pub cell: u32,
    /// Per-channel difference that counts as a changed pixel (ignores compression/AA noise).
    pub tolerance: u8,
    /// Pixels that must change inside a cell before the cell counts.
    pub min_pixels: u32,
    /// Search for the best offset up to this many pixels (0 = no alignment).
    pub max_shift: u32,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            cell: 16,
            tolerance: 24,
            min_pixels: 3,
            max_shift: 32,
        }
    }
}

/// Coarse alignment runs on images shrunk by this factor.
const COARSE: u32 = 4;

/// Integer luma, much faster than a float conversion on 4K frames.
fn luma(img: &RgbaImage) -> GrayImage {
    let data = img
        .as_raw()
        .chunks(4)
        .map(|p| ((p[0] as u32 * 77 + p[1] as u32 * 150 + p[2] as u32 * 29) >> 8) as u8)
        .collect();
    GrayImage::from_raw(img.width(), img.height(), data).expect("same dimensions")
}

/// Box-average downscale by `COARSE` (much faster than a filtered resize, and smooth enough).
fn small_gray(gray: &GrayImage) -> GrayImage {
    let (w, h) = gray.dimensions();
    let (sw, sh) = ((w / COARSE).max(1), (h / COARSE).max(1));
    let raw = gray.as_raw();
    let mut out = GrayImage::new(sw, sh);
    for sy in 0..sh {
        for sx in 0..sw {
            let (mut sum, mut n) = (0u32, 0u32);
            for y in (sy * COARSE)..((sy + 1) * COARSE).min(h) {
                let row = &raw[(y * w) as usize..][..w as usize];
                for x in (sx * COARSE)..((sx + 1) * COARSE).min(w) {
                    sum += row[x as usize] as u32;
                    n += 1;
                }
            }
            out.put_pixel(sx, sy, image::Luma([(sum / n.max(1)) as u8]));
        }
    }
    out
}

/// Mean absolute difference between A and B shifted by (dx, dy), sampled on a stride.
/// `None` when the overlap is too small to judge.
fn score(a: &GrayImage, b: &GrayImage, dx: i32, dy: i32, stride: u32) -> Option<f64> {
    let (aw, ah) = (a.width() as i32, a.height() as i32);
    let (bw, bh) = (b.width() as i32, b.height() as i32);
    let x0 = 0.max(-dx);
    let y0 = 0.max(-dy);
    let x1 = aw.min(bw - dx);
    let y1 = ah.min(bh - dy);
    // require most of the smaller image to overlap, so a big shift can't "win" on a sliver
    let need = (aw.min(bw) * ah.min(bh)) as i64 * 6 / 10;
    if x1 <= x0 || y1 <= y0 || ((x1 - x0) as i64 * (y1 - y0) as i64) < need {
        return None;
    }
    let (ra, rb) = (a.as_raw(), b.as_raw());
    let (mut sum, mut n) = (0u64, 0u64);
    for y in (y0..y1).step_by(stride as usize) {
        let row_a = &ra[(y * aw) as usize..][..aw as usize];
        let row_b = &rb[((y + dy) * bw) as usize..][..bw as usize];
        for x in (x0..x1).step_by(stride as usize) {
            sum += row_a[x as usize].abs_diff(row_b[(x + dx) as usize]) as u64;
            n += 1;
        }
    }
    (n > 0).then(|| sum as f64 / n as f64)
}

/// The `keep` lowest-scoring offsets, best first. Ties keep candidate order, so listing
/// (0, 0) first means identical images stay unshifted.
fn best_offsets(
    a: &GrayImage,
    b: &GrayImage,
    candidates: impl Iterator<Item = (i32, i32)>,
    stride: u32,
    keep: usize,
) -> Vec<(i32, i32)> {
    let mut scored: Vec<(f64, (i32, i32))> = candidates
        .filter_map(|(dx, dy)| score(a, b, dx, dy, stride).map(|s| (s, (dx, dy))))
        .collect();
    scored.sort_by(|x, y| x.0.total_cmp(&y.0));
    scored.into_iter().take(keep).map(|(_, o)| o).collect()
}

/// Every offset within ±r of `center`.
fn square(center: (i32, i32), r: i32) -> impl Iterator<Item = (i32, i32)> {
    let (cx, cy) = center;
    (cy - r..=cy + r).flat_map(move |dy| (cx - r..=cx + r).map(move |dx| (dx, dy)))
}

/// Best (dx, dy) within ±max_shift: coarse search on shrunk images, then the few best coarse
/// candidates (and "no shift") are refined at full size.
fn align(a: &RgbaImage, b: &RgbaImage, max_shift: u32) -> (i32, i32) {
    if max_shift == 0 {
        return (0, 0);
    }
    let (ga, gb) = (luma(a), luma(b));
    let (sa, sb) = (small_gray(&ga), small_gray(&gb));
    let r = max_shift.div_ceil(COARSE) as i32;
    let coarse = best_offsets(
        &sa,
        &sb,
        std::iter::once((0, 0)).chain(square((0, 0), r)),
        // full density here: striding aliases with repeating text rows and picks wrong offsets
        1,
        3,
    );
    let c = COARSE as i32;
    let mut refine = vec![(0, 0)];
    for (cx, cy) in coarse {
        for o in square((cx * c, cy * c), c / 2 + 1) {
            if !refine.contains(&o) {
                refine.push(o);
            }
        }
    }
    // only nearby offsets compete here (the right one scores ~0), so sparse sampling is safe
    best_offsets(&ga, &gb, refine.into_iter(), 4, 1)
        .first()
        .copied()
        .unwrap_or((0, 0))
}

pub fn diff(a: &RgbaImage, b: &RgbaImage, opts: &DiffOptions) -> DiffResult {
    let same_size = a.dimensions() == b.dimensions();
    let (dx, dy) = align(a, b, opts.max_shift);
    // overlap in B's coordinates
    let bx0 = dx.max(0);
    let by0 = dy.max(0);
    let bx1 = (a.width() as i32 + dx).min(b.width() as i32);
    let by1 = (a.height() as i32 + dy).min(b.height() as i32);
    let mut result = DiffResult {
        offset_x: dx,
        offset_y: dy,
        boxes: Vec::new(),
        changed_percent: 0.0,
        same_size,
    };
    if bx1 <= bx0 || by1 <= by0 {
        return result;
    }
    let cell = opts.cell.max(1) as i32;
    let cols = ((bx1 - bx0 + cell - 1) / cell) as usize;
    let rows = ((by1 - by0 + cell - 1) / cell) as usize;
    let mut counts = vec![0u32; cols * rows];
    let (ra, rb) = (a.as_raw(), b.as_raw());
    let (aw, bw) = (a.width() as usize, b.width() as usize);
    for y in by0..by1 {
        let row = ((y - by0) / cell) as usize * cols;
        let line_b = &rb[y as usize * bw * 4..][..bw * 4];
        let line_a = &ra[(y - dy) as usize * aw * 4..][..aw * 4];
        for x in bx0..bx1 {
            let pb = &line_b[x as usize * 4..][..3];
            let pa = &line_a[(x - dx) as usize * 4..][..3];
            if pa
                .iter()
                .zip(pb)
                .any(|(p, q)| p.abs_diff(*q) > opts.tolerance)
            {
                counts[row + ((x - bx0) / cell) as usize] += 1;
            }
        }
    }
    let changed: Vec<bool> = counts.iter().map(|&n| n >= opts.min_pixels).collect();
    let changed_cells = changed.iter().filter(|&&c| c).count();
    result.changed_percent = changed_cells as f64 * 100.0 / changed.len() as f64;

    // 8-connected groups of changed cells -> one box each
    let mut seen = vec![false; changed.len()];
    for start in 0..changed.len() {
        if !changed[start] || seen[start] {
            continue;
        }
        let (mut c0, mut r0, mut c1, mut r1) = (usize::MAX, usize::MAX, 0, 0);
        let mut stack = vec![start];
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (c, r) = (i % cols, i / cols);
            c0 = c0.min(c);
            r0 = r0.min(r);
            c1 = c1.max(c);
            r1 = r1.max(r);
            for nr in r.saturating_sub(1)..=(r + 1).min(rows - 1) {
                for nc in c.saturating_sub(1)..=(c + 1).min(cols - 1) {
                    let j = nr * cols + nc;
                    if changed[j] && !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        let x = bx0 + c0 as i32 * cell;
        let y = by0 + r0 as i32 * cell;
        let right = (bx0 + (c1 as i32 + 1) * cell).min(bx1);
        let bottom = (by0 + (r1 as i32 + 1) * cell).min(by1);
        result
            .boxes
            .push(Rect::new(x, y, (right - x) as u32, (bottom - y) as u32));
    }
    // biggest changes first
    result
        .boxes
        .sort_by_key(|r| std::cmp::Reverse(r.width as u64 * r.height as u64));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// Something that looks like a UI: a light page, panels of different shades, and rows
    /// of dark "text" strokes of varying length.
    fn page(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let panel = (x / 70 + y / 50) % 3;
            let base = [245u8, 232, 210][panel as usize];
            let in_line = y % 12 < 3;
            let word = (x + (y / 12) * 17) % 37 < 20 + (y / 12 % 5) * 3;
            if in_line && word {
                Rgba([30, 40, 60, 255])
            } else {
                Rgba([base, base, base.saturating_sub(10), 255])
            }
        })
    }

    fn paint(img: &mut RgbaImage, r: Rect, color: [u8; 4]) {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                img.put_pixel(x as u32, y as u32, Rgba(color));
            }
        }
    }

    fn no_align() -> DiffOptions {
        DiffOptions {
            max_shift: 0,
            ..Default::default()
        }
    }

    #[test]
    fn identical_images_have_no_changes() {
        let a = page(200, 120);
        let d = diff(&a, &a.clone(), &DiffOptions::default());
        assert_eq!((d.offset_x, d.offset_y), (0, 0));
        assert!(d.boxes.is_empty());
        assert_eq!(d.changed_percent, 0.0);
        assert!(d.same_size);
    }

    #[test]
    fn finds_separate_changes_as_separate_boxes() {
        let a = page(200, 120);
        let mut b = a.clone();
        paint(&mut b, Rect::new(20, 20, 30, 10), [255, 0, 0, 255]);
        paint(&mut b, Rect::new(150, 90, 8, 8), [0, 0, 0, 255]);
        let d = diff(&a, &b, &no_align());
        assert_eq!(d.boxes.len(), 2, "{:?}", d.boxes);
        // cell-aligned box covering the bigger change
        assert_eq!(d.boxes[0], Rect::new(16, 16, 48, 16));
        assert!(d.boxes[1].contains(150, 90) && d.boxes[1].contains(157, 97));
    }

    #[test]
    fn small_noise_is_ignored() {
        let a = page(64, 64);
        let mut b = a.clone();
        let p = b.get_pixel(10, 10).0;
        b.put_pixel(10, 10, Rgba([p[0].saturating_add(10), p[1], p[2], 255])); // under tolerance
        b.put_pixel(40, 40, Rgba([0, 0, 0, 255])); // one pixel < min_pixels
        assert!(diff(&a, &b, &no_align()).boxes.is_empty());
    }

    #[test]
    fn alignment_undoes_a_shifted_capture() {
        let full = page(260, 180);
        let a = image::imageops::crop_imm(&full, 20, 20, 200, 120).to_image();
        let b = image::imageops::crop_imm(&full, 14, 23, 200, 120).to_image();
        // content at A(x, y) = full(x+20, y+20) sits at B(x+6, y-3)
        let d = diff(&a, &b, &DiffOptions::default());
        assert_eq!((d.offset_x, d.offset_y), (6, -3));
        assert!(d.boxes.is_empty(), "{:?}", d.boxes);
    }

    #[test]
    fn alignment_survives_repeating_text_rows() {
        // evenly spaced text lines are where sparse sampling locks onto the wrong row
        let text = |shift: u32| {
            RgbaImage::from_fn(640, 400, move |x, y| {
                let (x, y) = (x + shift, y + shift / 2);
                let base = [245u8, 232, 210][((x / 300 + y / 200) % 3) as usize];
                if y % 24 < 6 && (x + (y / 24) * 17) % 70 < 40 {
                    Rgba([30, 40, 60, 255])
                } else {
                    Rgba([base, base, base, 255])
                }
            })
        };
        let d = diff(&text(0), &text(10), &DiffOptions::default());
        assert_eq!((d.offset_x, d.offset_y), (-10, -5));
        assert!(d.boxes.is_empty(), "{:?}", d.boxes);
    }

    #[test]
    fn different_sizes_compare_the_overlap() {
        let a = page(100, 80);
        let b = page(120, 80);
        let d = diff(&a, &b, &no_align());
        assert!(!d.same_size);
        assert!(d.boxes.is_empty());
    }
}
