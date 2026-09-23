//! Scrolling capture: stitch viewport-sized frames taken while a page scrolls into one tall
//! image. Handles a fixed header/footer (identical rows at the same place in every frame), and
//! reports when the page stopped moving (the end). Pure logic, unit-tested.

use image::RgbaImage;

/// Per-row fingerprint: a hash of the row's pixels, and whether the row has any detail at all
/// (blank rows match anywhere, so they don't count as evidence when aligning).
#[derive(Clone, Copy, PartialEq, Eq)]
struct Row {
    hash: u64,
    detailed: bool,
}

fn rows(img: &RgbaImage) -> Vec<Row> {
    let stride = img.width() as usize * 4;
    img.as_raw()
        .chunks(stride.max(1))
        .map(|row| {
            // FNV-1a over the row's bytes
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for &b in row {
                h ^= b as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
            let first = &row[..4.min(row.len())];
            let detailed = row.chunks(4).any(|px| px != first);
            Row { hash: h, detailed }
        })
        .collect()
}

/// Fixed rows at the top and bottom: identical, at the same position, in both frames.
/// Capped so a mostly blank page can't be mistaken for one big header.
pub fn static_bands(a: &RgbaImage, b: &RgbaImage) -> (u32, u32) {
    let (ra, rb) = (rows(a), rows(b));
    let n = ra.len().min(rb.len());
    let header = ra.iter().zip(&rb).take_while(|(x, y)| x == y).count();
    if header >= n {
        return (n as u32, 0); // identical frames: nothing moved
    }
    let footer = ra
        .iter()
        .rev()
        .zip(rb.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let cap = n * 2 / 5;
    (header.min(cap) as u32, footer.min(cap) as u32)
}

/// How far the content moved up between `prev` and `next` (row r of `next` shows row r + d of
/// `prev`), looking only between the fixed header and footer. `None` if it didn't move or no
/// offset explains the frames well (e.g. the page changed instead of scrolling).
pub fn find_scroll(prev: &RgbaImage, next: &RgbaImage, header: u32, footer: u32) -> Option<u32> {
    let (rp, rn) = (rows(prev), rows(next));
    let h = rp.len().min(rn.len());
    let (top, bottom) = (header as usize, h.saturating_sub(footer as usize));
    let body = bottom.saturating_sub(top);
    if body < 16 {
        return None;
    }
    let min_overlap = (body / 5).max(8);
    let mut best: Option<(f64, usize)> = None;
    for d in 1..body.saturating_sub(min_overlap) {
        let (mut detailed, mut matched) = (0usize, 0usize);
        for r in top..bottom - d {
            let (n, p) = (rn[r], rp[r + d]);
            if n.detailed || p.detailed {
                detailed += 1;
                matched += (n.hash == p.hash) as usize;
            }
        }
        if detailed < 4 {
            continue;
        }
        let score = matched as f64 / detailed as f64;
        if best.is_none_or(|(s, _)| score > s + 1e-9) {
            best = Some((score, d));
        }
    }
    best.filter(|(score, _)| *score >= 0.9)
        .map(|(_, d)| d as u32)
}

/// Accumulates frames while scrolling; `finish` builds the tall image.
pub struct Stitcher {
    frames: Vec<RgbaImage>,
    offsets: Vec<u32>,
    header: u32,
    footer: u32,
}

impl Stitcher {
    pub fn new(first: RgbaImage) -> Self {
        Self {
            frames: vec![first],
            offsets: Vec::new(),
            header: 0,
            footer: 0,
        }
    }

    /// Add the frame after a scroll step. False when the page didn't move (the end, or it
    /// isn't scrollable) — the frame is dropped and capturing should stop.
    pub fn push(&mut self, next: RgbaImage) -> bool {
        let prev = self.frames.last().expect("stitcher starts with a frame");
        if next.dimensions() != prev.dimensions() {
            return false;
        }
        if self.frames.len() == 1 {
            let (header, footer) = static_bands(prev, &next);
            if header >= prev.height() {
                return false;
            }
            self.header = header;
            self.footer = footer;
        }
        match find_scroll(prev, &next, self.header, self.footer) {
            Some(d) if d > 0 => {
                self.offsets.push(d);
                self.frames.push(next);
                true
            }
            _ => false,
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Height of the image `finish` would produce.
    pub fn height(&self) -> u32 {
        self.frames[0].height() + self.offsets.iter().sum::<u32>()
    }

    /// Header (from the first frame), every new body row in order, footer (from the last frame).
    pub fn finish(self) -> RgbaImage {
        let first = &self.frames[0];
        let (w, fh) = first.dimensions();
        let stride = w as usize * 4;
        let body_bottom = (fh - self.footer) as usize;
        let mut out: Vec<u8> = Vec::with_capacity(stride * self.height() as usize);
        // everything of the first frame above its footer
        out.extend_from_slice(&first.as_raw()[..body_bottom * stride]);
        // each later frame contributes the rows that scrolled into view just above its footer
        for (frame, &d) in self.frames[1..].iter().zip(&self.offsets) {
            let from = body_bottom - d as usize;
            out.extend_from_slice(&frame.as_raw()[from * stride..body_bottom * stride]);
        }
        let last = self.frames.last().expect("at least one frame");
        out.extend_from_slice(&last.as_raw()[body_bottom * stride..]);
        let h = (out.len() / stride.max(1)) as u32;
        RgbaImage::from_raw(w, h, out).expect("rows add up")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// A tall "page" with unique, detailed rows (so any alignment is unambiguous) and some
    /// blank bands like real documents have.
    fn page(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            if (y / 40) % 5 == 4 {
                return Rgba([255, 255, 255, 255]); // blank paragraph gap
            }
            let v = (y.wrapping_mul(2654435761) >> 7) as u8;
            Rgba([v, ((x * 3 + y) % 251) as u8, (x ^ y) as u8, 255])
        })
    }

    /// What the screen shows with the page scrolled to `pos`: optional fixed header/footer.
    fn viewport(page: &RgbaImage, pos: u32, h: u32, header: u32, footer: u32) -> RgbaImage {
        let w = page.width();
        RgbaImage::from_fn(w, h, |x, y| {
            if y < header {
                Rgba([30, 60, 120, 255])
            } else if y >= h - footer {
                Rgba([200, 200, 60, 255])
            } else {
                *page.get_pixel(x, pos + (y - header))
            }
        })
    }

    fn scroll_through(header: u32, footer: u32, step: u32) {
        let (w, vh) = (120, 300);
        let body = vh - header - footer;
        let p = page(w, 1400);
        let max_pos = p.height() - body;
        let mut s = Stitcher::new(viewport(&p, 0, vh, header, footer));
        let mut pos = 0;
        loop {
            let next_pos = (pos + step).min(max_pos);
            let moved = s.push(viewport(&p, next_pos, vh, header, footer));
            if !moved {
                assert_eq!(pos, max_pos, "stopped early at {pos}");
                break;
            }
            pos = next_pos;
        }
        let out = s.finish();
        assert_eq!(out.height(), header + p.height() + footer);
        // header, then the whole page, then the footer
        if header > 0 {
            assert_eq!(out.get_pixel(5, 0).0, [30, 60, 120, 255]);
        }
        for y in [0, 1, 299, 777, 1399] {
            assert_eq!(
                out.get_pixel(7, header + y),
                p.get_pixel(7, y),
                "page row {y}"
            );
        }
        let last = out.get_pixel(5, out.height() - 1).0;
        if footer > 0 {
            assert_eq!(last, [200, 200, 60, 255]);
        } else {
            assert_eq!(last, p.get_pixel(5, 1399).0);
        }
    }

    #[test]
    fn stitches_a_plain_page() {
        scroll_through(0, 0, 170);
    }

    #[test]
    fn keeps_a_sticky_header_and_footer_once() {
        scroll_through(50, 30, 120);
    }

    #[test]
    fn small_steps_work_too() {
        scroll_through(40, 0, 45);
    }

    #[test]
    fn no_movement_means_the_end() {
        let p = page(80, 600);
        let v = viewport(&p, 0, 200, 0, 0);
        let mut s = Stitcher::new(v.clone());
        assert!(!s.push(v));
        assert_eq!(s.frame_count(), 1);
    }

    #[test]
    fn unrelated_frames_are_not_forced_together() {
        let a = page(80, 600);
        let b = RgbaImage::from_fn(80, 600, |x, y| Rgba([(x * y) as u8, 9, (y * 7) as u8, 255]));
        let mut s = Stitcher::new(viewport(&a, 0, 200, 0, 0));
        assert!(!s.push(viewport(&b, 0, 200, 0, 0)));
    }
}
