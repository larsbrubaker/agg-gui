//! Backend panel — left-side collapsible panel shown when the "Backend" button
//! is active in the top bar.
//!
//! All text is rendered through `Label` children so that glyph rasterization
//! is cached to offscreen framebuffers (backbuffer path).  For the live FPS
//! display and screen-size label (which change every frame) the labels use
//! `buffered = false` since caching a value that changes every render cycle
//! adds overhead with no benefit.
//!
//! Contents mirror egui's backend panel:
//! - Renderer / backend info
//! - Screen size (live)
//! - Run mode (Reactive / Continuous)
//! - Frame rate sparkline + mean CPU usage label
//! - Inspector checkbox toggle
//! - "Reset all state" button

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Checkbox, Color, DrawCtx, Event, EventResult,
    FlexColumn, Font, Insets, Label, Rect, Separator,
    Size, SizedBox, Widget,
};
use agg_gui::widget::paint_subtree;
use agg_gui::widgets::button::Button;

// ── Run mode ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RunMode { Reactive, Continuous }

// ── Frame history (simple ring buffer) ────────────────────────────────────────

/// Rolling FPS / frame-time display — stores the last N frame times in ms.
pub struct FrameHistory {
    times: Vec<f32>,
    head:  usize,
    len:   usize,
}

impl FrameHistory {
    const CAP: usize = 60;

    pub fn new() -> Self {
        Self { times: vec![0.0; Self::CAP], head: 0, len: 0 }
    }

    pub fn push(&mut self, frame_ms: f32) {
        self.times[self.head] = frame_ms;
        self.head = (self.head + 1) % Self::CAP;
        if self.len < Self::CAP { self.len += 1; }
    }

    pub fn mean_ms(&self) -> f32 {
        if self.len == 0 { return 0.0; }
        self.times[..self.len].iter().sum::<f32>() / self.len as f32
    }

    #[allow(dead_code)]
    pub fn fps(&self) -> f32 {
        let m = self.mean_ms();
        if m < 0.001 { 0.0 } else { 1000.0 / m }
    }

    /// Samples as a slice from oldest to newest (for sparkline rendering).
    pub fn samples(&self) -> impl Iterator<Item = f32> + '_ {
        let cap = Self::CAP;
        (0..self.len).map(move |i| {
            let idx = (self.head + cap - self.len + i) % cap;
            self.times[idx]
        })
    }
}

// ── Sparkline widget ──────────────────────────────────────────────────────────

/// Renders a line chart of the last N frame times.  No text is drawn here —
/// the adjacent `FpsLabel` handles the textual display.
struct Sparkline {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    history:  Rc<RefCell<FrameHistory>>,
}

impl Widget for Sparkline {
    fn type_name(&self) -> &'static str { "Sparkline" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, 48.0);
        Size::new(available.width, 48.0)
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let hist = self.history.borrow();

        // Background.
        ctx.set_fill_color(v.track_bg);
        ctx.begin_path();
        ctx.rounded_rect(0.0, 0.0, w, h, 4.0);
        ctx.fill();

        if hist.len < 2 { return; }
        let samples: Vec<f32> = hist.samples().collect();
        let max_ms = samples.iter().cloned().fold(0.1_f32, f32::max).max(16.7);

        // Draw line chart.
        ctx.set_stroke_color(v.accent);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        let n = samples.len();
        for (i, &ms) in samples.iter().enumerate() {
            let x = i as f64 / (n - 1) as f64 * w;
            let y = (1.0 - ms as f64 / max_ms as f64) * (h - 4.0) + 2.0;
            if i == 0 { ctx.move_to(x, y); } else { ctx.line_to(x, y); }
        }
        ctx.stroke();

        // 16.7 ms reference line (60 fps target).
        let ref_y = (1.0 - 16.7 / max_ms as f64) * (h - 4.0) + 2.0;
        if ref_y >= 2.0 && ref_y <= h - 2.0 {
            ctx.set_stroke_color(Color::rgba(1.0, 0.6, 0.0, 0.7)); // orange 60fps reference line
            ctx.set_line_width(1.0);
            ctx.begin_path();
            ctx.move_to(0.0, ref_y);
            ctx.line_to(w,   ref_y);
            ctx.stroke();
        }
    }
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── FPS label ─────────────────────────────────────────────────────────────────

