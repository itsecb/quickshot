use serde::{Deserialize, Serialize};

/// Axis-aligned rectangle in global physical pixels.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.right() && py < self.bottom()
    }

    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r > x && b > y {
            Some(Rect::new(x, y, (r - x) as u32, (b - y) as u32))
        } else {
            None
        }
    }

    /// Bounding box of all given rects.
    pub fn union_all<'a>(rects: impl IntoIterator<Item = &'a Rect>) -> Option<Rect> {
        let mut it = rects.into_iter();
        let first = *it.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.right(), first.bottom());
        for r in it {
            x0 = x0.min(r.x);
            y0 = y0.min(r.y);
            x1 = x1.max(r.right());
            y1 = y1.max(r.bottom());
        }
        Some(Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_overlapping() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        assert_eq!(a.intersect(&b), Some(Rect::new(50, 50, 50, 50)));
    }

    #[test]
    fn intersect_disjoint_and_touching() {
        let a = Rect::new(0, 0, 100, 100);
        assert_eq!(a.intersect(&Rect::new(100, 0, 10, 10)), None);
        assert_eq!(a.intersect(&Rect::new(200, 200, 10, 10)), None);
    }

    #[test]
    fn intersect_negative_origin() {
        let a = Rect::new(-1920, 0, 1920, 1080);
        let b = Rect::new(-100, -100, 200, 200);
        assert_eq!(a.intersect(&b), Some(Rect::new(-100, 0, 100, 100)));
    }

    #[test]
    fn union() {
        let rects = [Rect::new(-1920, 0, 1920, 1080), Rect::new(0, 0, 2560, 1440)];
        assert_eq!(
            Rect::union_all(&rects),
            Some(Rect::new(-1920, 0, 4480, 1440))
        );
    }
}
