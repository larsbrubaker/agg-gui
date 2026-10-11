//! Basic 2D geometry types.
//!
//! All coordinates are in first-quadrant (Y-up) space unless otherwise noted.
//! Origin is bottom-left. Positive Y goes upward.

/// A 2D point in first-quadrant (Y-up) coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Feeds `value` to `state` so that values comparing equal hash equal: `-0.0`
/// is folded into `0.0` (they are `==`).  NaN never equals anything, so how
/// it hashes cannot break `Hash`'s contract.
pub(crate) fn hash_f64<H: std::hash::Hasher>(value: f64, state: &mut H) {
    let value = if value == 0.0 { 0.0 } else { value };
    state.write_u64(value.to_bits());
}

/// Equal points hash equal (`-0.0` and `0.0` alike), so a `Point` can key a
/// hash map or set.
impl std::hash::Hash for Point {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        hash_f64(self.x, state);
        hash_f64(self.y, state);
    }
}

impl Point {
    pub const ORIGIN: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A 2D size (width × height), always non-negative.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const ZERO: Self = Self {
        width: 0.0,
        height: 0.0,
    };

    /// A very large size used as the default `max_size` in [`WidgetBase`].
    ///
    /// Uses `f64::MAX / 2` rather than `f64::MAX` so that summing sizes with
    /// margins or gaps never overflows to infinity.
    pub const MAX: Self = Self {
        width: f64::MAX / 2.0,
        height: f64::MAX / 2.0,
    };

    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

/// An axis-aligned rectangle in first-quadrant (Y-up) coordinates.
///
/// `(x, y)` is the bottom-left corner. Width and height are positive.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn left(&self) -> f64 {
        self.x
    }
    pub fn bottom(&self) -> f64 {
        self.y
    }
    pub fn right(&self) -> f64 {
        self.x + self.width
    }
    pub fn top(&self) -> f64 {
        self.y + self.height
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.top()
    }
}