/// Displays live frame-time statistics.  Uses `buffered = false` because
/// the text string changes every frame, so caching it to a backbuffer would
/// rebuild the cache every frame anyway — direct rasterization is cheaper.
struct FpsLabel {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    history:  Rc<RefCell<FrameHistory>>,
    /// Inner Label — not buffered (text changes every frame).
    label:    Label,
}

impl FpsLabel {
    fn new(font: Arc<Font>, history: Rc<RefCell<FrameHistory>>) -> Self {
        let mut label = Label::new("0.0 ms  (0 fps)", font)
            .with_font_size(11.0);
        label.buffered = false; // live counter: no benefit to caching
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            history,
            label,
        }
    }
}

impl Widget for FpsLabel {
    fn type_name(&self) -> &'static str { "FpsLabel" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, 18.0);
        let s = self.label.layout(Size::new(available.width, 18.0));
        self.label.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        Size::new(available.width, 18.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let hist = self.history.borrow();
        let text = format!("Mean CPU usage: {:.2} ms / frame", hist.mean_ms());
        drop(hist);

        // Update label text and color, then paint it.
        self.label.set_text(text);
        self.label.set_color(v.text_dim);

        let h = self.bounds.height;
        let lw = self.label.bounds().width;
        let lh = self.label.bounds().height;
        let ly = (h - lh) * 0.5;
        self.label.set_bounds(Rect::new(0.0, ly, lw, lh));

        ctx.save();
        ctx.translate(12.0, ly);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Screen size label ─────────────────────────────────────────────────────────

/// Displays the current screen dimensions.  Uses `buffered = false` because
/// the text changes on every resize event — direct rasterization is cheaper
/// than rebuilding the cache on each change.
struct ScreenSizeLabel {
    bounds:      Rect,
    children:    Vec<Box<dyn Widget>>,
    screen_size: Rc<Cell<(u32, u32)>>,
    /// Inner Label — not buffered (value changes on resize).
    label:       Label,
}

impl ScreenSizeLabel {
    fn new(font: Arc<Font>, screen_size: Rc<Cell<(u32, u32)>>) -> Self {
        let mut label = Label::new("0 × 0", font)
            .with_font_size(11.0);
        label.buffered = false;
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            screen_size,
            label,
        }
    }
}

impl Widget for ScreenSizeLabel {
    fn type_name(&self) -> &'static str { "ScreenSizeLabel" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, 18.0);
        let s = self.label.layout(Size::new(available.width, 18.0));
        self.label.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        Size::new(available.width, 18.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let (w, h) = self.screen_size.get();
        let text = format!("{w} \u{00d7} {h}");

        self.label.set_text(text);
        self.label.set_color(v.text_dim);

        let height = self.bounds.height;
        let lw = self.label.bounds().width;
        let lh = self.label.bounds().height;
        let ly = (height - lh) * 0.5;
        self.label.set_bounds(Rect::new(0.0, ly, lw, lh));

        ctx.save();
        ctx.translate(12.0, ly);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();
    }

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Run mode row ─────────────────────────────────────────────────────────────

/// Reactive / Continuous toggle.  Two segmented buttons, each with a
/// backbuffered Label child.
struct RunModeRow {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    run_mode: Rc<Cell<RunMode>>,
    hovered:  Option<usize>,
    /// One Label per button.
    labels:   Vec<Label>,
}

impl RunModeRow {
    const BTN_W: f64 = 96.0;
    const BTN_H: f64 = 24.0;
    const LABELS: &'static [&'static str] = &["Reactive", "Continuous"];

    fn new(font: Arc<Font>, run_mode: Rc<Cell<RunMode>>) -> Self {
        let labels = Self::LABELS.iter().map(|text| {
            Label::new(*text, Arc::clone(&font))
                .with_font_size(12.0)
        }).collect();
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            run_mode,
            hovered: None,
            labels,
        }
    }

    fn btn_rect(&self, i: usize) -> Rect {
        let gy = (self.bounds.height - Self::BTN_H) * 0.5;
        Rect::new(12.0 + i as f64 * (Self::BTN_W + 4.0), gy, Self::BTN_W, Self::BTN_H)
    }
}

impl Widget for RunModeRow {
    fn type_name(&self) -> &'static str { "RunModeRow" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, Self::BTN_H + 8.0);
        for i in 0..2 {
            let r = self.btn_rect(i);
            let s = self.labels[i].layout(Size::new(r.width, r.height));
            self.labels[i].set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        }
        Size::new(available.width, Self::BTN_H + 8.0)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let current = self.run_mode.get();
        let modes = [RunMode::Reactive, RunMode::Continuous];

