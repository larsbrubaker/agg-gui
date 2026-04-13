//! Shared demo UI — identical widget tree for both native and WASM targets.
//!
//! The only platform-specific piece is the cube widget; callers pass it in as
//! `Box<dyn Widget>` so `demo-ui` has no GL dependency.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    App, Button, Checkbox, Color, CompOp, DrawCtx, Event, EventResult,
    FlexColumn, FlexRow, Font, InspectorNode, InspectorPanel, Label, NodeIcon,
    ProgressBar, RadioGroup, Rect, ScrollView, Separator, Size, SizedBox,
    Slider, Spacer, Splitter, Stack, TabView, TextField, TreeView, Widget,
    Window,
};
use agg_gui::widgets::button::ButtonTheme;

// ── Public API ────────────────────────────────────────────────────────────────

/// Build the full demo `App`.
///
/// `cube_widget` is platform-specific (GL widget provided by the caller).
/// Returns the `App` plus shared handles for the inspector toggle and node list.
pub fn build_demo_ui(
    font:        Arc<Font>,
    cube_widget: Box<dyn Widget>,
) -> (App, Rc<Cell<bool>>, Rc<RefCell<Vec<InspectorNode>>>) {
    let show_inspector  = Rc::new(Cell::new(false));
    let inspector_nodes = Rc::new(RefCell::new(Vec::<InspectorNode>::new()));

    let inspector = InspectorPanel::new(
        Arc::clone(&font),
        Rc::clone(&inspector_nodes),
    );

    let show_clone = Rc::clone(&show_inspector);
    let tab_view = TabView::new(Arc::clone(&font))
        .with_tab_bar_height(40.0)
        .with_font_size(13.0)
        .add_tab("Basics",  Box::new(build_basics_content(Arc::clone(&font))))
        .add_tab("Widgets", Box::new(build_widgets_content(Arc::clone(&font))))
        .add_tab("Text",    Box::new(TextDemoWidget::new(Arc::clone(&font))))
        .add_tab("Layout",  Box::new(build_layout_content(Arc::clone(&font))))
        .add_tab("Tree",    Box::new(build_tree_content(Arc::clone(&font))))
        .with_action_button("Inspector", move || {
            show_clone.set(!show_clone.get());
        })
        .with_sidebar(Box::new(inspector), Rc::clone(&show_inspector));

    let window = build_demo_window(Arc::clone(&font), cube_widget);

    let root = Stack::new()
        .add(Box::new(tab_view))
        .add(Box::new(window));

    (App::new(Box::new(root)), show_inspector, inspector_nodes)
}


// ── Tab content builders (all pub so callers can compose freely) ──────────────

pub fn build_basics_content(font: Arc<Font>) -> impl Widget {
    let mut root = agg_gui::Container::new()
        .with_background(Color::rgb(0.94, 0.94, 0.96))
        .with_padding(24.0);

    root.children_mut().push(Box::new(
        Button::new("Primary Action", Arc::clone(&font))
            .with_font_size(14.0)
            .on_click(|| {}),
    ));
    root.children_mut().push(Box::new(
        Button::new("Secondary", Arc::clone(&font))
            .with_font_size(14.0)
            .with_theme(ButtonTheme {
                background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                label_color:        Color::rgb(0.22, 0.45, 0.88),
                border_radius:      6.0,
                focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                focus_ring_width:   2.5,
            }),
    ));
    root.children_mut().push(Box::new(
        Button::new("Destructive", Arc::clone(&font))
            .with_font_size(14.0)
            .with_theme(ButtonTheme {
                background:         Color::rgb(0.88, 0.25, 0.18),
                background_hovered: Color::rgb(0.95, 0.32, 0.24),
                background_pressed: Color::rgb(0.72, 0.18, 0.12),
                label_color:        Color::white(),
                border_radius:      6.0,
                focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                focus_ring_width:   2.5,
            }),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_placeholder("Type something…"),
    ));
    root.children_mut().push(Box::new(
        TextField::new(Arc::clone(&font))
            .with_font_size(14.0)
            .with_text("editable text")
            .with_placeholder("Another field"),
    ));
    root
}

