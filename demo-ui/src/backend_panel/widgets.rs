use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, DrawCtx, Event, EventResult, Font, HAnchor, Label, Rect, Size, Widget,
};

// The "Mean CPU usage" label + frame-time sparkline, the Mode header, the
// Reactive/Continuous segmented selector, and the dynamic description label
// all moved into `agg_gui::widgets::performance::PerformanceView` so other
// apps (Solitaire's debug menu pop-up, AtomArtist's debug Performance
// window) render the same readout + selector without duplicating the
// sparkline math or the segmented-toggle composition.

// ── Screen size label ─────────────────────────────────────────────────────────

/// Displays the current screen dimensions.  Uses `buffered = false` because
/// the text changes on every resize event — direct rasterization is cheaper
/// than rebuilding the cache on each change.
pub(super) struct ScreenSizeLabel {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    screen_size: Rc<Cell<(u32, u32)>>,
}

impl ScreenSizeLabel {
    pub(super) fn new(font: Arc<Font>, screen_size: Rc<Cell<(u32, u32)>>) -> Self {
        let mut label = Label::new("0 × 0", font).with_font_size(11.0);
        label.buffered = false;
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(label)],
            screen_size,
        }
    }
}

impl Widget for ScreenSizeLabel {
    fn type_name(&self) -> &'static str {
        "ScreenSizeLabel"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, 18.0);
        if let Some(child) = self.children.first_mut() {
            let s = child.layout(Size::new(available.width, 18.0));
            let ly = (18.0 - s.height) * 0.5;
            child.set_bounds(Rect::new(12.0, ly, s.width, s.height));
        }
        Size::new(available.width, 18.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let (w, h) = self.screen_size.get();
        let text = format!("{w} \u{00d7} {h}");
        if let Some(child) = self.children.first_mut() {
            child.set_label_text(&text);
            child.set_label_color(v.text_dim);
        }
        // Label child paints itself via the framework's tree walk.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Toggle pill ──────────────────────────────────────────────────────────────

/// Sidebar button that toggles a bound `Rc<Cell<bool>>` on click — visually
/// matches the top-bar "Backend" button (solid rounded pill, accent-filled
/// when the cell is true, white label in the active state, dim hover fill
/// otherwise).  Used for the "System" and "Inspector" entries in the
/// Backend sidebar's "agg-gui windows" section so the sidebar's window
/// togglers share the same look as the rest of the app's chrome.
/// Sidebar pill that stretches to the full row width.  Wraps a real
/// `Button` child sized to fill via `with_min_size`; styling driven by
/// `with_subtle()` + `with_active_fn()` matches the rest of the demo's
/// segmented-toggle look.
pub(super) struct TogglePill {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl TogglePill {
    const H: f64 = 26.0;
    const SIDE_GUTTER: f64 = 12.0;

    pub(super) fn new(font: Arc<Font>, label_text: &'static str, show: Rc<Cell<bool>>) -> Self {
        let show_active = Rc::clone(&show);
        let show_click = show;
        let btn = Button::new(label_text, font)
            .with_font_size(12.0)
            .with_subtle()
            .with_h_anchor(HAnchor::STRETCH)
            .with_active_fn(move || show_active.get())
            .on_click(move || {
                show_click.set(!show_click.get());
                agg_gui::animation::request_draw();
            });
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(btn)],
        }
    }
}

impl Widget for TogglePill {
    fn type_name(&self) -> &'static str {
        "TogglePill"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let total_h = Self::H + 4.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, total_h);
        let pill_w = (available.width - Self::SIDE_GUTTER * 2.0).max(0.0);
        let child = &mut self.children[0];
        // Force the Button to fill the pill width by floor-clamping its
        // natural width via min_size — the standard composition route for
        // "stretch this widget to fill its row".
        child.layout(Size::new(pill_w, Self::H));
        child.set_bounds(Rect::new(Self::SIDE_GUTTER, 2.0, pill_w, Self::H));
        Size::new(available.width, total_h)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Button child paints itself via the framework's tree walk.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── MSAA row ─────────────────────────────────────────────────────────────────

