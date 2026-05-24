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

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult};
use crate::geometry::{Point, Rect, Size};
use crate::text::Font;
use crate::widget::Widget;
use crate::widgets::chevron::{ChevronWidget, CHEVRON_SIZE};
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
/// chevron (real child widget), title label (real child widget), and
/// paints the maximize / close buttons inline (Window owns their
/// hit-tests for now — follow-up to migrate those to child widgets too).
/// A 1-px separator paints below the bar when the window is expanded.
pub(crate) struct WindowTitleBar {
    bounds: Rect,
    /// `children[0]` is the [`ChevronWidget`]; `children[1]` is the
    /// title [`Label`]. The framework paints + hit-tests them through
    /// the normal child walk.
    children: Vec<Box<dyn Widget>>,
    state: Rc<RefCell<TitleBarView>>,
    /// Shared collapse flag — written by `Window` (whose `collapsed`
    /// field is the source of truth), read by `ChevronWidget` to pick
    /// its glyph orientation each paint.
    collapsed: Rc<Cell<bool>>,
    /// Shared chevron-glyph colour — written by [`paint`] from the
    /// current visuals each frame; read by `ChevronWidget`.
    chevron_color: Rc<Cell<Color>>,
    /// Set by the chevron's `on_click` closure. `Window` drains it on
    /// every `on_event` pass and runs the collapse toggle.
    chevron_clicked: Rc<Cell<bool>>,
}

impl WindowTitleBar {
    pub fn new(title: &str, font: Arc<Font>, state: Rc<RefCell<TitleBarView>>) -> Self {
        let collapsed = Rc::new(Cell::new(false));
        let chevron_clicked = Rc::new(Cell::new(false));
        let chevron_color = Rc::new(Cell::new(Color::white()));
        let chevron = {
            let flag = Rc::clone(&chevron_clicked);
            ChevronWidget::new(Rc::clone(&collapsed))
                .with_color_cell(Rc::clone(&chevron_color))
                .on_click(move || {
                    flag.set(true);
                })
        };
        let label = Label::new(title, Arc::clone(&font)).with_font_size(13.0);
        Self {
            bounds: Rect::default(),
            children: vec![Box::new(chevron), Box::new(label)],
            state,
            collapsed,
            chevron_color,
            chevron_clicked,
        }
    }

    /// Atomically read + clear the shared chevron-clicked flag.
    /// `Window::layout` calls this each frame; when the chevron child
    /// has consumed a `MouseDown` and set the flag, this returns true
    /// and resets it for the next click cycle.
    pub(crate) fn take_chevron_click(&self) -> bool {
        self.chevron_clicked.replace(false)
    }

    /// Write the live collapse state into the shared cell so the
    /// chevron child renders the correct glyph.
    #[allow(dead_code)]
    pub(crate) fn sync_collapsed(&self, collapsed: bool) {
        self.collapsed.set(collapsed);
    }

    #[allow(dead_code)]
    pub fn set_title(&mut self, title: &str) {
        // children[1] is the label (children[0] is the chevron).
        self.children[1].set_label_text(title);
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

    // Hit-tests inside the bar area so the framework can descend into
    // the chevron + label children. On_event returns `Ignored` for any
    // position not claimed by a child, which lets the event bubble up
    // to `Window` for title-drag and close / maximize handling.
    fn hit_test(&self, local: Point) -> bool {
        local.x >= 0.0
            && local.x <= self.bounds.width
            && local.y >= 0.0
            && local.y <= self.bounds.height
    }

    fn layout(&mut self, available: Size) -> Size {
        // Chevron on the left — centred vertically, fixed size from the
        // widget's own CHEVRON_SIZE constant.
        let chev_size = CHEVRON_SIZE;
        let chev_x = 4.0;
        let chev_y = (available.height - chev_size) * 0.5;
        self.children[0].set_bounds(Rect::new(chev_x, chev_y, chev_size, chev_size));

        // Title label — inset past the chevron, reserve right-side
        // room for the inline close / max buttons (Window paints them
        // directly until they're migrated to child widgets).
        let label = &mut self.children[1];
        let s = label.layout(Size::new(available.width - 24.0 - 48.0, available.height));
        let lx = 24.0;
        let ly = (available.height - s.height) * 0.5;
        label.set_bounds(Rect::new(lx, ly, s.width, s.height));
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
            ctx.rounded_rect(0.0, 0.0, w, h, CORNER_R);
            ctx.rect(0.0, 0.0, w, CORNER_R.min(h));
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

        // Chevron + title label are real child widgets — the framework
        // walks them after this paint pass.  Sync per-frame state into
        // the shared cells the children read from.
        self.chevron_color.set(v.window_title_text);
        self.collapsed.set(st.collapsed);
        self.children[1].set_label_color(st.title_color);

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