pub fn build_widgets_content(font: Arc<Font>) -> impl Widget {
    let slider_val = Rc::new(Cell::new(0.42_f64));
    let cb1        = Rc::new(Cell::new(true));
    let cb2        = Rc::new(Cell::new(false));
    let cb3        = Rc::new(Cell::new(true));
    let radio_sel  = Rc::new(Cell::new(0_usize));

    let mut col = FlexColumn::new()
        .with_gap(20.0)
        .with_padding(24.0)
        .with_background(Color::rgb(0.94, 0.94, 0.96));

    col.push(Box::new(Label::new("Buttons", Arc::clone(&font))
        .with_font_size(16.0).with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);
    {
        let row = FlexRow::new().with_gap(8.0)
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(34.0).with_child(Box::new(
                Button::new("Primary",   Arc::clone(&font)).with_font_size(13.0).on_click(|| {})
            ))))
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(34.0).with_child(Box::new(
                Button::new("Secondary", Arc::clone(&font)).with_font_size(13.0)
                    .with_theme(ButtonTheme {
                        background:         Color::rgba(0.22, 0.45, 0.88, 0.12),
                        background_hovered: Color::rgba(0.22, 0.45, 0.88, 0.22),
                        background_pressed: Color::rgba(0.22, 0.45, 0.88, 0.35),
                        label_color:        Color::rgb(0.22, 0.45, 0.88),
                        border_radius:      6.0,
                        focus_ring_color:   Color::rgba(0.22, 0.45, 0.88, 0.55),
                        focus_ring_width:   2.5,
                    }).on_click(|| {})
            ))))
            .add(Box::new(SizedBox::new().with_width(120.0).with_height(34.0).with_child(Box::new(
                Button::new("Danger", Arc::clone(&font)).with_font_size(13.0)
                    .with_theme(ButtonTheme {
                        background:         Color::rgb(0.88, 0.25, 0.18),
                        background_hovered: Color::rgb(0.95, 0.32, 0.24),
                        background_pressed: Color::rgb(0.72, 0.18, 0.12),
                        label_color:        Color::white(),
                        border_radius:      6.0,
                        focus_ring_color:   Color::rgba(0.88, 0.25, 0.18, 0.55),
                        focus_ring_width:   2.5,
                    }).on_click(|| {})
            ))));
        col.push(Box::new(row), 0.0);
    }
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Checkboxes", Arc::clone(&font))
        .with_font_size(16.0).with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);
    { let v = Rc::clone(&cb1);
      col.push(Box::new(Checkbox::new("Enable notifications", Arc::clone(&font), cb1.get())
          .on_change(move |v2| v.set(v2))), 0.0); }
    { let v = Rc::clone(&cb2);
      col.push(Box::new(Checkbox::new("Dark mode", Arc::clone(&font), cb2.get())
          .on_change(move |v2| v.set(v2))), 0.0); }
    { let v = Rc::clone(&cb3);
      col.push(Box::new(Checkbox::new("Send analytics", Arc::clone(&font), cb3.get())
          .on_change(move |v2| v.set(v2))), 0.0); }
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Slider", Arc::clone(&font))
        .with_font_size(16.0).with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);
    { let sv = Rc::clone(&slider_val);
      col.push(Box::new(Slider::new(slider_val.get(), 0.0, 1.0, Arc::clone(&font))
          .with_step(0.01).on_change(move |v| sv.set(v))), 0.0); }
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Radio Group", Arc::clone(&font))
        .with_font_size(16.0).with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);
    { let rs = Rc::clone(&radio_sel);
      col.push(Box::new(RadioGroup::new(
          vec!["Option A", "Option B", "Option C"],
          radio_sel.get(), Arc::clone(&font),
      ).on_change(move |i| rs.set(i))), 0.0); }
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Progress Bar", Arc::clone(&font))
        .with_font_size(16.0).with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);
    col.push(Box::new(ProgressBar::new(slider_val.get(), Arc::clone(&font))), 0.0);
    col.push(Box::new(Separator::horizontal()), 0.0);

    col.push(Box::new(Label::new("Text Input", Arc::clone(&font))
        .with_font_size(16.0).with_color(Color::rgb(0.1, 0.1, 0.12))), 0.0);
    col.push(Box::new(TextField::new(Arc::clone(&font))
        .with_font_size(14.0).with_placeholder("Type something here…")), 0.0);
    col.push(Box::new(SizedBox::new().with_height(24.0)), 0.0);

    ScrollView::new(Box::new(col))
}