        for (i, (label_text, mode)) in Self::LABELS.iter().zip(modes.iter()).enumerate() {
            let r = self.btn_rect(i);
            let active  = current == *mode;
            let hovered = self.hovered == Some(i);

            let bg = if active { v.accent }
                     else if hovered { v.widget_bg_hovered }
                     else { v.widget_bg };
            ctx.set_fill_color(bg);
            ctx.begin_path();
            ctx.rounded_rect(r.x, r.y, r.width, r.height, 4.0);
            ctx.fill();

            // Update label text + color.
            self.labels[i].set_text(*label_text);
            let text_color = if active { Color::white() } else { v.text_color };
            self.labels[i].set_color(text_color);

            // Center label within button.
            let lw = self.labels[i].bounds().width;
            let lh = self.labels[i].bounds().height;
            let lx = r.x + (r.width - lw) * 0.5;
            let ly = r.y + (r.height - lh) * 0.5;
            self.labels[i].set_bounds(Rect::new(lx, ly, lw, lh));

            ctx.save();
            ctx.translate(lx, ly);
            paint_subtree(&mut self.labels[i], ctx);
            ctx.restore();
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let hit = |p: agg_gui::Point| (0..2).find(|&i| {
            let r = self.btn_rect(i);
            p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
        });
        match event {
            Event::MouseMove { pos } => { self.hovered = hit(*pos); EventResult::Ignored }
            Event::MouseDown { button: agg_gui::MouseButton::Left, pos, .. } => {
                if let Some(i) = hit(*pos) {
                    self.run_mode.set([RunMode::Reactive, RunMode::Continuous][i]);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}

// ── Run mode description label ────────────────────────────────────────────────

/// Dynamic label beneath the run-mode buttons.
/// Reactive: "Only running UI code when there are animations or input."
/// Continuous: "Repainting the UI each frame. FPS: X.X"
struct RunModeDesc {
    bounds:   Rect,
    children: Vec<Box<dyn Widget>>,
    run_mode: Rc<Cell<RunMode>>,
    history:  Rc<RefCell<FrameHistory>>,
    label:    Label,
}

impl RunModeDesc {
    fn new(font: Arc<Font>, run_mode: Rc<Cell<RunMode>>, history: Rc<RefCell<FrameHistory>>) -> Self {
        let mut label = Label::new("", Arc::clone(&font))
            .with_font_size(10.0)
            .with_wrap(true);
        label.buffered = false;
        Self { bounds: Rect::default(), children: Vec::new(), run_mode, history, label }
    }
}

impl Widget for RunModeDesc {
    fn type_name(&self) -> &'static str { "RunModeDesc" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        // Set the text first so wrapped height is measured correctly for the
        // worst-case (reactive) string, then layout once within the available
        // width minus the 12-px horizontal padding used at paint time.
        self.label.set_text(
            "Only running UI code when there are animations or input.".to_owned()
        );
        let inner_w = (available.width - 24.0).max(1.0);
        let s = self.label.layout(Size::new(inner_w, f64::MAX / 2.0));
        self.label.set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        let h = (s.height + 8.0).max(18.0);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        let text = match self.run_mode.get() {
            RunMode::Reactive   => "Only running UI code when there are animations or input.".to_owned(),
            RunMode::Continuous => {
                let hist = self.history.borrow();
                let fps = if hist.mean_ms() < 0.001 { 0.0 } else { 1000.0 / hist.mean_ms() };
                format!("Repainting the UI each frame. FPS: {fps:.1}")
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

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Backend panel ─────────────────────────────────────────────────────────────

/// Build the backend panel widget (240 px wide).
///
/// Mirrors egui's Backend panel layout: renderer/backend info, screen size,
/// run mode selector, FPS sparkline + mean CPU usage, inspector checkbox,
/// and a reset button.
pub fn build_backend_panel(
    font:           Arc<Font>,
    run_mode:       Rc<Cell<RunMode>>,
    history:        Rc<RefCell<FrameHistory>>,
    screen_size:    Rc<Cell<(u32, u32)>>,
    show_inspector: Rc<Cell<bool>>,
    renderer_name:  &'static str,
    backend_name:   &'static str,
    on_reset:       impl FnMut() + 'static,
) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(0.0)
        .with_padding(0.0)
        .with_panel_bg();

    // ── Heading ────────────────────────────────────────────────────────────── (FA4 "laptop")
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(Box::new(
        Label::new("\u{F109} Backend", Arc::clone(&font))
            .with_font_size(14.0)
            .with_margin(Insets::from_sides(12.0, 12.0, 4.0, 4.0))
    ), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);

    // ── Renderer / backend info ────────────────────────────────────────────────
    let running_text = format!("agg-gui running inside {backend_name}.");
    col.push(Box::new(
        Label::new(running_text, Arc::clone(&font))
            .with_font_size(11.0)
            .with_wrap(true)
            .with_margin(Insets::from_sides(12.0, 12.0, 2.0, 2.0))
    ), 0.0);
    let renderer_text = format!("Renderer: {renderer_name}");
    col.push(Box::new(
        Label::new(renderer_text, Arc::clone(&font))
            .with_font_size(11.0)
            .with_wrap(true)
            .with_margin(Insets::from_sides(12.0, 12.0, 2.0, 2.0))
    ), 0.0);
    let backend_text = format!("Backend: {backend_name}");
    col.push(Box::new(
        Label::new(backend_text, Arc::clone(&font))
            .with_font_size(11.0)
            .with_wrap(true)
            .with_margin(Insets::from_sides(12.0, 12.0, 2.0, 2.0))
    ), 0.0);

    // ── Screen size (live) ─────────────────────────────────────────────────────
    col.push(Box::new(ScreenSizeLabel::new(Arc::clone(&font), screen_size)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // ── Run mode toggle ───────────────────────────────────────────────────────
    col.push(Box::new(
        Label::new("Mode", Arc::clone(&font))
            .with_font_size(11.0)
            .with_margin(Insets::from_sides(12.0, 12.0, 2.0, 0.0))
    ), 0.0);

    col.push(Box::new(RunModeRow::new(Arc::clone(&font), Rc::clone(&run_mode))), 0.0);

    // Dynamic description: "Only running UI code..." (Reactive) or "FPS: X.X" (Continuous).
    col.push(Box::new(RunModeDesc::new(Arc::clone(&font), Rc::clone(&run_mode), Rc::clone(&history))), 0.0);

    col.push(Box::new(SizedBox::new().with_height(4.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // ── Mean CPU usage label (primary display, matches egui reference) ────────
    col.push(Box::new(FpsLabel::new(Arc::clone(&font), Rc::clone(&history))), 0.0);

    // ── FPS sparkline (CPU history graph) ────────────────────────────────────
    col.push(Box::new(
        SizedBox::new()
            .with_margin(Insets::from_sides(12.0, 12.0, 4.0, 8.0))
            .with_child(Box::new(Sparkline {
                bounds: Rect::default(), children: Vec::new(),
                history: Rc::clone(&history),
            }))
    ), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // ── agg-gui windows section (Inspector checkbox) ───────────────────────────
    col.push(Box::new(
        Label::new("agg-gui windows:", Arc::clone(&font))
            .with_font_size(11.0)
            .with_margin(Insets::from_sides(12.0, 12.0, 2.0, 0.0))
    ), 0.0);

    col.push(Box::new(
        Checkbox::new("Inspector", Arc::clone(&font), show_inspector.get())
            .with_font_size(13.0)
            .with_state_cell(Rc::clone(&show_inspector))
            .with_margin(Insets::from_sides(10.0, 0.0, 1.0, 1.0))
    ), 0.0);

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    // ── Reset button ──────────────────────────────────────────────────────────
    col.push(Box::new(
        SizedBox::new()
            .with_height(28.0)
            .with_margin(Insets::from_sides(12.0, 12.0, 4.0, 4.0))
            .with_child(Box::new(
                Button::new("Reset all state", Arc::clone(&font))
                    .with_font_size(12.0)
                    .on_click(on_reset)
            ))
    ), 0.0);

    col.push(Box::new(SizedBox::new().with_height(12.0)), 0.0);

    // Flex spacer fills any remaining vertical space so the FlexColumn always
    // occupies the full panel height — this ensures with_panel_bg() paints
    // panel_fill over the entire panel area rather than stopping at content height.
    col.push(Box::new(SizedBox::new()), 1.0);

    Box::new(col)
}
