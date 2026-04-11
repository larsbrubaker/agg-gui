//! Basic 2D geometry types.
//!
//! All coordinates are in first-quadrant (Y-up) space unless otherwise noted.
//! Origin is bottom-left. Positive Y goes upward.

/// A 2D point in first-quadrant (Y-up) coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A 2D size (width × height), always non-negative.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const ZERO: Self = Self { width: 0.0, height: 0.0 };

    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle in first-quadrant (Y-up) coordinates.
///
/// `(x, y)` is the bottom-left corner. Width and height are positive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn left(&self) -> f64 { self.x }
    pub fn bottom(&self) -> f64 { self.y }
    pub fn right(&self) -> f64 { self.x + self.width }
    pub fn top(&self) -> f64 { self.y + self.height }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.top()
    }
}
