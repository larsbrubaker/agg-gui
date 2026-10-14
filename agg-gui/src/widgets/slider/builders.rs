//! Configuration builders for [`super::Slider`]: the egui-parity value
//! options (logarithmic mapping, clamping, smart aim, integer, suffix),
//! orientation and handle shape, the trailing fill, the value cell and label
//! options, and the layout (margin / anchor / size) builders.
//!
//! Split out of `slider/mod.rs` to keep that file under the 800-line limit;
//! the fields they set are declared there and read by the mapping in
//! `mod.rs` and the painters in `paint.rs`.  The in-place counterpart of
//! [`Slider::with_trailing_fill`] lives here too ([`Slider::set_trailing_fill`]).

use super::*;

impl Slider {
    /// Enable/disable logarithmic value mapping. Great for spanning huge ranges
    /// (and, unlike naive log, tolerates ranges that include zero and infinity).
    pub fn with_logarithmic(mut self, logarithmic: bool) -> Self {
        self.spec.logarithmic = logarithmic;
        self
    }

    /// For logarithmic sliders including zero: the smallest positive value the
    /// user can select. Default `1e-6` (or `1` for integer sliders).
    pub fn with_smallest_positive(mut self, smallest_positive: f64) -> Self {
        self.spec.smallest_positive = smallest_positive;
        self
    }

    /// For logarithmic sliders whose high end is infinity: the largest finite
    /// value before the slider switches to `∞`. Default `∞`.
    pub fn with_largest_finite(mut self, largest_finite: f64) -> Self {
        self.spec.largest_finite = largest_finite;
        self
    }

    /// Choose when values are clamped to the range. Default [`SliderClamping::Always`].
    pub fn with_clamping(mut self, clamping: SliderClamping) -> Self {
        self.clamping = clamping;
        self
    }

    /// Turn smart aim (snap toward round values while dragging) on/off. Default on.
    pub fn with_smart_aim(mut self, smart_aim: bool) -> Self {
        self.smart_aim = smart_aim;
        self
    }

    /// Horizontal or vertical. Default horizontal.
    pub fn with_orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Change the handle shape (circle or aspect-ratio'd rectangle).
    pub fn with_handle_shape(mut self, shape: HandleShape) -> Self {
        self.handle_shape = shape;
        self
    }

    /// Make this an integer slider: values round to whole numbers, formatting
    /// uses no decimals, and logarithmic mapping treats `1` as the smallest
    /// positive value.
    pub fn with_integer(mut self, integer: bool) -> Self {
        self.integer = integer;
        if integer {
            self.spec.smallest_positive = 1.0;
        }
        self
    }

    /// Append a suffix to the value label (e.g. "°" or " m").
    pub fn with_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.suffix = suffix.into();
        self
    }

    /// Toggle the accent-colored trailing fill painted up to the handle.
    pub fn with_trailing_fill(mut self, trailing_fill: bool) -> Self {
        self.trailing_fill = trailing_fill;
        self
    }

    /// In-place [`Self::with_trailing_fill`] for a live slider (agg-sharp
    /// `SlideView.TrailingFill`); the change reaches the next paint.
    pub fn set_trailing_fill(&mut self, trailing_fill: bool) {
        if self.trailing_fill != trailing_fill {
            self.trailing_fill = trailing_fill;
            crate::animation::request_draw();
        }
    }

    /// Whether the accent-coloured trailing fill is painted.
    pub fn trailing_fill(&self) -> bool {
        self.trailing_fill
    }

    /// Bind this slider's value to an external `Rc<Cell<f64>>`.
    ///
    /// The cell becomes the source-of-truth: `layout()` reads it every
    /// frame so any other widget (or code path) that writes the cell
    /// will drive this slider live; drag interactions here write back
    /// to the cell too.  Pattern mirrors `ToggleSwitch::with_state_cell`.
    pub fn with_value_cell(mut self, cell: Rc<Cell<f64>>) -> Self {
        // Reversed-range/NaN tolerant — see the note in `new`.
        self.props.value = clamp_value_to_range(cell.get(), self.props.min, self.props.max);
        self.value_cell = Some(cell);
        self
    }
    pub fn with_show_value(mut self, show: bool) -> Self {
        self.props.show_value = show;
        self
    }

    /// Force a specific decimal count for the numeric value label.  When
    /// unset, the format falls back to a heuristic based on `step`.
    pub fn with_decimals(mut self, decimals: usize) -> Self {
        self.props.decimals = Some(decimals);
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
    pub fn with_min_size(mut self, s: Size) -> Self {
        self.base.min_size = s;
        self
    }
    pub fn with_max_size(mut self, s: Size) -> Self {
        self.base.max_size = s;
        self
    }
}
