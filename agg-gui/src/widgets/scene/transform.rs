//! Pan/zoom transform math for the [`Scene`](super::Scene) container.
//!
//! A [`SceneTransform`] is a translation + uniform positive scale that maps
//! **scene coordinates** (the space child widgets are laid out in) to
//! **screen-local coordinates** (the Scene widget's own bottom-left-origin
//! Y-up pixel space).  Both spaces are Y-up — unlike egui, whose Scene is
//! Y-down — so a positive uniform scale needs no axis flips: a scene point
//! above another stays above it on screen.
//!
//! The mapping is `screen = zoom * scene + offset` (component-wise).  Keeping
//! the transform as an explicit `(zoom, offset)` pair rather than a full
//! affine matrix keeps the zoom-at-cursor and fit math trivial to reason
//! about and to unit-test.

use crate::geometry::{Point, Rect, Size};

/// Translation + uniform scale mapping scene space to Scene-local screen space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneTransform {
    /// Screen pixels per scene unit.  Always `> 0`.
    pub zoom: f64,
    /// Scene→screen translation, applied after scaling (Scene-local Y-up px).
    pub offset: Point,
}

impl Default for SceneTransform {
    fn default() -> Self {
        Self::identity()
    }
}

impl SceneTransform {
    pub fn new(zoom: f64, offset: Point) -> Self {
        Self { zoom, offset }
    }

    /// Zoom 1, no translation — scene coordinates equal screen coordinates.
    pub fn identity() -> Self {
        Self {
            zoom: 1.0,
            offset: Point::ORIGIN,
        }
    }

    /// Map a scene-space point to Scene-local screen space.
    pub fn scene_to_screen(&self, p: Point) -> Point {
        Point::new(self.zoom * p.x + self.offset.x, self.zoom * p.y + self.offset.y)
    }

    /// This transform as a [`crate::TransAffine`] mapping scene (child) space to
    /// Scene-local screen space, i.e. `screen = zoom * scene + offset`.
    ///
    /// This is what `Scene` returns from
    /// [`Widget::child_transform`](crate::widget::Widget::child_transform) so
    /// the framework's paint / hit-test / dispatch / inspector traversals all
    /// map coordinates through the same pan/zoom the content is drawn under.
    pub fn to_affine(&self) -> crate::TransAffine {
        let mut m = crate::TransAffine::new_scaling_uniform(self.zoom);
        m.translate(self.offset.x, self.offset.y);
        m
    }

    /// Map a Scene-local screen point back to scene space.
    pub fn screen_to_scene(&self, p: Point) -> Point {
        let z = self.zoom.max(1e-12);
        Point::new((p.x - self.offset.x) / z, (p.y - self.offset.y) / z)
    }

    /// The region of scene space currently visible in a container of `size`
    /// (the Scene widget's bounds), expressed in scene coordinates.
    ///
    /// This is what the caller reads back as the "scene rect".
    pub fn visible_scene_rect(&self, size: Size) -> Rect {
        let bl = self.screen_to_scene(Point::ORIGIN);
        let z = self.zoom.max(1e-12);
        Rect::new(bl.x, bl.y, size.width / z, size.height / z)
    }

    /// Translate the view by a screen-space delta (drag-to-pan).  Content
    /// follows the cursor: dragging right moves the scene content right.
    pub fn pan(&mut self, delta_screen: Point) {
        self.offset.x += delta_screen.x;
        self.offset.y += delta_screen.y;
    }

    /// Change the zoom to `new_zoom` (clamped to `range`) while keeping the
    /// scene point currently under `cursor_screen` fixed on screen.
    ///
    /// This is the invariant that makes wheel-zoom feel anchored to the
    /// cursor rather than to the origin.
    pub fn zoom_at(&mut self, cursor_screen: Point, new_zoom: f64, range: (f64, f64)) {
        let target = clamp_zoom(new_zoom, range);
        // Scene point under the cursor *before* the zoom change.
        let scene_pt = self.screen_to_scene(cursor_screen);
        self.zoom = target;
        // Solve `cursor = target * scene_pt + offset` for the new offset so
        // that same scene point still lands under the cursor.
        self.offset = Point::new(
            cursor_screen.x - target * scene_pt.x,
            cursor_screen.y - target * scene_pt.y,
        );
    }

    /// Build a transform that fits `content` (a rect in scene coords) fully
    /// inside a `container` of the given size, centered, with a uniform scale
    /// clamped to `range`.  Used for the initial view and for "reset view".
    pub fn fit(content: Rect, container: Size, range: (f64, f64)) -> Self {
        let cw = content.width.max(1e-9);
        let ch = content.height.max(1e-9);
        let raw = (container.width / cw).min(container.height / ch);
        let zoom = clamp_zoom(raw, range);
        // Center: map the content's center onto the container's center.
        let cc = Point::new(container.width * 0.5, container.height * 0.5);
        let sc = content.center();
        let offset = Point::new(cc.x - zoom * sc.x, cc.y - zoom * sc.y);
        Self { zoom, offset }
    }
}