/// SSAA pixel-multiplier selector — a row of segmented buttons composed
/// from real `Button` children.  The set of segments is supplied at
/// construction (`(label, samples)` pairs) so the same widget can power
/// any AA picker that writes to an `Rc<Cell<u8>>`.  Each segment uses
/// `with_subtle()` + `with_active_fn()` so the selected sample count
/// flips to the accent surface and the others stay muted.
pub(crate) struct SsaaRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    /// Shared selection cell.  `paint()` reads this each frame to compose
    /// the status caption — kept as a field so the status line updates
    /// live with the buttons' `with_active_fn`-driven highlight.
    samples: Rc<Cell<u8>>,
    /// Cube widget's current pixel size (physical, post-transform), written
    /// by the cube each paint and read here to surface the actual GPU
    /// backbuffer memory cost alongside the multiplier label.  `(0, 0)`
    /// until the cube has painted at least once.
    cube_pixel_size: Rc<Cell<(u32, u32)>>,
    /// Font used for the status caption below the buttons.  The leading
    /// "SSAA" label child carries its own clone; this is the one
    /// `paint()` uses for the direct `ctx.fill_text` of the caption.
    font: Arc<Font>,
}

impl SsaaRow {
    const BTN_H: f64 = 24.0;
    /// Height reserved for the status caption below the segment buttons —
    /// font_size 11 + a little leading.
    const STATUS_H: f64 = 18.0;
    const STATUS_FONT_SIZE: f64 = 11.0;
    /// Cube-widget segments — SSAA pixel multipliers.  The cube widget
    /// renders into an offscreen texture at `sqrt(samples) × {w, h}` and
    /// downsamples to the surface, so `Off / 4× / 9× / 16×` work
    /// identically on every adapter (no spec-only `{1, 4}` MSAA limit, no
    /// per-format adapter feature dance).  Cell values: `1` = no AA,
    /// `4` = 2× linear, `9` = 3× linear, `16` = 4× linear.
    pub(crate) const CUBE_SEGMENTS: &'static [(&'static str, u8)] =
        &[("Off", 1), ("4×", 4), ("9×", 9), ("16×", 16)];

    pub(crate) fn new(
        font: Arc<Font>,
        samples: Rc<Cell<u8>>,
        segments: &'static [(&'static str, u8)],
        cube_pixel_size: Rc<Cell<(u32, u32)>>,
    ) -> Self {
        // Leading "SSAA" label so the row reads as a labelled control set
        // instead of three free-floating buttons.  Treated as child[0] in
        // layout; the segment buttons follow at child[1..].
        let label_widget: Box<dyn Widget> =
            Box::new(Label::new("SSAA", Arc::clone(&font)).with_font_size(12.0));
        let mut children: Vec<Box<dyn Widget>> = vec![label_widget];
        children.extend(segments.iter().map(|(label, val)| {
            let samples_active = Rc::clone(&samples);
            let samples_click = Rc::clone(&samples);
            let this = *val;
            let btn = Button::new(*label, Arc::clone(&font))
                .with_font_size(12.0)
                .with_subtle()
                .with_active_fn(move || samples_active.get() == this)
                .on_click(move || {
                    if samples_click.get() != this {
                        samples_click.set(this);
                        agg_gui::animation::request_draw();
                    }
                });
            Box::new(btn) as Box<dyn Widget>
        }));
        Self {
            bounds: Rect::default(),
            children,
            samples,
            cube_pixel_size,
            font,
        }
    }

    /// Status caption shown directly under the segment buttons — explains
    /// the trade-off behind the selected sample count so the cost of the
    /// "more pixels = more memory" knob is visible at a glance.  Pure
    /// function of the cell values; safe to call every paint.
    ///
    /// Format: `<linear>× linear · <raw>× memory (<W> × <H> = <N.N> MB)`.
    /// At the `Off` setting the prefix becomes `"Off — "` and the
    /// multiplier is 1.  Memory is the framebuffer footprint a downstream
    /// 3-D widget would pay: RGBA8 colour + Depth32 = 8 bytes / pixel,
    /// scaled by the SSAA pixel multiplier (`raw`).
    fn status_caption(&self) -> String {
        let raw = self.samples.get().max(1) as u32;
        let linear = match raw {
            1 => 1,
            2..=5 => 2,
            6..=12 => 3,
            _ => 4,
        };
        let (w, h) = self.cube_pixel_size.get();
        let bytes = (w as u64) * (h as u64) * 8 * (raw as u64);
        let mb = bytes as f64 / (1024.0 * 1024.0);
        let prefix = if raw <= 1 {
            "Off".to_string()
        } else {
            format!("{linear}× linear · {raw}× memory")
        };
        if w == 0 || h == 0 {
            return prefix;
        }
        format!("{prefix}  ({w} × {h} = {mb:.1} MB)")
    }

    /// Per-segment button width — tighter for the 5-way native list,
    /// roomier for the 2-way Web list so each button can fit a longer
    /// label without truncating.
    ///
    /// `children[0]` is the leading "SSAA" label, so segment count is
    /// `children.len() - 1`.
    fn btn_width(&self) -> f64 {
        let segments = self.children.len().saturating_sub(1);
        if segments <= 2 {
            60.0
        } else {
            44.0
        }
    }
}