pub fn build_layout_content(font: Arc<Font>) -> impl Widget {
    TabView::new(Arc::clone(&font))
        .with_tab_bar_height(36.0)
        .with_font_size(13.0)
        .add_tab("Flex",   Box::new(build_flex_demo(Arc::clone(&font))))
        .add_tab("Scroll", Box::new(build_scroll_demo(Arc::clone(&font))))
        .add_tab("Split",  Box::new(build_split_demo(Arc::clone(&font))))
}

pub fn build_tree_content(font: Arc<Font>) -> impl Widget {
    let mut tv = TreeView::new(Arc::clone(&font))
        .with_row_height(26.0).with_font_size(13.0).with_indent_width(18.0);

    let alpha = tv.add_root("Project Alpha", NodeIcon::Package);
    tv.expand(alpha);
    let src = tv.add_child(alpha, "src", NodeIcon::Folder);
    tv.expand(src);
    tv.add_child(src, "main.rs", NodeIcon::File);
    tv.add_child(src, "lib.rs",  NodeIcon::File);
    let widgets_dir = tv.add_child(src, "widgets", NodeIcon::Folder);
    tv.expand(widgets_dir);
    tv.add_child(widgets_dir, "button.rs",      NodeIcon::File);
    tv.add_child(widgets_dir, "scroll_view.rs", NodeIcon::File);
    tv.add_child(widgets_dir, "tree_view.rs",   NodeIcon::File);
    let tests = tv.add_child(alpha, "tests", NodeIcon::Folder);
    tv.expand(tests);
    tv.add_child(tests, "integration.rs", NodeIcon::File);
    tv.add_child(tests, "unit.rs",        NodeIcon::File);
    tv.add_child(alpha, "Cargo.toml", NodeIcon::File);
    tv.add_child(alpha, "README.md",  NodeIcon::File);

    let beta   = tv.add_root("Project Beta", NodeIcon::Package);
    let assets = tv.add_child(beta, "assets", NodeIcon::Folder);
    tv.add_child(assets, "logo.svg", NodeIcon::File);
    tv.add_child(assets, "icons.png", NodeIcon::File);
    let bsrc = tv.add_child(beta, "src", NodeIcon::Folder);
    tv.add_child(bsrc, "app.rs",    NodeIcon::File);
    tv.add_child(bsrc, "config.rs", NodeIcon::File);
    tv.add_child(beta, "Cargo.toml", NodeIcon::File);

    let gamma   = tv.add_root("Project Gamma", NodeIcon::Package);
    let gsrc    = tv.add_child(gamma, "src", NodeIcon::Folder);
    tv.add_child(gsrc, "main.rs",   NodeIcon::File);
    tv.add_child(gsrc, "render.rs", NodeIcon::File);
    tv.add_child(gsrc, "scene.rs",  NodeIcon::File);
    let shaders = tv.add_child(gsrc, "shaders", NodeIcon::Folder);
    tv.add_child(shaders, "vert.glsl", NodeIcon::File);
    tv.add_child(shaders, "frag.glsl", NodeIcon::File);
    tv.add_child(gamma, "Cargo.toml", NodeIcon::File);

    tv
}

