//! Screen geometry shared by every platform layer.

use serde::{Deserialize, Serialize};

/// A screen rectangle in logical points, origin at the top-left of the primary
/// display (the coordinate system macOS Accessibility reports in; the Windows
/// layer converts UI Automation's physical pixels to it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x1 = self.x.max(o.x);
        let y1 = self.y.max(o.y);
        let x2 = (self.x + self.w).min(o.x + o.w);
        let y2 = (self.y + self.h).min(o.y + o.h);
        if x2 > x1 && y2 > y1 {
            Some(Rect { x: x1, y: y1, w: x2 - x1, h: y2 - y1 })
        } else {
            None
        }
    }
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::Rect;

    #[test]
    fn intersect_contains_and_empty() {
        let a = Rect { x: 0.0, y: 0.0, w: 100.0, h: 50.0 };
        let b = Rect { x: 80.0, y: 40.0, w: 100.0, h: 100.0 };
        assert_eq!(a.intersect(&b), Some(Rect { x: 80.0, y: 40.0, w: 20.0, h: 10.0 }));
        assert_eq!(a.intersect(&Rect { x: 200.0, y: 0.0, w: 5.0, h: 5.0 }), None);
        assert!(a.contains(100.0, 50.0));
        assert!(!a.contains(100.1, 10.0));
        assert!(Rect { x: 1.0, y: 1.0, w: 0.0, h: 3.0 }.is_empty());
    }
}
