//! `WindowTitleBar` — paint delegate for the strip at the top of a `Window`.
//!
//! The existence of this widget is purely so every standard widget in the
//! toolkit has **one** background colour accessible through
//! `Widget::background_color`.  Previously `Window::paint` filled two
//! differently-coloured rects (body + title bar) in a single widget; labels
//! placed on the title bar then couldn't be answered by the ancestor-chain
//! walk because `Window::background_color` could only return one of them.
//!
//! # Event handling
//!
//! This widget *does not* handle any input — `hit_test` returns `false` so
//! pointer events pass through to the parent `Window`, which continues to
//! own drag / resize / collapse / button-hover tracking.  Window writes the
//! resulting display state (drag-fill variant, button-hover flags, etc.)
//! into a shared `TitleBarView` every frame before `paint_subtree` descends
//! into this widget.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::label::Label;

// ── Button constants — kept in sync with `window.rs` so event hit-tests
// and paint geometry agree.  Moving them here would split the source of
// truth across two files; owning them in Window and mirroring here is the
// lesser evil.

const CLOSE_R: f64 = 6.0;
const CLOSE_PAD: f64 = 10.0;
const MAX_PAD: f64 = CLOSE_PAD + CLOSE_R * 2.0 + 4.0;
const CORNER_R: f64 = 8.0;

/// Display-state snapshot `Window` hands to the title bar each frame.
pub(crate) struct TitleBarView {
    pub bar_color: Color,
    pub title_color: Color,
    pub collapsed: bool,
    pub maximized: bool,
    pub close_hovered: bool,
    pub maximize_hovered: bool,
}

impl TitleBarView {
    pub fn default_visuals() -> Self {
        Self {
            bar_color: Color::rgb(0.2, 0.2, 0.2),
            title_color: Color::rgb(1.0, 1.0, 1.0),
            collapsed: false,
            maximized: false,
            close_hovered: false,
            maximize_hovered: false,
        }
    }
}

/// Title bar strip painted on top of a Window body.  Hosts the collapse
/// chevron, title label, maximize button, close button, and a 1-px
/// separator below (when the window is expanded).
pub(crate) struct WindowTitleBar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    label: Label,
    state: Rc<RefCell<TitleBarView>>,
}

impl WindowTitleBar {
    pub fn new(title: &str, font: Arc<Font>, state: Rc<RefCell<TitleBarView>>) -> Self {
        let label = Label::new(title, Arc::clone(&font)).with_font_size(13.0);
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            label,
            state,
        }
    }

    #[allow(dead_code)]
    pub fn set_title(&mut self, title: &str) {
        self.label.set_text(title);
    }
}

impl Widget for WindowTitleBar {
    fn type_name(&self) -> &'static str {
        "WindowTitleBar"
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

    // Transparent to input — Window owns drag / double-click / button
    // hit-testing.  This keeps all pointer state in one place.
    fn hit_test(&self, _: Point) -> bool {
        false
    }

    fn layout(&mut self, available: Size) -> Size {
        // Label::layout returns its intrinsic size but doesn't set its own
        // bounds — the caller must.  Without this set_bounds the title
        // label would report (0, 0, 0, 0) at paint time and render at
        // the origin of the title bar strip.
        let s = self
            .label
            .layout(Size::new(available.width - 48.0, available.height));
        self.label
            .set_bounds(Rect::new(0.0, 0.0, s.width, s.height));
        available
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let st = self.state.borrow();
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;

        // Title bar fill. Expanded windows need a square bottom edge against
        // the content separator; collapsed windows are title-bar-only, so the
        // fill must carry the window's bottom corner radius too.
        ctx.set_fill_color(st.bar_color);
        ctx.begin_path();
        if st.collapsed {
            ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
        } else {
            ctx.rect(0.0, 0.0, w, h);
        }
        ctx.fill();

        // 1-px separator below the title bar (= Y=0 in local space),
        // visible only when the window is expanded.  Drawn as a thin rect
        // at y = -1 which is clipped out by the children clip; draw
        // instead as an inset rect along the bottom edge.
        if !st.collapsed {
            ctx.set_fill_color(v.window_stroke);
            ctx.begin_path();
            ctx.rect(0.0, 0.0, w, 1.0);
            ctx.fill();
        }

        // Collapse / expand chevron on the left.
        let chev_x = 12.0;
        let chev_cy = h * 0.5;
        let chev_sz = 4.0;
        ctx.set_stroke_color(v.window_title_text);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        if st.collapsed {
            ctx.move_to(chev_x, chev_cy - chev_sz);
            ctx.line_to(chev_x + chev_sz, chev_cy);
            ctx.line_to(chev_x, chev_cy + chev_sz);
        } else {
            ctx.move_to(chev_x - chev_sz, chev_cy - chev_sz * 0.5);
            ctx.line_to(chev_x, chev_cy + chev_sz * 0.5);
            ctx.line_to(chev_x + chev_sz, chev_cy - chev_sz * 0.5);
        }
        ctx.stroke();

        // Title label — backbuffered `Label` painted via `paint_subtree`.
        self.label.set_color(st.title_color);
        let lw = self.label.bounds().width;
        let lh = self.label.bounds().height;
        let lx = 24.0;
        let ly = (h - lh) * 0.5;
        self.label.set_bounds(Rect::new(lx, ly, lw, lh));
        ctx.save();
        ctx.translate(lx, ly);
        paint_subtree(&mut self.label, ctx);
        ctx.restore();

        // Maximize / restore button.
        let mc_x = w - MAX_PAD;
        let mc_y = h * 0.5;
        let max_bg = if st.maximize_hovered {
            v.window_close_bg_hovered
        } else {
            v.window_close_bg
        };
        ctx.set_fill_color(max_bg);
        ctx.begin_path();
        ctx.circle(mc_x, mc_y, CLOSE_R);
        ctx.fill();

        ctx.set_stroke_color(v.window_close_fg);
        ctx.set_line_width(1.5);
        let sz = 3.5_f64;
        if st.maximized {
            let off = 2.0_f64;
            let sq = sz * 2.0 - off;
            ctx.begin_path();
            ctx.rect(mc_x - sz + off, mc_y - sz + off, sq, sq);
            ctx.stroke();
            ctx.set_fill_color(max_bg);
            ctx.begin_path();
            ctx.rect(mc_x - sz, mc_y - sz, sq, sq);
            ctx.fill();
            ctx.begin_path();
            ctx.rect(mc_x - sz, mc_y - sz, sq, sq);
            ctx.stroke();
        } else {
            ctx.begin_path();
            ctx.rect(mc_x - sz, mc_y - sz, sz * 2.0, sz * 2.0);
            ctx.stroke();
        }

        // Close button.
        let cc_x = w - CLOSE_PAD;
        let cc_y = h * 0.5;
        let close_bg = if st.close_hovered {
            v.window_close_bg_hovered
        } else {
            v.window_close_bg
        };
        ctx.set_fill_color(close_bg);
        ctx.begin_path();
        ctx.circle(cc_x, cc_y, CLOSE_R);
        ctx.fill();

        let arm = CLOSE_R * 0.5;
        ctx.set_stroke_color(v.window_close_fg);
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(cc_x - arm, cc_y - arm);
        ctx.line_to(cc_x + arm, cc_y + arm);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(cc_x + arm, cc_y - arm);
        ctx.line_to(cc_x - arm, cc_y + arm);
        ctx.stroke();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