// ── TextDemoWidget ────────────────────────────────────────────────────────────

pub struct TextDemoWidget {
    bounds:   Rect,
    font:     Arc<Font>,
    children: Vec<Box<dyn Widget>>,
}

impl TextDemoWidget {
    pub fn new(font: Arc<Font>) -> Self {
        Self { bounds: Rect::default(), font, children: Vec::new() }
    }
}

impl Widget for TextDemoWidget {
    fn type_name(&self) -> &'static str { "TextDemoWidget" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        draw_text_tab(ctx, self.bounds.width, self.bounds.height, &Arc::clone(&self.font));
    }
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn build_demo_window(font: Arc<Font>, cube_widget: Box<dyn Widget>) -> Window {
    let mut content = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_background(Color::rgb(0.08, 0.08, 0.12));

    content.push(Box::new(Label::new("GL — rotating cube", Arc::clone(&font))
        .with_font_size(11.0).with_color(Color::rgba(1.0, 1.0, 1.0, 0.55))), 0.0);
    content.push(cube_widget, 1.0);

    Window::new("3D Demo", font, Box::new(content))
        .with_bounds(Rect::new(60.0, 160.0, 300.0, 260.0))
}

fn build_flex_demo(font: Arc<Font>) -> impl Widget {
    let row = FlexRow::new().with_gap(8.0)
        .add_flex(Box::new(Button::new("One",   Arc::clone(&font)).with_font_size(13.0).on_click(|| {})), 1.0)
        .add_flex(Box::new(Button::new("Two",   Arc::clone(&font)).with_font_size(13.0).on_click(|| {})), 1.0)
        .add_flex(Box::new(Button::new("Three", Arc::clone(&font)).with_font_size(13.0).on_click(|| {})), 1.0);

    FlexColumn::new()
        .with_gap(12.0).with_padding(20.0).with_background(Color::rgb(0.94, 0.94, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(row))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(Arc::clone(&font)).with_font_size(14.0).with_placeholder("Search…")
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0)
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Confirm", Arc::clone(&font)).with_font_size(14.0).on_click(|| {})
        ))))
}

fn build_scroll_demo(font: Arc<Font>) -> impl Widget {
    let mut col = FlexColumn::new()
        .with_gap(8.0).with_padding(16.0).with_background(Color::rgb(0.94, 0.94, 0.96));
    for i in 0..24u32 {
        col.push(Box::new(SizedBox::new().with_height(40.0).with_child(Box::new(
            Button::new(format!("Item {:02}", i + 1), Arc::clone(&font))
                .with_font_size(13.0).on_click(|| {}),
        ))), 0.0);
    }
    ScrollView::new(Box::new(col))
}

fn build_split_demo(font: Arc<Font>) -> impl Widget {
    let left = FlexColumn::new()
        .with_gap(8.0).with_padding(16.0).with_background(Color::rgb(0.96, 0.96, 0.99))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left A", Arc::clone(&font)).with_font_size(13.0).on_click(|| {})
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Left B", Arc::clone(&font)).with_font_size(13.0).on_click(|| {})
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    let right = FlexColumn::new()
        .with_gap(8.0).with_padding(16.0).with_background(Color::rgb(0.99, 0.96, 0.96))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            TextField::new(Arc::clone(&font)).with_font_size(13.0).with_placeholder("Right field…")
        ))))
        .add(Box::new(SizedBox::new().with_height(36.0).with_child(Box::new(
            Button::new("Action", Arc::clone(&font)).with_font_size(13.0).on_click(|| {})
        ))))
        .add_flex(Box::new(Spacer::new()), 1.0);

    Splitter::new(Box::new(left), Box::new(right)).with_ratio(0.4)
}

// ── Text tab draw helpers ─────────────────────────────────────────────────────

