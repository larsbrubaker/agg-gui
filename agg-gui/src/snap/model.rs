//! Data model for the snap engine.
//!
//! Defines the [`Snappable`] trait that movable types implement, the
//! input enums that select snap behaviour ([`SnapMode`],
//! [`ResizeEdge`]), and the result types ([`SnapResult`],
//! [`SnapGuide`]).
//!
//! Kept free of widget / event dependencies so the engine can be
//! re-used from any drag handler (window manager, node graph,
//! diagram editor, etc.).

use crate::geometry::Rect;

/// Opaque identifier for a snappable rect.  Used by [`compute_snap`]
/// to skip self-matches when the moving rect is also present in the
/// target list.  The `u64` payload is opaque to the engine; callers
/// pick a scheme that makes their rects distinguishable — pointer
/// values cast to `u64`, monotonic ids, hash of a name, anything that
/// is unique per logical entity.
///
/// [`compute_snap`]: crate::snap::compute_snap
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnapId(pub u64);

/// What kind of drag operation produced the candidate rect.  Moving
/// snaps treat the whole rect; resize snaps only consider edges that
/// the active resize handle actually controls, so a resize from the
/// right edge can't snap the LEFT side of the moving rect to a
/// target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapMode {
    /// The whole rect is being translated — every edge is fair game
    /// for snapping.  Equal-spacing detection runs.
    Move,
    /// The rect is being resized via a specific edge / corner — only
    /// the affected edges may snap.  Equal-spacing detection is
    /// suppressed (it doesn't make geometric sense for a resize).
    Resize(ResizeEdge),
}

/// Which edge (or corner) is currently driving a resize.  Eight
/// compass directions cover the standard handle layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl ResizeEdge {
    /// Does this resize handle move the LEFT edge of the moving rect?
    pub fn affects_left(self) -> bool {
        matches!(
            self,
            ResizeEdge::West | ResizeEdge::NorthWest | ResizeEdge::SouthWest
        )
    }
    /// Does this resize handle move the RIGHT edge of the moving rect?
    pub fn affects_right(self) -> bool {
        matches!(
            self,
            ResizeEdge::East | ResizeEdge::NorthEast | ResizeEdge::SouthEast
        )
    }
    /// Does this resize handle move the TOP edge of the moving rect?
    /// "Top" is the higher Y in Y-up coords.
    pub fn affects_top(self) -> bool {
        matches!(
            self,
            ResizeEdge::North | ResizeEdge::NorthEast | ResizeEdge::NorthWest
        )
    }
    /// Does this resize handle move the BOTTOM edge of the moving rect?
    /// "Bottom" is the lower Y in Y-up coords.
    pub fn affects_bottom(self) -> bool {
        matches!(
            self,
            ResizeEdge::South | ResizeEdge::SouthEast | ResizeEdge::SouthWest
        )
    }
}

/// Trait implemented by anything that wants to participate in the
/// snap layout system.  Rust's equivalent of a C# interface: callers
/// take `&dyn Snappable` (or `&mut dyn Snappable`) and the engine
/// reads / writes the rect through these accessors without caring
/// about the underlying concrete type.
///
/// The two opt-in flags ([`is_snap_source`], [`is_snap_target`])
/// default to `true` — implementors only override when an instance
/// should sit out (e.g., a window is hidden / minimised and shouldn't
/// pull other windows toward an off-screen edge).
///
/// [`is_snap_source`]: Snappable::is_snap_source
/// [`is_snap_target`]: Snappable::is_snap_target
pub trait Snappable {
    /// Identity used to skip self-matches in the target list.
    fn snap_id(&self) -> SnapId;

    /// Current rect — both the source of the moving candidate and
    /// what the engine reads to build the target list for stationary
    /// neighbors.
    fn snap_rect(&self) -> Rect;

    /// Apply a snapped rect back to the implementor.  Called once per
    /// drag tick after [`compute_snap`] returns.  Default implementors
    /// just overwrite their internal bounds with `r`.
    ///
    /// [`compute_snap`]: crate::snap::compute_snap
    fn set_snap_rect(&mut self, r: Rect);

    /// `false` to opt this rect OUT of being the moving rect in a
    /// snap.  Rare — most movable things stay sources.
    fn is_snap_source(&self) -> bool {
        true
    }

    /// `false` to opt this rect OUT of being a snap target for other
    /// rects (the moving rect still won't snap to it).  Useful for
    /// transient overlays, hidden windows, etc.
    fn is_snap_target(&self) -> bool {
        true
    }
}

/// Output of [`compute_snap`].  The engine returns BOTH the corrected
/// rect (with any snap adjustments applied) AND the visual guides the
/// drag UI should render — alignment lines, equal-spacing dimension
/// markers, etc.  The dragger persists the rect; the guide overlay
/// reads `guides` and paints them on top.
///
/// [`compute_snap`]: crate::snap::compute_snap
#[derive(Clone, Debug, Default)]
pub struct SnapResult {
    /// Adjusted rect, ready to apply via [`Snappable::set_snap_rect`].
    /// When no snap fired the engine returns this unchanged from the
    /// input candidate.
    pub rect: Rect,
    /// Visual guides for the drag overlay to render.  Empty when no
    /// snap engaged.
    pub guides: Vec<SnapGuide>,
}

/// One visual guide.  Vertical / horizontal lines come from
/// edge-alignment snaps; spacing markers come from equal-gap
/// detection.  Coordinates are in the same space as the input rects
/// — typically the scene's root space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapGuide {
    /// Vertical alignment line at `x`, drawn from `y0` to `y1` to
    /// span the moving rect plus the target it snapped against.
    VLine { x: f64, y0: f64, y1: f64 },
    /// Horizontal alignment line at `y`, drawn from `x0` to `x1`.
    HLine { y: f64, x0: f64, x1: f64 },
    /// Horizontal spacing marker — the engine matched the moving
    /// rect's horizontal gap to an existing gap between two
    /// neighbors.  Drawn as a dimension line at vertical `y` between
    /// `x0`..`x1`.
    HSpacing { y: f64, x0: f64, x1: f64 },
    /// Vertical spacing marker — equal-gap match on the vertical
    /// axis.  Drawn as a dimension line at horizontal `x` between
    /// `y0`..`y1`.
    VSpacing { x: f64, y0: f64, y1: f64 },
}