impl Widget for SsaaRow {
    fn type_name(&self) -> &'static str {
        "SsaaRow"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, b: Rect) {
        self.bounds = b;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let button_row_h = Self::BTN_H + 8.0;
        let row_h = button_row_h + Self::STATUS_H;
        self.bounds = Rect::new(0.0, 0.0, available.width, row_h);
        let gy = (button_row_h - Self::BTN_H) * 0.5;
        let bw = self.btn_width();

        // Layout: 12 px gutter, "SSAA" label at child[0], 8 px gap, then
        // segment buttons at child[1..].  Status caption is painted
        // directly in `paint()` in the strip from `button_row_h` to
        // `row_h` — no child widget needed because the text is a pure
        // function of the cell and changes every frame.
        const LABEL_W: f64 = 44.0;
        const LABEL_GAP: f64 = 8.0;
        if let Some(label) = self.children.first_mut() {
            label.layout(Size::new(LABEL_W, Self::BTN_H));
            label.set_bounds(Rect::new(12.0, gy, LABEL_W, Self::BTN_H));
        }
        let buttons_x = 12.0 + LABEL_W + LABEL_GAP;
        for (i, child) in self.children.iter_mut().enumerate().skip(1) {
            let btn_idx = (i - 1) as f64;
            child.layout(Size::new(bw, Self::BTN_H));
            child.set_bounds(Rect::new(
                buttons_x + btn_idx * (bw + 4.0),
                gy,
                bw,
                Self::BTN_H,
            ));
        }
        Size::new(available.width, row_h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        // Buttons paint themselves through the framework's tree walk.
        // Here we just add the status caption directly under the buttons.
        let button_row_h = Self::BTN_H + 8.0;
        let strip_h = Self::STATUS_H;
        let caption = self.status_caption();

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(Self::STATUS_FONT_SIZE);
        // Muted variant of theme text colour — caption is secondary info,
        // not a control label, so it should sit visually under the buttons.
        let base = ctx.visuals().text_color;
        ctx.set_fill_color(Color::rgba(base.r, base.g, base.b, 0.65));

        // Left-align under the leading "SSAA" label.  Use the same 12 px
        // gutter as the label / button row.
        let tx = 12.0;
        if let Some(m) = ctx.measure_text(&caption) {
            let ty = button_row_h + m.centered_baseline_y(strip_h);
            ctx.fill_text(&caption, tx, ty);
        }
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}
