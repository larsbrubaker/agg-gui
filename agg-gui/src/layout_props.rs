//! Layout property types: [`Insets`], [`HAnchor`], [`VAnchor`], [`WidgetBase`].
//!
//! These types mirror the C# agg-sharp `BorderDouble`, `HAnchor`, `VAnchor`,
//! and the per-widget layout fields that every `GuiWidget` carried.
//!
//! # Design
//!
//! Every concrete widget embeds a [`WidgetBase`] and delegates the five
//! layout-property getters on the [`Widget`](crate::widget::Widget) trait to
//! it.  The parent layout container reads those getters when placing children.
//!
//! All values are stored in **logical (device-independent) units**.
//! [`WidgetBase::scaled_margin`] multiplies by the global
//! [`device_scale`](crate::device_scale::device_scale) factor to produce
//! physical pixel values for use inside layout algorithms.
//!
//! # Margin vs padding
//!
//! - **Margin** lives on the child and is read by the parent during layout.
//!   It is space *outside* the widget's bounds.
//! - **Padding** is the parent container's internal inset — space between its
//!   own border and its children.  Containers store padding directly (e.g.
//!   `FlexColumn::inner_padding`); individual leaf widgets do not have padding.
//!
//! # Margin semantics
//!
//! Margins are **additive**, not collapsed.  When child A has
//! `margin.bottom = 4` and child B has `margin.top = 6`, the gap between them
//! is `gap + 4 + 6 = 10 + gap`, not `max(4, 6) = 6`.  This matches the
//! original C# agg-sharp behaviour.

use crate::geometry::Size;

// ---------------------------------------------------------------------------
// Insets
// ---------------------------------------------------------------------------

