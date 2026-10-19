#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A half-open rectangle: width = right - left, height = bottom - top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn from_points(a: Point, b: Point) -> Self {
        Self::new(a.x, a.y, b.x, b.y).normalized()
    }

    pub fn width(self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }

    pub fn is_empty(self) -> bool {
        self.width() <= 0 || self.height() <= 0
    }

    pub fn normalized(mut self) -> Self {
        if self.left > self.right {
            std::mem::swap(&mut self.left, &mut self.right);
        }
        if self.top > self.bottom {
            std::mem::swap(&mut self.top, &mut self.bottom);
        }
        self
    }

    pub fn contains(self, point: Point) -> bool {
        point.x >= self.left && point.x < self.right && point.y >= self.top && point.y < self.bottom
    }

    pub fn intersects(self, other: Rect) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.top < other.bottom
            && other.top < self.bottom
    }

    pub fn intersection(self, other: Rect) -> Rect {
        if !self.intersects(other) {
            return Rect::default();
        }
        Rect::new(
            self.left.max(other.left),
            self.top.max(other.top),
            self.right.min(other.right),
            self.bottom.min(other.bottom),
        )
    }

    pub fn union(self, other: Rect) -> Rect {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        Rect::new(
            self.left.min(other.left),
            self.top.min(other.top),
            self.right.max(other.right),
            self.bottom.max(other.bottom),
        )
    }

    pub fn clamp_to(self, bounds: Rect) -> Rect {
        if self.is_empty() {
            return self;
        }
        Rect::new(
            self.left.clamp(bounds.left, bounds.right),
            self.top.clamp(bounds.top, bounds.bottom),
            self.right.clamp(bounds.left, bounds.right),
            self.bottom.clamp(bounds.top, bounds.bottom),
        )
        .normalized()
    }

    pub fn center(self) -> Point {
        Point::new(self.left + self.width() / 2, self.top + self.height() / 2)
    }

    pub fn nearest_distance_sq(self, point: Point) -> i64 {
        let nx = point.x.clamp(self.left, self.right) as i64;
        let ny = point.y.clamp(self.top, self.bottom) as i64;
        let dx = point.x as i64 - nx;
        let dy = point.y as i64 - ny;
        dx * dx + dy * dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_and_measures() {
        let r = Rect::from_points(Point::new(10, 20), Point::new(2, 5));
        assert_eq!(r, Rect::new(2, 5, 10, 20));
        assert_eq!(r.width(), 8);
        assert_eq!(r.height(), 15);
        assert!(!r.is_empty());
        assert!(r.contains(Point::new(2, 5)));
        assert!(!r.contains(Point::new(10, 20)));
    }

    #[test]
    fn intersects_and_unions() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 15, 15);
        assert!(a.intersects(b));
        assert_eq!(a.intersection(b), Rect::new(5, 5, 10, 10));
        assert_eq!(a.union(b), Rect::new(0, 0, 15, 15));
        assert!(!a.intersects(Rect::new(20, 20, 30, 30)));
        assert_eq!(a.intersection(Rect::new(20, 20, 30, 30)), Rect::default());
    }

    #[test]
    fn clamps_to_bounds() {
        let bounds = Rect::new(-100, -100, 100, 100);
        assert_eq!(
            Rect::new(-200, -200, 50, 50).clamp_to(bounds),
            Rect::new(-100, -100, 50, 50)
        );
        assert_eq!(
            Rect::new(200, 200, 300, 300).clamp_to(bounds),
            Rect::new(100, 100, 100, 100)
        );
    }

    #[test]
    fn nearest_distance_handles_gaps() {
        let r = Rect::new(0, 0, 10, 10);
        assert_eq!(r.nearest_distance_sq(Point::new(5, 5)), 0);
        assert_eq!(r.nearest_distance_sq(Point::new(13, 14)), 25);
        assert_eq!(r.nearest_distance_sq(Point::new(-3, 4)), 9);
    }
}