/// Clamp `z` into `(min, max)`, tolerating a caller that passes the pair
/// reversed.
fn clamp_zoom(z: f64, range: (f64, f64)) -> f64 {
    let (lo, hi) = if range.0 <= range.1 {
        (range.0, range.1)
    } else {
        (range.1, range.0)
    };
    z.clamp(lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RANGE: (f64, f64) = (0.1, 2.0);

    fn approx(a: Point, b: Point) {
        assert!(
            (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9,
            "expected {:?} ≈ {:?}",
            a,
            b
        );
    }

    #[test]
    fn screen_scene_round_trip() {
        let t = SceneTransform::new(1.5, Point::new(10.0, 20.0));
        let p = Point::new(3.0, 4.0);
        let screen = t.scene_to_screen(p);
        approx(t.screen_to_scene(screen), p);
    }

    #[test]
    fn zoom_at_cursor_keeps_scene_point_fixed() {
        let mut t = SceneTransform::new(2.0, Point::new(5.0, 5.0));
        let cursor = Point::new(100.0, 80.0);
        let scene_before = t.screen_to_scene(cursor);
        t.zoom_at(cursor, 0.5, RANGE);
        let scene_after = t.screen_to_scene(cursor);
        approx(scene_after, scene_before);
        assert!((t.zoom - 0.5).abs() < 1e-12);
    }

    #[test]
    fn zoom_is_clamped_to_range() {
        let mut t = SceneTransform::identity();
        t.zoom_at(Point::new(50.0, 50.0), 10.0, RANGE);
        assert!((t.zoom - 2.0).abs() < 1e-12, "zoom should clamp to max");
        t.zoom_at(Point::new(50.0, 50.0), 0.0001, RANGE);
        assert!((t.zoom - 0.1).abs() < 1e-12, "zoom should clamp to min");
    }

    #[test]
    fn pan_shifts_visible_rect_inversely() {
        let mut t = SceneTransform::identity();
        let size = Size::new(200.0, 150.0);
        let before = t.visible_scene_rect(size);
        // Pan content +30 in x, -10 in y (screen delta). At zoom 1 the visible
        // scene rect's origin moves by the negative of that delta.
        t.pan(Point::new(30.0, -10.0));
        let after = t.visible_scene_rect(size);
        assert!((after.x - (before.x - 30.0)).abs() < 1e-9);
        assert!((after.y - (before.y + 10.0)).abs() < 1e-9);
    }

    #[test]
    fn identity_visible_rect_matches_container() {
        let t = SceneTransform::identity();
        let r = t.visible_scene_rect(Size::new(200.0, 150.0));
        assert_eq!(r, Rect::new(0.0, 0.0, 200.0, 150.0));
    }

    #[test]
    fn fit_centers_content_in_container() {
        let content = Rect::new(0.0, 0.0, 100.0, 50.0);
        let container = Size::new(200.0, 200.0);
        let t = SceneTransform::fit(content, container, RANGE);
        // min(200/100, 200/50) = min(2, 4) = 2, within range.
        assert!((t.zoom - 2.0).abs() < 1e-12);
        // Content center should land at the container center.
        approx(
            t.scene_to_screen(content.center()),
            Point::new(100.0, 100.0),
        );
    }

    #[test]
    fn to_affine_matches_scene_to_screen() {
        // The affine handed to the framework must reproduce `scene_to_screen`
        // exactly, and its inverse must reproduce `screen_to_scene` — that is
        // the contract the hit-test / dispatch traversals rely on.
        let t = SceneTransform::new(1.75, Point::new(12.0, -7.0));
        let m = t.to_affine();
        for p in [
            Point::new(0.0, 0.0),
            Point::new(3.0, 4.0),
            Point::new(-10.0, 25.0),
        ] {
            let (mut x, mut y) = (p.x, p.y);
            m.transform(&mut x, &mut y);
            let expect = t.scene_to_screen(p);
            approx(Point::new(x, y), expect);

            let (mut ix, mut iy) = (expect.x, expect.y);
            m.inverse_transform(&mut ix, &mut iy);
            approx(Point::new(ix, iy), p);
        }
    }

    #[test]
    fn fit_scale_clamped_to_range() {
        // A tiny content rect would need a huge scale to fill the container;
        // the fit must clamp to the zoom range's max.
        let content = Rect::new(0.0, 0.0, 1.0, 1.0);
        let t = SceneTransform::fit(content, Size::new(1000.0, 1000.0), RANGE);
        assert!((t.zoom - 2.0).abs() < 1e-12);
    }
}
