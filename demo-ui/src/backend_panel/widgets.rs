use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widget::paint_subtree;
use agg_gui::{
    Button, Color, DrawCtx, Event, EventResult, Font, HAnchor, Label, Rect, Size, Widget,
};

use super::{FrameHistory, RunMode};

// The "Mean CPU usage" label + frame-time sparkline that used to live
// here moved into `agg_gui::widgets::performance::PerformanceView` so
// other apps (Solitaire's debug menu pop-up) can render the same
// readout without duplicating the sparkline math.

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

// ── Run mode row ─────────────────────────────────────────────────────────────

/// Reactive / Continuous selector built from two real `Button` children
/// sharing a `Rc<Cell<RunMode>>`.  Each button uses
/// [`Button::with_subtle`] + [`Button::with_active_fn`] — the inactive
/// segment paints in muted theme colours and the selected segment flips
/// to the accent surface.  The label glyph cache lives inside each
/// Button's Label child, so font rendering stays backbuffered without
/// any manual `paint_subtree` calls in this widget.
pub(super) struct RunModeRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl RunModeRow {
    const BTN_W: f64 = 96.0;
    const BTN_H: f64 = 24.0;

    pub(super) fn new(font: Arc<Font>, run_mode: Rc<Cell<RunMode>>) -> Self {
        let segments: [(&'static str, RunMode); 2] = [
            ("Reactive", RunMode::Reactive),
            ("Continuous", RunMode::Continuous),
        ];
        let children: Vec<Box<dyn Widget>> = segments
            .iter()
            .map(|(label, this_mode)| {
                let mode_active = Rc::clone(&run_mode);
                let mode_click = Rc::clone(&run_mode);
                let this = *this_mode;
                let btn = Button::new(*label, Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_subtle()
                    .with_active_fn(move || mode_active.get() == this)
                    .on_click(move || {
                        if mode_click.get() != this {
                            mode_click.set(this);
                            agg_gui::animation::request_draw();
                        }
                    });
                Box::new(btn) as Box<dyn Widget>
            })
            .collect();
        Self {
            bounds: Rect::default(),
            children,
        }
    }
}

impl Widget for RunModeRow {
    fn type_name(&self) -> &'static str {
        "RunModeRow"
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
        let row_h = Self::BTN_H + 8.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, row_h);
        let gy = (row_h - Self::BTN_H) * 0.5;
        for (i, child) in self.children.iter_mut().enumerate() {
            child.layout(Size::new(Self::BTN_W, Self::BTN_H));
            child.set_bounds(Rect::new(
                12.0 + i as f64 * (Self::BTN_W + 4.0),
                gy,
                Self::BTN_W,
                Self::BTN_H,
            ));
        }
        Size::new(available.width, row_h)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Buttons paint themselves through the framework's tree walk.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
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

/// MSAA sample-count selector — a row of segmented buttons composed from
/// real `Button` children.  The set of segments is supplied at
/// construction (`(label, sample_count)` pairs) so the same widget powers
/// both the native shell's 5-way Off/2×/4×/8×/16× picker and the WASM
/// shell's 2-way Off/On picker (browser WebGL only exposes a boolean
/// `antialias` flag).  Each segment uses `with_subtle()` +
/// `with_active_fn()` so the selected sample count flips to the accent
/// surface and the others stay muted.
///
/// Exposed to other crate modules (the System window's Render tab uses
/// the same widget) via `pub(crate)`.
pub(crate) struct MsaaRow {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl MsaaRow {
    const BTN_H: f64 = 24.0;
    /// Cube-widget segments — these are SSAA pixel multipliers, not MSAA
    /// sample counts.  The cube widget renders into an offscreen texture
    /// at `sqrt(samples) × {w, h}` and downsamples to the surface, so
    /// `Off / 4× / 16×` work identically on every adapter (no spec-only
    /// `{1, 4}` MSAA limit, no per-format adapter feature dance).  Cell
    /// values: `1` = no AA, `4` = 2× linear, `16` = 4× linear.
    pub(crate) const CUBE_SEGMENTS: &'static [(&'static str, u8)] =
        &[("Off", 1), ("4×", 4), ("16×", 16)];

    pub(crate) fn new(
        font: Arc<Font>,
        samples: Rc<Cell<u8>>,
        segments: &'static [(&'static str, u8)],
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
        }
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

impl Widget for MsaaRow {
    fn type_name(&self) -> &'static str {
        "MsaaRow"
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
        let row_h = Self::BTN_H + 8.0;
        self.bounds = Rect::new(0.0, 0.0, available.width, row_h);
        let gy = (row_h - Self::BTN_H) * 0.5;
        let bw = self.btn_width();

        // Layout: 12 px gutter, "SSAA" label at child[0], 8 px gap, then
        // segment buttons at child[1..].
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

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {
        // Buttons paint themselves through the framework's tree walk.
    }

    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Run mode description label ────────────────────────────────────────────────

/// Dynamic label beneath the run-mode buttons.
/// Reactive: "Only running UI code when there are animations or input."
/// Continuous: "Repainting the UI each frame. FPS: X.X"
pub(super) struct RunModeDesc {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    run_mode: Rc<Cell<RunMode>>,
    history: Rc<RefCell<FrameHistory>>,
    label: Label,
}

impl RunModeDesc {
    pub(super) fn new(
        font: Arc<Font>,
        run_mode: Rc<Cell<RunMode>>,
        history: Rc<RefCell<FrameHistory>>,
    ) -> Self {
        let mut label = Label::new("", Arc::clone(&font))
            .with_font_size(10.0)
            .with_wrap(true);
        label.buffered = false;
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            run_mode,
            history,
            label,
        }
    }
}

impl Widget for RunModeDesc {
    fn type_name(&self) -> &'static str {
        "RunModeDesc"
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
        // Set the text first so wrapped height is measured correctly for the
        // worst-case (reactive) string, then layout once within the available
        // width minus the 12-px horizontal padding used at paint time.
        self.label
            .set_text("Only running UI code when there are animations or input.".to_owned());
        let inner_w = (available.width - 24.0).max(1.0);
        let s = self.label.layout(Size::new(inner_w, f64::MAX / 2.0));
        self.label
            .set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        let h = (s.height + 8.0).max(18.0);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let text = match self.run_mode.get() {
            RunMode::Reactive => {
                "Only running UI code when there are animations or input.".to_owned()
            }
            RunMode::Continuous => {
                let hist = self.history.borrow();
                let fps = if hist.mean_ms() < 0.001 {
                    0.0
                } else {
                    1000.0 / hist.mean_ms()
                };
                format!("Running continuously as fast as possible. FPS: {fps:.1}")
            }
        };
        self.label.set_text(text);
        self.label.set_color(v.text_dim);

        let lh = self.label.bounds().height;
        let ly = ((self.bounds.height - lh) * 0.5).max(2.0);

        ctx.save();
        ctx.translate(12.0, ly);
        agg_gui::widget::paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