/// Per-side inset values (logical units).
///
/// Used for both widget **margin** (space outside the widget) and container
/// **padding** (space inside the container around its children).
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub struct Insets {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

impl Insets {
    /// All sides zero.
    pub const ZERO: Self = Self {
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
    };

    /// All four sides the same value.
    pub fn all(v: f64) -> Self {
        Self {
            left: v,
            right: v,
            top: v,
            bottom: v,
        }
    }

    /// Horizontal sides (`left` / `right`) = `h`, vertical (`top` / `bottom`) = `v`.
    pub fn symmetric(h: f64, v: f64) -> Self {
        Self {
            left: h,
            right: h,
            top: v,
            bottom: v,
        }
    }

    /// Explicit per-side constructor.
    pub fn from_sides(left: f64, right: f64, top: f64, bottom: f64) -> Self {
        Self {
            left,
            right,
            top,
            bottom,
        }
    }

    /// Sum of `left + right`.
    #[inline]
    pub fn horizontal(&self) -> f64 {
        self.left + self.right
    }

    /// Sum of `top + bottom`.
    #[inline]
    pub fn vertical(&self) -> f64 {
        self.top + self.bottom
    }

    /// Return a new `Insets` with all sides multiplied by `factor`.
    #[inline]
    pub fn scale(self, factor: f64) -> Self {
        Self {
            left: self.left * factor,
            right: self.right * factor,
            top: self.top * factor,
            bottom: self.bottom * factor,
        }
    }
}

// ---------------------------------------------------------------------------
// HAnchor
// ---------------------------------------------------------------------------

/// Horizontal anchor flags — how a widget sizes and positions itself
/// horizontally within the slot assigned by its parent.
///
/// | Constant | Meaning |
/// |---|---|
/// | `ABSOLUTE` | No automatic sizing or positioning (manual bounds). |
/// | `LEFT` | Align to the left edge of the slot (respecting margin). |
/// | `CENTER` | Center horizontally in the slot (respecting margin). |
/// | `RIGHT` | Align to the right edge of the slot (respecting margin). |
/// | `FIT` | Width encloses natural content (default). |
/// | `STRETCH` | Fill the slot width (`LEFT \| RIGHT`). |
/// | `MAX_FIT_OR_STRETCH` | Take the larger of Fit or Stretch. |
/// | `MIN_FIT_OR_STRETCH` | Take the smaller of Fit or Stretch. |
///
/// At most one of `LEFT`, `CENTER`, `RIGHT` may be set for position anchoring;
/// combining `LEFT | RIGHT` means "stretch", not "anchor to both edges".
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HAnchor(u8);

impl HAnchor {
    pub const ABSOLUTE: Self = HAnchor(0);
    pub const LEFT: Self = HAnchor(1);
    pub const CENTER: Self = HAnchor(2);
    pub const RIGHT: Self = HAnchor(4);
    /// Width fits natural content size (default).
    pub const FIT: Self = HAnchor(8);
    /// Fill parent slot width (`LEFT | RIGHT`).
    pub const STRETCH: Self = HAnchor(5); // 1 | 4
    /// Take the larger of Fit or Stretch.
    pub const MAX_FIT_OR_STRETCH: Self = HAnchor(13); // 8 | 5
    /// Take the smaller of Fit or Stretch.
    pub const MIN_FIT_OR_STRETCH: Self = HAnchor(16);

    /// Returns `true` if all bits in `flags` are set in `self`.
    #[inline]
    pub fn contains(self, flags: Self) -> bool {
        flags.0 != 0 && (self.0 & flags.0) == flags.0
    }

    /// Returns `true` if this anchor causes horizontal stretching
    /// (both LEFT and RIGHT are set, or MIN/MAX_FIT_OR_STRETCH resolves to stretch).
    #[inline]
    pub fn is_stretch(self) -> bool {
        self.contains(Self::LEFT) && self.contains(Self::RIGHT)
    }
}

impl Default for HAnchor {
    /// Default is [`FIT`](HAnchor::FIT): take natural content width.
    fn default() -> Self {
        Self::FIT
    }
}

impl std::ops::BitOr for HAnchor {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        HAnchor(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for HAnchor {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        HAnchor(self.0 & rhs.0)
    }
}

// ---------------------------------------------------------------------------
// VAnchor
// ---------------------------------------------------------------------------

/// Vertical anchor flags — how a widget sizes and positions itself vertically
/// within the slot assigned by its parent.
///
/// Mirrors [`HAnchor`] with `BOTTOM` / `TOP` instead of `LEFT` / `RIGHT`.
/// Y-up convention: `BOTTOM` is the visually lower edge (small Y), `TOP` is
/// the visually upper edge (large Y).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct VAnchor(u8);

impl VAnchor {
    pub const ABSOLUTE: Self = VAnchor(0);
    pub const BOTTOM: Self = VAnchor(1);
    pub const CENTER: Self = VAnchor(2);
    pub const TOP: Self = VAnchor(4);
    /// Height fits natural content size (default).
    pub const FIT: Self = VAnchor(8);
    /// Fill parent slot height (`BOTTOM | TOP`).
    pub const STRETCH: Self = VAnchor(5); // 1 | 4
    /// Take the larger of Fit or Stretch.
    pub const MAX_FIT_OR_STRETCH: Self = VAnchor(13); // 8 | 5
    /// Take the smaller of Fit or Stretch.
    pub const MIN_FIT_OR_STRETCH: Self = VAnchor(16);

    /// Returns `true` if all bits in `flags` are set in `self`.
    #[inline]
    pub fn contains(self, flags: Self) -> bool {
        flags.0 != 0 && (self.0 & flags.0) == flags.0
    }

    /// Returns `true` if this anchor causes vertical stretching.
    #[inline]
    pub fn is_stretch(self) -> bool {
        self.contains(Self::BOTTOM) && self.contains(Self::TOP)
    }
}

impl Default for VAnchor {
    /// Default is [`FIT`](VAnchor::FIT): take natural content height.
    fn default() -> Self {
        Self::FIT
    }
}

impl std::ops::BitOr for VAnchor {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        VAnchor(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for VAnchor {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        VAnchor(self.0 & rhs.0)
    }
}

// ---------------------------------------------------------------------------
// WidgetBase
// ---------------------------------------------------------------------------

/// Stores the five universal layout properties that every widget carries.
///
/// Embed in every concrete widget and delegate the five
/// [`Widget`](crate::widget::Widget) layout-property getters to the
/// corresponding fields.  The builder methods return `Self` so they can be
/// chained on the concrete type.
///
/// ```rust,ignore
/// pub struct MyWidget {
///     bounds:   Rect,
///     children: Vec<Box<dyn Widget>>,
///     base:     WidgetBase,
///     // ...widget-specific fields...
/// }
///
/// impl Widget for MyWidget {
///     fn margin(&self)   -> Insets  { self.base.margin }
///     fn h_anchor(&self) -> HAnchor { self.base.h_anchor }
///     fn v_anchor(&self) -> VAnchor { self.base.v_anchor }
///     fn min_size(&self) -> Size    { self.base.min_size }
///     fn max_size(&self) -> Size    { self.base.max_size }
///     // ...
/// }
///
/// impl MyWidget {
///     pub fn with_margin(mut self, m: Insets)    -> Self { self.base.margin   = m; self }
///     pub fn with_h_anchor(mut self, h: HAnchor) -> Self { self.base.h_anchor = h; self }
///     pub fn with_v_anchor(mut self, v: VAnchor) -> Self { self.base.v_anchor = v; self }
///     pub fn with_min_size(mut self, s: Size)    -> Self { self.base.min_size = s; self }
///     pub fn with_max_size(mut self, s: Size)    -> Self { self.base.max_size = s; self }
/// }
/// ```
#[derive(Copy, Clone, Debug)]
pub struct WidgetBase {
    /// Space outside this widget's bounds (read by the parent during layout).
    pub margin: Insets,
    /// Horizontal anchor — how this widget positions/sizes itself horizontally.
    pub h_anchor: HAnchor,
    /// Vertical anchor — how this widget positions/sizes itself vertically.
    pub v_anchor: VAnchor,
    /// Minimum size constraint (logical units).  The parent will never assign
    /// a slot smaller than this in either axis.
    pub min_size: Size,
    /// Maximum size constraint (logical units).  The parent will never assign
    /// a slot larger than this in either axis.
    pub max_size: Size,
    /// Per-widget override of the global pixel-alignment policy.  When
    /// `true` (the common default) `paint_subtree` rounds the child
    /// translation to the physical pixel grid before painting, so crisp text
    /// and strokes land on whole pixels regardless of fractional Label
    /// heights (`font_size × 1.5`) accumulating through a flex stack.
    /// Disable for widgets that deliberately want sub-pixel positioning
    /// (smooth-scrolling markers, zoomed canvases).
    ///
    /// Mirrors MatterCAD's `GuiWidget.EnforceIntegerBounds`.  Captured from
    /// [`pixel_bounds::default_enforce_integer_bounds`] at construction;
    /// later global changes do NOT retroactively alter existing widgets.
    pub enforce_integer_bounds: bool,
}

impl WidgetBase {
    /// Construct a `WidgetBase` with all defaults:
    /// zero margin, `FIT` anchors, `ZERO` min size, `Size::MAX` max size.
    /// `enforce_integer_bounds` captures the current process-wide default.
    pub fn new() -> Self {
        Self {
            margin: Insets::ZERO,
            h_anchor: HAnchor::FIT,
            v_anchor: VAnchor::FIT,
            min_size: Size::ZERO,
            max_size: Size::MAX,
            enforce_integer_bounds: crate::pixel_bounds::default_enforce_integer_bounds(),
        }
    }

    // ----- consuming builder methods ----------------------------------------

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.max_size = s;
        self
    }

    // ----- helpers ----------------------------------------------------------

    /// Clamp `proposed` to `[min_size, max_size]`.
    #[inline]
    pub fn clamp_size(&self, proposed: Size) -> Size {
        Size::new(
            proposed
                .width
                .clamp(self.min_size.width, self.max_size.width),
            proposed
                .height
                .clamp(self.min_size.height, self.max_size.height),
        )
    }

    /// Return [`margin`](Self::margin) in logical units.
    ///
    /// Previously multiplied by [`device_scale`](crate::device_scale::device_scale)
    /// when margin handling was spread across widgets.  DPI scaling is now
    /// applied once at the [`App`](crate::widget::App) boundary via a paint-
    /// ctx transform, so widgets work in logical units end-to-end and this
    /// helper is a simple passthrough kept for call-site readability.
    pub fn scaled_margin(&self) -> Insets {
        self.margin
    }
}

impl Default for WidgetBase {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helper: resolve MIN/MAX_FIT_OR_STRETCH
// ---------------------------------------------------------------------------

/// Given a natural (fit) size and a stretch (fill) size for one axis, resolve
/// the `MIN_FIT_OR_STRETCH` or `MAX_FIT_OR_STRETCH` anchor to a concrete size.
///
/// Used by layout containers when a child has one of the composite anchors.
#[inline]
pub fn resolve_fit_or_stretch(fit_size: f64, stretch_size: f64, max_mode: bool) -> f64 {
    if max_mode {
        fit_size.max(stretch_size)
    } else {
        fit_size.min(stretch_size)
    }
}