fn draw_text_tab(ctx: &mut dyn DrawCtx, w: f64, h: f64, font: &Arc<Font>) {
    ctx.set_font(Arc::clone(font));
    ctx.set_fill_color(Color::rgb(0.94, 0.94, 0.96));
    ctx.begin_path(); ctx.rect(0.0, 0.0, w, h); ctx.fill();

    let pad   = (w.min(h) * 0.03).max(10.0);
    let gap   = pad * 0.6;
    let col_w = (w - pad * 2.0 - gap) / 2.0;
    let row_h = (h - pad * 2.0 - gap) / 2.0;

    let panels = [
        (pad,               pad + row_h + gap, col_w, row_h),
        (pad + col_w + gap, pad + row_h + gap, col_w, row_h),
        (pad,               pad,               col_w, row_h),
        (pad + col_w + gap, pad,               col_w, row_h),
    ];
    for &(px, py, pw, ph) in &panels { draw_card(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[0]; draw_sizes_panel(ctx, px, py, pw, ph); }
    { let (px, py, pw, ph) = panels[1]; draw_measure_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[2]; draw_multiline_panel(ctx, px, py, pw, ph, font); }
    { let (px, py, pw, ph) = panels[3]; draw_buttons_panel(ctx, px, py, pw, ph); }

    let lsize = (w * 0.012).clamp(9.0, 13.0);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.35));
    ctx.fill_text_gsv("agg-gui  Phase 3 — Text", pad, pad * 0.4, lsize);
}

fn draw_card(ctx: &mut dyn DrawCtx, x: f64, y: f64, w: f64, h: f64) {
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.08));
    ctx.set_blend_mode(CompOp::Multiply);
    ctx.begin_path(); ctx.rounded_rect(x + 2.0, y - 2.0, w, h, 10.0); ctx.fill();
    ctx.set_blend_mode(CompOp::SrcOver);
    ctx.set_fill_color(Color::rgb(1.0, 1.0, 1.0));
    ctx.begin_path(); ctx.rounded_rect(x, y, w, h, 10.0); ctx.fill();
}

fn panel_title(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64, title: &str) {
    let size = (pw * 0.055).clamp(10.0, 16.0);
    ctx.set_font_size(size);
    ctx.set_fill_color(Color::rgba(0.0, 0.0, 0.0, 0.55));
    ctx.fill_text(title, px + pw * 0.05, py + ph * 0.86);
}

fn draw_sizes_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64) {
    panel_title(ctx, px, py, pw, ph, "Font Sizes");
    let margin = pw * 0.06;
    let sizes: &[(f64, &str)] = &[
        (10.0, "Caption — 10px  The quick brown fox"),
        (13.0, "Body — 13px  The quick brown fox"),
        (18.0, "Subhead — 18px  The quick"),
        (24.0, "Heading — 24px  agg-gui"),
        (34.0, "Display — 34px  Aa"),
    ];
    let mut y = py + ph * 0.82;
    let adv = ph * 0.155;
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    for &(size, label) in sizes {
        ctx.set_font_size(size);
        ctx.fill_text(label, px + margin, y);
        y -= adv;
    }
}

fn draw_measure_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title(ctx, px, py, pw, ph, "Measure Text");
    let margin    = pw * 0.06;
    let font_size = (pw * 0.08).clamp(14.0, 26.0);
    ctx.set_font_size(font_size);
    let samples = ["Hello", "World!", "agg-gui", "Rust"];
    let col_w   = (pw - margin * 2.0) / samples.len() as f64;
    let base_y  = py + ph * 0.5;
    for (i, &word) in samples.iter().enumerate() {
        let x = px + margin + col_w * i as f64;
        let m = ctx.measure_text(word).unwrap_or_default();
        ctx.set_stroke_color(Color::rgba(0.6, 0.6, 0.65, 0.5)); ctx.set_line_width(1.0);
        ctx.begin_path(); ctx.move_to(x, base_y-2.0); ctx.line_to(x+m.width, base_y-2.0); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.2, 0.5, 0.9, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y+m.ascent); ctx.line_to(x+m.width, base_y+m.ascent); ctx.stroke();
        ctx.set_stroke_color(Color::rgba(0.9, 0.3, 0.3, 0.35));
        ctx.begin_path(); ctx.move_to(x, base_y-m.descent); ctx.line_to(x+m.width, base_y-m.descent); ctx.stroke();
        ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.07));
        ctx.begin_path(); ctx.rect(x, base_y-m.descent, m.width, m.ascent+m.descent); ctx.fill();
        ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.88));
        ctx.fill_text(word, x, base_y);
    }
    let lsize = (pw * 0.032).clamp(7.0, 10.0);
    let ly = py + ph * 0.22;
    let lx = px + margin;
    ctx.set_font_size(lsize);
    ctx.set_fill_color(Color::rgba(0.2, 0.5, 0.9, 0.7)); ctx.fill_text("— ascent", lx, ly);
    ctx.set_fill_color(Color::rgba(0.9, 0.3, 0.3, 0.7)); ctx.fill_text("— descent", lx, ly - lsize * 1.5);
    ctx.set_fill_color(Color::rgba(0.5, 0.5, 0.55, 0.7)); ctx.fill_text("— baseline", lx, ly - lsize * 3.0);
    let _ = font;
}

fn draw_multiline_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64, font: &Arc<Font>) {
    panel_title(ctx, px, py, pw, ph, "Multi-line");
    let margin    = pw * 0.06;
    let font_size = (pw * 0.055).clamp(11.0, 16.0);
    ctx.set_font_size(font_size);
    ctx.set_fill_color(Color::rgba(0.05, 0.05, 0.1, 0.85));
    let line_h = font.line_height_px(font_size) * 1.25;
    let x = px + margin;
    let lines = [
        "agg-gui renders text by", "shaping with rustybuzz,",
        "extracting outlines via",  "ttf-parser, and feeding",
        "Bezier curves into AGG.",  "",
        "No glyph atlas. Kerning",  "and hinting are preserved.",
    ];
    let mut y = py + ph * 0.82;
    for line in &lines {
        if !line.is_empty() { ctx.fill_text(line, x, y); }
        y -= line_h;
    }
}

fn draw_buttons_panel(ctx: &mut dyn DrawCtx, px: f64, py: f64, pw: f64, ph: f64) {
    panel_title(ctx, px, py, pw, ph, "Text + Graphics");
    let margin = pw * 0.07;
    let btn_h  = ph * 0.16;
    let btn_r  = btn_h * 0.35;
    let bx     = px + margin;
    let bw     = pw - margin * 2.0;
    let buttons: &[(&str, Color, Color)] = &[
        ("Primary Action", Color::rgb(0.22, 0.45, 0.88), Color::white()),
        ("Secondary",      Color::rgba(0.22, 0.45, 0.88, 0.12), Color::rgb(0.22, 0.45, 0.88)),
        ("Destructive",    Color::rgb(0.88, 0.25, 0.18), Color::white()),
        ("Disabled",       Color::rgba(0.0, 0.0, 0.0, 0.08), Color::rgba(0.0, 0.0, 0.0, 0.3)),
    ];
    let spacing   = (ph * 0.74) / buttons.len() as f64;
    let font_size = (btn_h * 0.38).clamp(10.0, 16.0);
    ctx.set_font_size(font_size);
    for (i, &(label, bg, fg)) in buttons.iter().enumerate() {
        let by = py + ph * 0.78 - i as f64 * spacing;
        ctx.set_fill_color(bg);
        ctx.set_blend_mode(CompOp::SrcOver);
        ctx.begin_path(); ctx.rounded_rect(bx, by - btn_h*0.5, bw, btn_h, btn_r); ctx.fill();
        if let Some(m) = ctx.measure_text(label) {
            ctx.set_fill_color(fg);
            ctx.fill_text(label, bx + (bw - m.width)*0.5, by - m.ascent*0.45 + m.descent*0.45);
        }
    }
}
