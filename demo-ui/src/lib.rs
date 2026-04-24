//! Shared demo UI — identical widget tree for both native and WASM targets.
//!
//! Implements the egui-style three-panel layout:
//! - **Top menu bar** (~36 px): "File" menu bar matching egui demo layout.
//! - **Central canvas**: floating `Window` widgets, one per demo.
//! - **Right sidebar** (~220 px): scrollable checkbox list grouped by Demos/Tests,
//!   with "Organize windows" button at the bottom — matching egui exactly.
//!
//! The only platform-specific piece is the 3D animation widget, passed by the caller.

mod backend_panel;
mod font_picker;
mod rendering_test;
mod sidebar;
mod state;
mod top_bar;
mod url;
mod windows;

pub use backend_panel::FrameHistory;
pub use state::{SavedState, StateAccessor, WindowState};
// Exposed for Layer-1 behaviour tests: the builders that produce the
// six Window Resize Test sub-windows and the helper type carrying each
// window's egui-parity flags.  Kept re-exports minimal — tests drive
// the library surface only, not internal modules.
pub use windows::{window_resize_sub_windows, ResizeTestWindow};

/// Encode a top-down RGBA8 buffer (first `width*4` bytes = top row, left→right)
/// as a PNG.  Shared by the native harness (writes to disk) and the WASM
/// harness (creates a browser blob for download).  Returns empty on failure.
pub fn encode_png_rgba(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    agg_gui::screenshot::encode_png_rgba(rgba, width, height).unwrap_or_else(|e| {
        eprintln!("encode_png_rgba failed: {e}");
        Vec::new()
    })
}

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    App, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, InspectorNode, InspectorPanel,
    Key, Modifiers, Rect, Size, Stack, ThemePreference, Widget, Window,
};

use backend_panel::{build_backend_panel, RunMode};
use sidebar::{build_sidebar, SidebarEntry, SidebarGroup};
use top_bar::build_top_bar_inner;

// ── Platform hook ─────────────────────────────────────────────────────────────

/// Which host shell is running the demo.  Consumed by the System window's
/// Render tab so platform-specific controls (MSAA as a five-value segmented
/// selector on native vs. a boolean on the web, "Relaunch" vs "Refresh"
/// button label) stay inside demo-ui — demo-native and demo-wasm only
/// declare which variant they are and hand in the hook closure that
/// actually performs the platform-specific restart.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PlatformKind {
    Native,
    Web,
}

/// Platform-specific hooks that demo-ui calls from the Render tab.
#[derive(Clone)]
pub struct PlatformHooks {
    pub kind: PlatformKind,
    /// MSAA sample count actually in effect on the currently-running GL
    /// surface.  Native hosts pass `gl_config.num_samples()`; web hosts
    /// pass `4` when `antialias: true` was honoured at canvas creation,
    /// `0` otherwise.  The Render tab compares this to the pending
    /// `msaa_samples` cell so the Relaunch / Refresh button only
    /// activates when the user has actually changed something.
    pub running_msaa: u8,
    /// Invoked when the user clicks the Render tab's Relaunch / Refresh
    /// button.  Expected behaviour:
    ///   - **Native**: flush any pending save, spawn a fresh copy of the
    ///     process, exit the current one so the new GL surface picks up
    ///     the saved MSAA request.
    ///   - **Web**: call `window.location.reload()` so the browser
    ///     re-creates the canvas with the saved `antialias` flag.
    pub on_reload: Rc<dyn Fn()>,
}

impl PlatformHooks {
    pub fn native(running_msaa: u8, on_reload: impl Fn() + 'static) -> Self {
        Self {
            kind: PlatformKind::Native,
            running_msaa,
            on_reload: Rc::new(on_reload),
        }
    }
    pub fn web(running_msaa: u8, on_reload: impl Fn() + 'static) -> Self {
        Self {
            kind: PlatformKind::Web,
            running_msaa,
            on_reload: Rc::new(on_reload),
        }
    }
}

// ── Canvas background ──────────────────────────────────────────────────────────

struct CanvasBg {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl CanvasBg {
    fn new() -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
        }
    }
}

impl Widget for CanvasBg {
    fn type_name(&self) -> &'static str {
        "CanvasBg"
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
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(ctx.visuals().bg_color);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Top menu bar ──────────────────────────────────────────────────────────────

/// Thin bar at the top of the window — mirrors egui's `Panel::top("menu_bar")`.
/// Contains a theme-toggle row on the right (☀ / 🌙 / System).
// Layout: a single FlexRow child fills the bar.
struct TopMenuBar {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl TopMenuBar {
    fn new(inner_row: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![inner_row],
        }
    }
}

impl Widget for TopMenuBar {
    fn type_name(&self) -> &'static str {
        "TopMenuBar"
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
        let h = 36.0_f64;
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(available.width, h));
            child.set_bounds(Rect::new(0.0, 0.0, available.width, h));
        }
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let v = ctx.visuals();
        ctx.set_fill_color(v.top_bar_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
        // Bottom separator line — match the `Separator` widget colour so
        // horizontal and vertical splits share the same tone.  Y-up: the
        // bar's local y=0 is its BOTTOM edge (where it meets the body
        // below), so the line goes there, not at `height - 1` (which is
        // the top of the bar, flush with the window edge).
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, 1.0);
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Backend panel pane ────────────────────────────────────────────────────────

/// Wraps the backend panel; returns zero width when hidden so FlexRow collapses it.
struct BackendPane {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    show: Rc<Cell<bool>>,
}

impl BackendPane {
    const PANEL_W: f64 = 240.0;
}

impl Widget for BackendPane {
    fn type_name(&self) -> &'static str {
        "BackendPane"
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
        if !self.show.get() {
            self.bounds = Rect::new(0.0, 0.0, 0.0, available.height);
            return Size::new(0.0, available.height);
        }
        let w = Self::PANEL_W.min(available.width);
        self.bounds = Rect::new(0.0, 0.0, w, available.height);
        if let Some(child) = self.children.first_mut() {
            child.layout(Size::new(w, available.height));
            child.set_bounds(Rect::new(0.0, 0.0, w, available.height));
        }
        Size::new(w, available.height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        if !self.show.get() {
            return;
        }
        // 1-px vertical separator line on the right edge, matched to the
        // `Separator` widget colour so horizontal and vertical splits
        // share the same tone.  Drawn in `paint_overlay` so it sits above
        // the child `FlexColumn`'s panel_bg fill.
        let v = ctx.visuals();
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(self.bounds.width - 1.0, 0.0, 1.0, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Sidebar pane ──────────────────────────────────────────────────────────────

/// Fixed-width wrapper for the right sidebar that also paints a 1-px
/// separator line on its left edge.
struct SidebarPane {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
}

impl SidebarPane {
    const PANEL_W: f64 = 220.0;
    fn new(inner: Box<dyn Widget>) -> Self {
        Self {
            bounds: Rect::default(),
            children: vec![inner],
        }
    }
}

impl Widget for SidebarPane {
    fn type_name(&self) -> &'static str {
        "SidebarPane"
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
        let w = Self::PANEL_W.min(available.width);
        self.bounds = Rect::new(0.0, 0.0, w, available.height);
        if let Some(child) = self.children.first_mut() {
            // Inner content starts 1 px in so the separator sits at x=0.
            let inner_w = (w - 1.0).max(0.0);
            child.layout(Size::new(inner_w, available.height));
            child.set_bounds(Rect::new(1.0, 0.0, inner_w, available.height));
        }
        Size::new(w, available.height)
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        // Uses `separator` to match the `Separator` widget tone used by
        // horizontal splits elsewhere.  Drawn in `paint_overlay` so the
        // sidebar's panel_bg fill can't cover it.
        let v = ctx.visuals();
        ctx.set_fill_color(v.separator);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, 1.0, self.bounds.height);
        ctx.fill();
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

// ── Window tiling ──────────────────────────────────────────────────────────────

const WIN_COLS: usize = 4;
const WIN_W: f64 = 360.0;
const WIN_H: f64 = 290.0;
const WIN_GAP_X: f64 = 20.0;
const WIN_GAP_Y: f64 = 20.0;
const WIN_ORIGIN_X: f64 = 20.0;
const WIN_ORIGIN_Y: f64 = 20.0; // from the TOP of the canvas (Y-down thinking)

/// Compute the tiled rect for demo index `i` given canvas `height` (Y-up space).
fn tile_rect(i: usize, canvas_height: f64, win_w: f64, win_h: f64) -> Rect {
    let col = i % WIN_COLS;
    let row = i / WIN_COLS;
    let x = WIN_ORIGIN_X + col as f64 * (WIN_W + WIN_GAP_X);
    let y_down = WIN_ORIGIN_Y + row as f64 * (WIN_H + WIN_GAP_Y);
    let y = (canvas_height - y_down - win_h).max(4.0);
    Rect::new(x, y, win_w, win_h)
}

// ── Demo window list ───────────────────────────────────────────────────────────

struct DemoSpec {
    title: &'static str,
    label: &'static str,
    /// Logical grouping shown as a collapsible section in the sidebar.
    /// Values: "Widgets", "Layout", "Graphics", "Interaction", "Tests", "Tools".
    group: &'static str,
    open: bool,
    win_w: f64,
    win_h: f64,
}

// Exact egui demo list (alphabetical) with egui's original icon prefixes.
// Default open matches egui: Code Example + Widget Gallery.  3D Animation is our
// addition and is open by default as the showcase feature.
// Font Awesome 4 codepoints used as icon prefixes.
// All in the Unicode Private Use Area (U+F000–U+F2FF) so they never
// conflict with regular text characters.
const DEMOS: &[DemoSpec] = &[
    // ── Widgets ──
    DemoSpec {
        title: "\u{F009} Widget Gallery",
        label: "\u{F009} Widget Gallery",
        group: "Widgets",
        open: true,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F1DE} Sliders",
        label: "\u{F1DE} Sliders",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F040} TextEdit",
        label: "\u{F040} TextEdit",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F086} Tooltips",
        label: "\u{F086} Tooltips",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F075} Popups",
        label: "\u{F075} Popups",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F2D0} Modals",
        label: "\u{F2D0} Modals",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F03A} Misc Demos",
        label: "\u{F03A} Misc Demos",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F121} Code Editor",
        label: "\u{F121} Code Editor",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F1C9} Code Example",
        label: "\u{F1C9} Code Example",
        group: "Widgets",
        open: true,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F031} Font Book",
        label: "\u{F031} Font Book",
        group: "Widgets",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    // ── Layout ──
    DemoSpec {
        title: "\u{F096} Frame",
        label: "\u{F096} Frame",
        group: "Layout",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0DB} Panels",
        label: "\u{F0DB} Panels",
        group: "Layout",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0C9} Strip",
        label: "\u{F0C9} Strip",
        group: "Layout",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0CE} Table",
        label: "\u{F0CE} Table",
        group: "Layout",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F07D} Scrolling",
        label: "\u{F07D} Scrolling",
        group: "Layout",
        open: false,
        win_w: 680.0,
        win_h: 540.0,
    },
    DemoSpec {
        title: "\u{F013} Window Options",
        label: "\u{F013} Window Options",
        group: "Layout",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F036} Text Layout",
        label: "\u{F036} Text Layout",
        group: "Layout",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F1B2} Interactive Container",
        label: "\u{F1B2} Interactive Container",
        group: "Layout",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    // ── Graphics ──
    DemoSpec {
        title: "\u{F1FE} Bézier Curve",
        label: "\u{F1FE} Bézier Curve",
        group: "Graphics",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F001} Dancing Strings",
        label: "\u{F001} Dancing Strings",
        group: "Graphics",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F1FC} Painting",
        label: "\u{F1FC} Painting",
        group: "Graphics",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0C3} Rendering Test",
        label: "\u{F0C3} Rendering Test",
        group: "Graphics",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F1B0} Lion",
        label: "\u{F1B0} Lion",
        group: "Graphics",
        open: true,
        win_w: 520.0,
        win_h: 620.0,
    },
    DemoSpec {
        title: "\u{F030} Screenshot",
        label: "\u{F030} Screenshot",
        group: "Graphics",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0D0} Highlighting",
        label: "\u{F0D0} Highlighting",
        group: "Graphics",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F1B3} 3D Animation",
        label: "\u{F1B3} 3D Animation",
        group: "Graphics",
        open: false,
        win_w: 300.0,
        win_h: 260.0,
    },
    DemoSpec {
        title: "\u{F013} System",
        label: "\u{F013} System",
        group: "Tools",
        open: false,
        win_w: 520.0,
        win_h: 640.0,
    },
    DemoSpec {
        title: "\u{F031} LCD Subpixel",
        label: "\u{F031} LCD Subpixel",
        group: "Graphics",
        open: false,
        win_w: 640.0,
        win_h: 720.0,
    },
    // ── Interaction ──
    DemoSpec {
        title: "\u{F0B2} Drag and Drop",
        label: "\u{F0B2} Drag and Drop",
        group: "Interaction",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0A4} Multi Touch",
        label: "\u{F0A4} Multi Touch",
        group: "Interaction",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0E2} Undo Redo",
        label: "\u{F0E2} Undo Redo",
        group: "Interaction",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F002} Scene",
        label: "\u{F002} Scene",
        group: "Interaction",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F108} Extra Viewport",
        label: "\u{F108} Extra Viewport",
        group: "Interaction",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
];

// Tests — regression/correctness windows.  Each one now has a Font Awesome
// icon prefix so tests look like the demos in the sidebar.
const TESTS: &[DemoSpec] = &[
    DemoSpec {
        title: "\u{F0EA} Clipboard Test",
        label: "\u{F0EA} Clipboard Test",
        group: "Tests",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F05B} Cursor Test",
        label: "\u{F05B} Cursor Test",
        group: "Tests",
        open: false,
        win_w: 296.0,
        win_h: 560.0,
    },
    DemoSpec {
        title: "\u{F00A} Grid Test",
        label: "\u{F00A} Grid Test",
        group: "Tests",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F007} Id Test",
        label: "\u{F007} Id Test",
        group: "Tests",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F1DA} Input Event History",
        label: "\u{F1DA} Input Event History",
        group: "Tests",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F11C} Input Test",
        label: "\u{F11C} Input Test",
        group: "Tests",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0E4} Layout Test",
        label: "\u{F0E4} Layout Test",
        group: "Tests",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F0AD} Manual Layout Test",
        label: "\u{F0AD} Manual Layout Test",
        group: "Tests",
        open: false,
        win_w: WIN_W,
        win_h: WIN_H,
    },
    DemoSpec {
        title: "\u{F03E} SVG Test",
        label: "\u{F03E} SVG Test",
        group: "Tests",
        open: false,
        win_w: 960.0,
        win_h: 620.0,
    },
    // The original "Window Resize Test" sidebar entry was a single
    // group toggle that opened all six sub-windows together.  egui's
    // demo treats each sub-window as its own first-class test, so
    // the six entries below replace it — each opens / closes
    // independently from the sidebar.  Initial sizes match the
    // hard-coded rects in `windows::tests::window_resize_sub_windows`.
    DemoSpec {
        title: "↔ auto-sized",
        label: "↔ auto-sized",
        group: "Window Resize Test",
        open: false,
        win_w: 360.0,
        win_h: 240.0,
    },
    DemoSpec {
        title: "↔ resizable + scroll",
        label: "↔ resizable + scroll",
        group: "Window Resize Test",
        open: false,
        win_w: 300.0,
        win_h: 290.0,
    },
    DemoSpec {
        title: "↔ resizable + embedded scroll",
        label: "↔ resizable + embedded scroll",
        group: "Window Resize Test",
        open: false,
        win_w: 300.0,
        win_h: 290.0,
    },
    DemoSpec {
        title: "↔ resizable without scroll",
        label: "↔ resizable without scroll",
        group: "Window Resize Test",
        open: false,
        win_w: 300.0,
        win_h: 290.0,
    },
    DemoSpec {
        title: "↔ resizable with TextEdit",
        label: "↔ resizable with TextEdit",
        group: "Window Resize Test",
        open: false,
        win_w: 300.0,
        win_h: 290.0,
    },
    DemoSpec {
        title: "↔ freely resized",
        label: "↔ freely resized",
        group: "Window Resize Test",
        open: false,
        win_w: 250.0,
        win_h: 150.0,
    },
];

// ── Index of the 3D Animation in DEMOS ─────────────────────────────
// Must match position of "\u{F1B3} 3D Animation" in DEMOS.  Computed at
// runtime via `find_cube_idx()` (checked once in `build_demo_ui`) so
// reordering DEMOS doesn't silently swap the GL cube widget onto some
// other demo's slot — the classic footgun that hit us when Lion was
// inserted in the Graphics group.
const CUBE_TITLE: &str = "\u{F1B3} 3D Animation";
fn find_cube_idx() -> usize {
    DEMOS
        .iter()
        .position(|d| d.title == CUBE_TITLE)
        .expect("DEMOS must contain the 3D Animation entry (CUBE_TITLE)")
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Handles returned by `build_demo_ui` — shared cells used by the platform harness.
pub struct DemoHandles {
    pub show_inspector: Rc<Cell<bool>>,
    pub inspector_nodes: Rc<RefCell<Vec<InspectorNode>>>,
    pub hovered_bounds: Rc<RefCell<Option<Rect>>>,
    pub cube_visible: Rc<Cell<bool>>,
    pub screen_size: Rc<Cell<(u32, u32)>>,
    pub frame_history: Rc<RefCell<FrameHistory>>,
    /// Fullscreen state of the OS window.  The platform harness sets this
    /// cell whenever the window transitions.
    pub window_fullscreen: Rc<Cell<bool>>,
    /// Maximized (not fullscreen) state of the OS window.
    pub window_maximized: Rc<Cell<bool>>,
    /// When set to `true`, the platform harness captures the frame buffer on
    /// the NEXT fully-rendered frame, writes the RGBA8 data + dimensions into
    /// `screenshot_image`, then resets this flag.  Set to `true` from any
    /// widget (e.g. the Screenshot demo button) to request a capture.
    pub screenshot_request: Rc<Cell<bool>>,
    /// Latest captured frame.  `None` until at least one capture completes.
    /// Top-down RGBA8; first `width * 4` bytes are the TOP row.
    ///
    /// Wrapped in `Arc<Vec<u8>>` so the GL texture cache can key on the
    /// Arc's pointer identity (via `draw_image_rgba_arc`).  Using a bare
    /// `Vec<u8>` triggered false cache hits — the allocator reused
    /// addresses across consecutive captures and the content-hash key
    /// (first/last 8 bytes) collided on screenshots whose corners were
    /// stable, causing stale frames to be bound.
    pub screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    /// Transient flag set by the harness during the FIRST render pass of a
    /// capture frame.  Read by the screenshot demo's preview pane so it
    /// paints an empty frame (not the stale previous capture) — this keeps
    /// the captured pixels free of the screenshot-of-a-screenshot recursion.
    pub screenshot_capturing: Rc<Cell<bool>>,
    pub state: StateAccessor,
}

/// Build the full demo `App`.
///
/// Returns `(App, DemoHandles)`. `initial_state` restores window positions and
/// open flags from a previous session; pass `None` on first run.
pub fn build_demo_ui(
    font: Arc<Font>,
    cube_widget: Box<dyn Widget>,
    renderer_name: &'static str,
    backend_name: &'static str,
    initial_state: Option<SavedState>,
    platform: PlatformHooks,
) -> (App, DemoHandles) {
    let show_inspector = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .and_then(|s| s.inspector.as_ref().map(|i| i.open))
            .unwrap_or(false),
    ));
    let inspector_nodes = Rc::new(RefCell::new(Vec::<InspectorNode>::new()));
    let hovered_bounds = Rc::new(RefCell::new(None::<Rect>));
    let screen_size = Rc::new(Cell::new((0u32, 0u32)));
    let window_fullscreen = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.window_fullscreen)
            .unwrap_or(false),
    ));
    let window_maximized = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.window_maximized)
            .unwrap_or(false),
    ));
    let screenshot_request = Rc::new(Cell::new(false));
    let screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>> =
        Rc::new(RefCell::new(None));
    let screenshot_capturing = Rc::new(Cell::new(false));

    // Theme preference — detect OS color scheme so we start in the right mode.
    let initial_theme = top_bar::detect_system_theme();
    match initial_theme {
        ThemePreference::Light => agg_gui::set_visuals(agg_gui::Visuals::light()),
        _ => agg_gui::set_visuals(agg_gui::Visuals::dark()),
    }
    let theme_pref = Rc::new(Cell::new(initial_theme));

    // ── Backend panel visibility — restored from saved state when present. ───
    let backend_initially_open = initial_state
        .as_ref()
        .map(|st| st.backend_open)
        .unwrap_or(false);
    let show_backend = Rc::new(Cell::new(backend_initially_open));

    // ── Backend panel state ────────────────────────────────────────────────────
    let run_mode = Rc::new(Cell::new(RunMode::Reactive));
    let frame_history = Rc::new(RefCell::new(FrameHistory::new()));

    // ── About window open-state cell ──────────────────────────────────────────
    let about_initially_open = initial_state
        .as_ref()
        .map(|st| st.about.open)
        .unwrap_or(true);
    let about_open = Rc::new(Cell::new(about_initially_open));

    // ── Sidebar entries ────────────────────────────────────────────────────────
    let demo_entries: Vec<SidebarEntry> = DEMOS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let open = initial_state
                .as_ref()
                .and_then(|st| st.demos.get(i))
                .map(|ws| ws.open)
                .unwrap_or(s.open);
            SidebarEntry::new(s.label, open)
        })
        .collect();
    let test_entries: Vec<SidebarEntry> = TESTS
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let open = initial_state
                .as_ref()
                .and_then(|st| st.tests.get(i))
                .map(|ws| ws.open)
                .unwrap_or(s.open);
            SidebarEntry::new(s.label, open)
        })
        .collect();

    // cube_visible shares the same cell as the 3D Animation sidebar entry.
    let cube_idx = find_cube_idx();
    let cube_visible = Rc::clone(&demo_entries[cube_idx].open);

    // Shared z-order tracker — every `Window` reports its title here on
    // raise (click-to-front + sidebar rising-edge), maintaining a
    // back-to-front list of recently-touched windows.  Seeded from the
    // saved order so freshly-built windows get re-sorted further down.
    let z_order_cell: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(
        initial_state
            .as_ref()
            .map(|s| s.z_order.clone())
            .unwrap_or_default(),
    ));
    let make_on_raised = || {
        let cell = Rc::clone(&z_order_cell);
        move |title: &str| {
            let mut v = cell.borrow_mut();
            // Move-to-back: removing any prior occurrence keeps the
            // list a true z-order rather than a raise log.
            v.retain(|t| t != title);
            v.push(title.to_string());
        }
    };

    // ── System-settings persistence cells ─────────────────────────────────────
    //
    // Seed from `initial_state` (None → defaults); the System window binds
    // its widgets to these cells so user edits write through to disk via
    // the auto-save loop.  Apply the seeded values to `agg_gui::font_settings`
    // immediately so the first frame already reflects the user's last
    // choice.
    let font_name_cell: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(
        initial_state.as_ref().and_then(|s| s.font_name.clone()),
    ));
    let font_size_scale_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.font_size_scale)
            .unwrap_or(1.0),
    ));
    // Library defaults — LCD subpixel and baseline snapping both ON
    // for standard-DPI displays where they noticeably improve crisp-
    // ness, OFF for HiDPI displays where the physical pixel pitch is
    // small enough that subpixel rendering adds chromatic noise and
    // baseline snapping costs sub-pixel positioning that HiDPI can
    // otherwise express cleanly.  Saved state (from a prior run)
    // overrides if present, so users who have explicitly chosen a
    // value keep their preference.
    let standard_dpi = agg_gui::device_scale() <= 1.25;
    let lcd_enabled_cell: Rc<Cell<bool>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.lcd_enabled)
            .unwrap_or(standard_dpi),
    ));
    let hinting_enabled_cell: Rc<Cell<bool>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.hinting_enabled)
            .unwrap_or(standard_dpi),
    ));
    // Typography-style cells.  Defaults match the agg-rust `truetype_test`
    // reference so first-run users see the neutral / no-effect state.
    let gamma_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.gamma).unwrap_or(1.0),
    ));
    let width_scale_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.width_scale).unwrap_or(1.0),
    ));
    let interval_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.interval).unwrap_or(0.0),
    ));
    let faux_weight_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.faux_weight).unwrap_or(0.0),
    ));
    let faux_italic_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.faux_italic).unwrap_or(0.0),
    ));
    let primary_weight_cell: Rc<Cell<f64>> = Rc::new(Cell::new(
        initial_state
            .as_ref()
            .map(|s| s.primary_weight)
            .unwrap_or(1.0 / 3.0),
    ));
    // OS-surface MSAA sample count.  0 = off (halo-AA only); the platform
    // harness reads this from the persisted state file at boot to set up
    // the GL config, so runtime edits only take effect on restart.  Panel
    // text below makes this explicit.
    let msaa_samples_cell: Rc<Cell<u8>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.msaa_samples).unwrap_or(0),
    ));
    // Persist the active tab inside the System window so users come
    // back to the tab they were last on.
    let system_tab_cell: Rc<Cell<usize>> = Rc::new(Cell::new(
        initial_state.as_ref().map(|s| s.system_tab).unwrap_or(0),
    ));
    // Shared font-index cell — both the System window's combo and the
    // LCD Subpixel demo's combo bind to this cell, so a font picked in
    // either window updates the other live.  Seeded from the persisted
    // font_name when present; otherwise falls back to the library
    // default (Nunito).
    let font_index_cell: Rc<Cell<usize>> = Rc::new(Cell::new({
        let name_lock = font_name_cell.borrow();
        name_lock
            .as_deref()
            .and_then(windows::font_option_index)
            .unwrap_or_else(windows::default_font_index)
    }));
    agg_gui::font_settings::set_font_size_scale(font_size_scale_cell.get());
    agg_gui::font_settings::set_lcd_enabled(lcd_enabled_cell.get());
    agg_gui::font_settings::set_hinting_enabled(hinting_enabled_cell.get());
    agg_gui::font_settings::set_gamma(gamma_cell.get());
    agg_gui::font_settings::set_width(width_scale_cell.get());
    agg_gui::font_settings::set_interval(interval_cell.get());
    agg_gui::font_settings::set_faux_weight(faux_weight_cell.get());
    agg_gui::font_settings::set_faux_italic(faux_italic_cell.get());
    agg_gui::font_settings::set_primary_weight(primary_weight_cell.get());
    // Always seed the system-font override so the library default
    // (Nunito) appears uniformly from the first frame, not just after
    // the user touches the picker.  Persisted name takes precedence;
    // otherwise we fall back to whatever `font_index_cell` resolved
    // to above, which itself defaults to `default_font_index()`.
    let resolved_idx = font_name_cell
        .borrow()
        .as_deref()
        .and_then(windows::font_option_index)
        .unwrap_or_else(|| font_index_cell.get());
    if let Some(name) = windows::font_option_names().get(resolved_idx).copied() {
        if let Some(f) = windows::load_font_by_name(name) {
            agg_gui::font_settings::set_system_font(Some(f));
        }
    }
    // Register the cells so `system_view` (called from the dispatcher
    // below) can bind its widgets without a new function signature.
    windows::init_system_cells(windows::SystemCells {
        font_name: Rc::clone(&font_name_cell),
        font_index: Rc::clone(&font_index_cell),
        font_size_scale: Rc::clone(&font_size_scale_cell),
        lcd_enabled: Rc::clone(&lcd_enabled_cell),
        hinting_enabled: Rc::clone(&hinting_enabled_cell),
        gamma: Rc::clone(&gamma_cell),
        width_scale: Rc::clone(&width_scale_cell),
        interval: Rc::clone(&interval_cell),
        faux_weight: Rc::clone(&faux_weight_cell),
        faux_italic: Rc::clone(&faux_italic_cell),
        primary_weight: Rc::clone(&primary_weight_cell),
        msaa_samples: Rc::clone(&msaa_samples_cell),
        system_tab: Rc::clone(&system_tab_cell),
        platform: platform.clone(),
    });

    // ── Reset cells — one per window ───────────────────────────────────────────
    let all_specs_count = DEMOS.len() + TESTS.len();
    let reset_cells: Vec<Rc<Cell<Option<Rect>>>> = (0..all_specs_count)
        .map(|_| Rc::new(Cell::new(None)))
        .collect();

    // ── Position output cells — written each layout pass for persistence ───────
    let demo_pos_cells: Vec<Rc<Cell<Rect>>> = (0..DEMOS.len())
        .map(|_| Rc::new(Cell::new(Rect::default())))
        .collect();
    let test_pos_cells: Vec<Rc<Cell<Rect>>> = (0..TESTS.len())
        .map(|_| Rc::new(Cell::new(Rect::default())))
        .collect();
    let about_pos_cell: Rc<Cell<Rect>> = Rc::new(Cell::new(Rect::default()));

    // Default canvas height used by tile_rect. 720px is a reasonable fallback;
    // it will look correct on most 1080p+ screens after accounting for the OS bar.
    let default_canvas_h = 720.0_f64;

    // ── Organize Windows callback ──────────────────────────────────────────────
    // Two separate clones: one for the sidebar button, one for Ctrl+Shift+O shortcut.
    let rc_for_cb: Vec<_> = reset_cells.iter().map(Rc::clone).collect();
    let rc_for_key: Vec<_> = reset_cells.iter().map(Rc::clone).collect();

    let specs_w: Vec<f64> = DEMOS
        .iter()
        .map(|s| s.win_w)
        .chain(TESTS.iter().map(|s| s.win_w))
        .collect();
    let specs_h: Vec<f64> = DEMOS
        .iter()
        .map(|s| s.win_h)
        .chain(TESTS.iter().map(|s| s.win_h))
        .collect();

    let on_organize = {
        let sw = specs_w.clone();
        let sh = specs_h.clone();
        move || {
            for (i, cell) in rc_for_cb.iter().enumerate() {
                let r = tile_rect(i, default_canvas_h, sw[i], sh[i]);
                cell.set(Some(r));
            }
        }
    };

    // ── Tools entries (Inspector) ──────────────────────────────────────────────
    // The Inspector is not a Window — it's an overlay on the canvas.  We expose
    // it through a sidebar entry whose open-cell IS `show_inspector` itself.
    let tool_entries: Vec<SidebarEntry> = vec![SidebarEntry::from_cell(
        "\u{F188} Inspector",
        Rc::clone(&show_inspector),
    )];

    // ── Sidebar groups ─────────────────────────────────────────────────────────
    // Build the ordered group list by partitioning demo_entries by each spec's
    // `group` field, then appending `Tests` and `Tools`.  Within each group,
    // entries are sorted alphabetically by their visible name — which means
    // stripping the leading Font Awesome icon (PUA range 0xE000–0xF8FF) +
    // separating whitespace before comparing.
    let group_names: &[&'static str] = &[
        "Widgets",
        "Layout",
        "Graphics",
        "Interaction",
        "Tests",
        "Window Resize Test",
        "Tools",
    ];
    /// Case-insensitive sort key for an entry label like "\u{F1DE} Sliders".
    fn sidebar_sort_key(s: &str) -> String {
        s.trim_start_matches(|c: char| {
            let cp = c as u32;
            (0xE000..=0xF8FF).contains(&cp)
        })
        .trim_start()
        .to_lowercase()
    }
    let sidebar_groups: Vec<SidebarGroup> = group_names
        .iter()
        .map(|&name| {
            // The TESTS array is split between two visual groups —
            // entries that start with "↔" go under the new
            // "Window Resize Test" accordion, the rest stay in the
            // generic "Tests" group.
            let mut entries: Vec<&SidebarEntry> = match name {
                "Tests" => test_entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| !e.label.starts_with('↔'))
                    .map(|(_, e)| e)
                    .collect(),
                "Window Resize Test" => test_entries
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.label.starts_with('↔'))
                    .map(|(_, e)| e)
                    .collect(),
                // Tools = the standalone tool entries (Inspector,
                // etc.) PLUS any DEMOS tagged with `group="Tools"`
                // (e.g. System, which is conceptually a tool the
                // developer reaches for, not a content demo).
                "Tools" => tool_entries
                    .iter()
                    .chain(
                        demo_entries
                            .iter()
                            .enumerate()
                            .filter(|(i, _)| DEMOS[*i].group == "Tools")
                            .map(|(_, e)| e),
                    )
                    .collect(),
                _ => demo_entries
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| DEMOS[*i].group == name)
                    .map(|(_, e)| e)
                    .collect(),
            };
            entries.sort_by(|a, b| sidebar_sort_key(a.label).cmp(&sidebar_sort_key(b.label)));
            SidebarGroup { name, entries }
        })
        .collect();

    // ── Sidebar ────────────────────────────────────────────────────────────────
    let sidebar_widget = build_sidebar(
        Arc::clone(&font),
        Rc::clone(&about_open),
        &sidebar_groups,
        on_organize,
    );
    let sidebar_panel = SidebarPane::new(sidebar_widget);

    // ── Canvas stack (floating windows) ───────────────────────────────────────
    let mut canvas = Stack::new().add(Box::new(CanvasBg::new()));

    // Add DEMO windows.
    for (i, spec) in DEMOS.iter().enumerate() {
        let open_cell = Rc::clone(&demo_entries[i].open);
        let reset_cell = Rc::clone(&reset_cells[i]);
        // `has_valid_bounds` filters out zero-size saved rects — a demo that
        // was never opened last session persists with (0,0,0,0) because its
        // position cell never got written.  Without this filter, those
        // entries turn into `with_bounds(0,0,0,0)` which the parent Stack
        // stretches to fill the whole canvas on first layout.
        let initial = initial_state
            .as_ref()
            .and_then(|st| st.demos.get(i))
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| tile_rect(i, default_canvas_h, spec.win_w, spec.win_h));

        let content: Box<dyn Widget> = if i == cube_idx {
            // Cube content requires the platform-provided cube_widget.
            // Use a placeholder here; replaced immediately after the loop.
            windows::coming_soon()
        } else {
            build_demo_content(
                spec.title,
                Arc::clone(&font),
                Rc::clone(&screenshot_request),
                Rc::clone(&screenshot_image),
                Rc::clone(&screenshot_capturing),
            )
        };

        // Frame demo mirrors egui's Frame inspector, which is `.resizable(false)`
        // and sizes itself to its contents.  We opt the window in to auto-size
        // for the same behaviour here.
        let auto_size = spec.title == "\u{F096} Frame";
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(
                initial.x,
                initial.y,
                initial.width,
                initial.height,
            ))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell)
            .with_position_cell(Rc::clone(&demo_pos_cells[i]))
            .with_auto_size(auto_size)
            .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(win));
    }

    // Replace the placeholder cube window with the real GL cube content.
    // Children layout: [0] = CanvasBg, [1..=30] = DEMOS windows in order.
    {
        let open_cell = Rc::clone(&demo_entries[cube_idx].open);
        let reset_cell = Rc::clone(&reset_cells[cube_idx]);
        let spec = &DEMOS[cube_idx];
        let initial = initial_state
            .as_ref()
            .and_then(|st| st.demos.get(cube_idx))
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| tile_rect(cube_idx, default_canvas_h, spec.win_w, spec.win_h));
        let content = windows::cube_content(Arc::clone(&font), cube_widget);
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(
                initial.x,
                initial.y,
                initial.width,
                initial.height,
            ))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell)
            .with_position_cell(Rc::clone(&demo_pos_cells[cube_idx]))
            .on_raised(make_on_raised());
        // Replace index 1 + cube_idx (offset by the CanvasBg at [0]).
        canvas.children_mut()[1 + cube_idx] = Box::new(win);
    }

    // Pre-build the six "↔" sub-windows from the resize-test set so
    // we can pop the matching entry as the TEST loop visits each
    // title.  The result is one Window per sidebar entry — the sub-
    // windows are no longer lumped under a single "Window Resize
    // Test" group toggle, matching egui where each ↔ window is its
    // own first-class demo with its own open / close state.
    let mut resize_sub: std::collections::HashMap<String, windows::ResizeTestWindow> =
        windows::window_resize_sub_windows(Arc::clone(&font))
            .into_iter()
            .map(|e| (e.title.clone(), e))
            .collect();

    // Add TEST windows.
    for (i, spec) in TESTS.iter().enumerate() {
        let total_i = DEMOS.len() + i;
        let open_cell = Rc::clone(&test_entries[i].open);
        let reset_cell = Rc::clone(&reset_cells[total_i]);
        let initial = initial_state
            .as_ref()
            .and_then(|st| st.tests.get(i))
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| tile_rect(total_i, default_canvas_h, spec.win_w, spec.win_h));

        // Resize sub-windows go through their own builder so the
        // per-window flags (`auto_size`, `vscroll`, `tight_fit`,
        // axis-locked resize) get applied exactly as egui requires.
        if let Some(sub) = resize_sub.remove(spec.title) {
            let mut win = Window::new(spec.title, Arc::clone(&font), sub.content)
                .with_bounds(Rect::new(
                    initial.x,
                    initial.y,
                    initial.width,
                    initial.height,
                ))
                .with_visible_cell(open_cell)
                .with_reset_cell(reset_cell)
                .with_position_cell(Rc::clone(&test_pos_cells[i]))
                .on_raised(make_on_raised());
            // `with_vscroll` mutates the children list, so it must
            // run before any other builder that reads them.
            if sub.vscroll {
                win = win.with_vscroll(true);
            }
            if sub.auto_size {
                win = win.with_auto_size(true);
            } else {
                win = win.with_resizable_axes(sub.resizable_h, sub.resizable_v);
                if !sub.resizable {
                    win = win.with_resizable(false);
                }
            }
            if sub.tight_fit {
                win = win.with_tight_content_fit(true);
            }
            if sub.floor_fit {
                win = win.with_height_floor_to_content(true);
            }
            canvas = canvas.add(Box::new(win));
            continue;
        }

        let content: Box<dyn Widget> = match spec.title {
            "\u{F0EA} Clipboard Test" => windows::clipboard_test(Arc::clone(&font)),
            "\u{F05B} Cursor Test" => windows::cursor_test(Arc::clone(&font)),
            "\u{F00A} Grid Test" => windows::grid_test(Arc::clone(&font)),
            "\u{F007} Id Test" => windows::id_test(Arc::clone(&font)),
            "\u{F1DA} Input Event History" => windows::input_event_history(Arc::clone(&font)),
            "\u{F11C} Input Test" => windows::input_test(Arc::clone(&font)),
            "\u{F0E4} Layout Test" => windows::layout_test(Arc::clone(&font)),
            "\u{F0AD} Manual Layout Test" => windows::manual_layout_test(Arc::clone(&font)),
            "\u{F03E} SVG Test" => windows::svg_test(Arc::clone(&font)),
            _ => windows::coming_soon(),
        };
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(
                initial.x,
                initial.y,
                initial.width,
                initial.height,
            ))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell)
            .with_position_cell(Rc::clone(&test_pos_cells[i]))
            .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(win));
    }

    // ── About window ──────────────────────────────────────────────────────────
    {
        let about_initial = initial_state
            .as_ref()
            .map(|st| &st.about)
            .filter(|ws| ws.has_valid_bounds())
            .map(|ws| ws.to_rect())
            .unwrap_or_else(|| Rect::new(80.0, 80.0, 440.0, 500.0));
        let about_win = Window::new(
            "About agg-gui",
            Arc::clone(&font),
            windows::about(Arc::clone(&font)),
        )
        .with_bounds(about_initial)
        .with_visible_cell(Rc::clone(&about_open))
        .with_position_cell(Rc::clone(&about_pos_cell))
        .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(about_win));
    }

    // ── Inspector as a floating window ─────────────────────────────────────────
    // Visible-cell is shared with the Tools sidebar entry so F12 / sidebar
    // toggle and window close button all stay in sync.  Expand / select
    // state is restored from `initial_state.inspector` and snapshotted out
    // each frame into `inspector_snapshot_cell` for persistence.
    let inspector_snapshot_cell: Rc<RefCell<Option<agg_gui::InspectorSavedState>>> =
        Rc::new(RefCell::new(None));
    {
        let mut inspector = InspectorPanel::new(
            Arc::clone(&font),
            Rc::clone(&inspector_nodes),
            Rc::clone(&hovered_bounds),
        )
        .with_snapshot_cell(Rc::clone(&inspector_snapshot_cell));
        if let Some(saved) = initial_state.as_ref().and_then(|s| s.inspector.clone()) {
            inspector.apply_saved_state(agg_gui::InspectorSavedState {
                expanded: saved.expanded,
                selected: saved.selected,
                props_h: saved.props_h,
            });
        }
        let inspector_win =
            Window::new("\u{F188} Inspector", Arc::clone(&font), Box::new(inspector))
                .with_bounds(Rect::new(960.0, 60.0, 320.0, 520.0))
                .with_visible_cell(Rc::clone(&show_inspector))
                .on_raised(make_on_raised());
        canvas = canvas.add(Box::new(inspector_win));
    }

    // ── Restore saved z-order ─────────────────────────────────────────────────
    //
    // Up to this point every Window was added in DEMOS / TESTS array
    // order.  If a previous session wrote a `z_order`, walk that list
    // back-to-front and physically pull each matching child to the end
    // of `canvas.children` — so when the Stack paints, the user's last
    // top-most window is still on top.  Windows the user never raised
    // keep their default array-order position (rendered behind the
    // persisted ones).
    {
        let saved_order = z_order_cell.borrow().clone();
        if !saved_order.is_empty() {
            let kids = canvas.children_mut();
            // Walk back-to-front.  After processing, the most-recently
            // raised window ends up at the very end of `kids`, which is
            // the front of the z-order in `Stack`'s render scheme.
            // Match by `Widget::id()` — `Window` returns its title;
            // other children (CanvasBg) return None and never match.
            for title in &saved_order {
                if let Some(idx) = kids.iter().position(|w| w.id() == Some(title.as_str())) {
                    let win = kids.remove(idx);
                    kids.push(win);
                }
            }
        }
    }
    let main_area = canvas;

    // ── Backend panel (left side, visible only when show_backend is true) ────────
    //
    // Build the Reset-all-state closure before passing it in.  Reset must:
    //   - Close every demo / test / about window (open cells → false).
    //   - Retile every window to its default `tile_rect` so the next time
    //     the user opens one, bounds are the configured defaults rather
    //     than the last user-dragged geometry.
    //   - Restore system font / size / LCD / hinting to defaults, both
    //     in the `font_settings` globals (so the live render updates)
    //     AND in the persisted cells (so the next auto-save records
    //     the reset state).
    let on_reset_all = {
        let demo_open = demo_entries
            .iter()
            .map(|e| Rc::clone(&e.open))
            .collect::<Vec<_>>();
        let test_open = test_entries
            .iter()
            .map(|e| Rc::clone(&e.open))
            .collect::<Vec<_>>();
        let about_open = Rc::clone(&about_open);
        let reset_cells = reset_cells.iter().map(Rc::clone).collect::<Vec<_>>();
        let specs_w = specs_w.clone();
        let specs_h = specs_h.clone();
        let font_name = Rc::clone(&font_name_cell);
        let font_index = Rc::clone(&font_index_cell);
        let font_scale = Rc::clone(&font_size_scale_cell);
        let lcd_cell = Rc::clone(&lcd_enabled_cell);
        let hint_cell = Rc::clone(&hinting_enabled_cell);
        let gamma = Rc::clone(&gamma_cell);
        let width_scl = Rc::clone(&width_scale_cell);
        let interval = Rc::clone(&interval_cell);
        let fweight = Rc::clone(&faux_weight_cell);
        let fitalic = Rc::clone(&faux_italic_cell);
        let pweight = Rc::clone(&primary_weight_cell);
        move || {
            // Close every window.
            for c in &demo_open {
                c.set(false);
            }
            for c in &test_open {
                c.set(false);
            }
            about_open.set(false);
            // Retile — `Window::set_bounds` picks up `Some(rect)` on its
            // next layout and snaps back to that rect (this is how the
            // "Organize windows" keyboard shortcut also works).
            for (i, cell) in reset_cells.iter().enumerate() {
                let r = tile_rect(i, default_canvas_h, specs_w[i], specs_h[i]);
                cell.set(Some(r));
            }
            // System settings → library defaults (both runtime globals
            // + cells).  Style parameters reset to pass-through; font
            // resets to Nunito; LCD + baseline snapping reset to the
            // first-run value for the **current** device scale (ON for
            // standard-DPI, OFF for HiDPI).
            let default_idx = windows::default_font_index();
            let default_name = windows::font_option_names().get(default_idx).copied();
            let standard_dpi = agg_gui::device_scale() <= 1.25;
            agg_gui::font_settings::set_system_font(
                default_name.and_then(windows::load_font_by_name),
            );
            agg_gui::font_settings::set_font_size_scale(1.0);
            agg_gui::font_settings::set_lcd_enabled(standard_dpi);
            agg_gui::font_settings::set_hinting_enabled(standard_dpi);
            agg_gui::font_settings::set_gamma(1.0);
            agg_gui::font_settings::set_width(1.0);
            agg_gui::font_settings::set_interval(0.0);
            agg_gui::font_settings::set_faux_weight(0.0);
            agg_gui::font_settings::set_faux_italic(0.0);
            agg_gui::font_settings::set_primary_weight(1.0 / 3.0);
            *font_name.borrow_mut() = default_name.map(|s| s.to_string());
            // Snap both font-picker ComboBoxes to the library default.
            font_index.set(default_idx);
            font_scale.set(1.0);
            lcd_cell.set(standard_dpi);
            hint_cell.set(standard_dpi);
            gamma.set(1.0);
            width_scl.set(1.0);
            interval.set(0.0);
            fweight.set(0.0);
            fitalic.set(0.0);
            pweight.set(1.0 / 3.0);
        }
    };

    // System-window open cell — the Backend sidebar's "System" pill binds to
    // the same cell as the sidebar's System demo entry, so the two stay in
    // lock-step without an extra signal round-trip.
    let system_open = {
        let idx = DEMOS
            .iter()
            .position(|d| d.title == "\u{F013} System")
            .expect("DEMOS must contain the System window entry");
        Rc::clone(&demo_entries[idx].open)
    };
    let backend_panel_widget = build_backend_panel(
        Arc::clone(&font),
        Rc::clone(&run_mode),
        Rc::clone(&frame_history),
        Rc::clone(&screen_size),
        Rc::clone(&show_inspector),
        system_open,
        renderer_name,
        backend_name,
        on_reset_all,
    );
    let backend_pane = BackendPane {
        bounds: Rect::default(),
        children: vec![backend_panel_widget],
        show: Rc::clone(&show_backend),
    };

    // ── Demos body: [backend panel] [canvas] [sidebar] — sidebar on RIGHT ─────
    let demos_body = FlexRow::new()
        .with_gap(0.0)
        .add(Box::new(backend_pane))
        .add_flex(Box::new(main_area), 1.0)
        .add(Box::new(sidebar_panel));

    // ── Top bar inner row ─────────────────────────────────────────────────────
    let top_bar_inner = build_top_bar_inner(
        Arc::clone(&font),
        Rc::clone(&show_backend),
        Rc::clone(&theme_pref),
    );

    // ── Root: top menu bar above the demos body ────────────────────────────────
    let root = FlexColumn::new()
        .with_gap(0.0)
        .add(Box::new(TopMenuBar::new(top_bar_inner)))
        .add_flex(Box::new(demos_body), 1.0);

    let mut app = App::new(Box::new(root));

    // ── Global keyboard shortcuts ─────────────────────────────────────────────
    // Ctrl+Shift+O — Organize Windows (tile all visible windows).
    // Ctrl+Shift+R — Reset Memory (resets all open/collapsed window states).
    let on_organize_key = {
        move || {
            for (i, cell) in rc_for_key.iter().enumerate() {
                let r = tile_rect(i, default_canvas_h, specs_w[i], specs_h[i]);
                cell.set(Some(r));
            }
        }
    };
    let demo_open_cells: Vec<Rc<Cell<bool>>> =
        demo_entries.iter().map(|e| Rc::clone(&e.open)).collect();
    let test_open_cells: Vec<Rc<Cell<bool>>> =
        test_entries.iter().map(|e| Rc::clone(&e.open)).collect();

    app.set_global_key_handler({
        let on_org = on_organize_key;
        move |key: Key, mods: Modifiers| {
            if mods.ctrl && mods.shift {
                match key {
                    Key::Char('O') | Key::Char('o') => {
                        on_org();
                        return true;
                    }
                    Key::Char('R') | Key::Char('r') => {
                        // Reset Memory: close all demo/test windows.
                        for c in &demo_open_cells {
                            c.set(false);
                        }
                        for c in &test_open_cells {
                            c.set(false);
                        }
                        return true;
                    }
                    _ => {}
                }
            }
            false
        }
    });

    // ── StateAccessor — collect all shared cells for the platform harness ─────
    let state_accessor = StateAccessor {
        demo_open: demo_entries.iter().map(|e| Rc::clone(&e.open)).collect(),
        demo_pos: demo_pos_cells,
        test_open: test_entries.iter().map(|e| Rc::clone(&e.open)).collect(),
        test_pos: test_pos_cells,
        about_open: Rc::clone(&about_open),
        about_pos: about_pos_cell,
        backend_open: Rc::clone(&show_backend),
        window_size: Rc::clone(&screen_size),
        window_fullscreen: Rc::clone(&window_fullscreen),
        window_maximized: Rc::clone(&window_maximized),
        inspector_snapshot: {
            let cell = Rc::clone(&inspector_snapshot_cell);
            let open_cell = Rc::clone(&show_inspector);
            Rc::new(move || {
                cell.borrow()
                    .as_ref()
                    .map(|s| crate::state::InspectorPersist {
                        expanded: s.expanded.clone(),
                        selected: s.selected,
                        props_h: s.props_h,
                        open: open_cell.get(),
                    })
            })
        },
        font_name: Rc::clone(&font_name_cell),
        font_size_scale: Rc::clone(&font_size_scale_cell),
        lcd_enabled: Rc::clone(&lcd_enabled_cell),
        hinting_enabled: Rc::clone(&hinting_enabled_cell),
        gamma: Rc::clone(&gamma_cell),
        width_scale: Rc::clone(&width_scale_cell),
        interval: Rc::clone(&interval_cell),
        faux_weight: Rc::clone(&faux_weight_cell),
        faux_italic: Rc::clone(&faux_italic_cell),
        primary_weight: Rc::clone(&primary_weight_cell),
        msaa_samples: Rc::clone(&msaa_samples_cell),
        system_tab: Rc::clone(&system_tab_cell),
        z_order: Rc::clone(&z_order_cell),
    };

    let handles = DemoHandles {
        show_inspector,
        inspector_nodes,
        hovered_bounds,
        cube_visible,
        screen_size,
        frame_history,
        window_fullscreen,
        window_maximized,
        screenshot_request: Rc::clone(&screenshot_request),
        screenshot_image: Rc::clone(&screenshot_image),
        screenshot_capturing: Rc::clone(&screenshot_capturing),
        state: state_accessor,
    };
    (app, handles)
}

// ── Demo content dispatcher ────────────────────────────────────────────────────

fn build_demo_content(
    title: &str,
    font: Arc<Font>,
    screenshot_request: Rc<Cell<bool>>,
    screenshot_image: Rc<RefCell<Option<(Arc<Vec<u8>>, u32, u32)>>>,
    screenshot_capturing: Rc<Cell<bool>>,
) -> Box<dyn Widget> {
    match title {
        // basic.rs
        "\u{F121} Code Editor" => windows::code_editor(font),
        "\u{F1DE} Sliders" => windows::sliders(font),
        "\u{F040} TextEdit" => windows::text_edit(font),
        "\u{F086} Tooltips" => windows::tooltips(font),
        // code_example.rs
        "\u{F1C9} Code Example" => windows::code_example(font),
        // gallery.rs
        "\u{F009} Widget Gallery" => windows::widget_gallery(font),
        // animation.rs
        "\u{F1FE} Bézier Curve" => windows::bezier_curve(font),
        "\u{F001} Dancing Strings" => windows::dancing_strings(font),
        "\u{F1FC} Painting" => windows::painting(font),
        // frame_demo.rs
        "\u{F096} Frame" => windows::frame_demo(font),
        // lion.rs — halo-AA pipeline proof
        "\u{F1B0} Lion" => windows::lion_demo(font),
        // misc.rs
        "\u{F108} Extra Viewport" => windows::extra_viewport(font),
        "\u{F0D0} Highlighting" => windows::highlighting(font),
        "\u{F1B2} Interactive Container" => windows::interactive_container(font),
        "\u{F031} Font Book" => windows::font_book(font),
        "\u{F03A} Misc Demos" => windows::misc_demos(font),
        // interaction.rs
        "\u{F0B2} Drag and Drop" => windows::drag_and_drop(font),
        "\u{F07D} Scrolling" => windows::scrolling_demo(font),
        "\u{F0DB} Panels" => windows::panels_demo(font),
        "\u{F075} Popups" => windows::popups_demo(font),
        "\u{F0C3} Rendering Test" => rendering_test::rendering_test_view(font),
        "\u{F013} System" => windows::system_view(font),
        "\u{F031} LCD Subpixel" => windows::truetype_lcd_view(font),
        "\u{F002} Scene" => windows::scene_demo(font),
        "\u{F030} Screenshot" => windows::screenshot_demo(
            font,
            screenshot_request,
            screenshot_image,
            screenshot_capturing,
        ),
        // text_demos.rs
        "\u{F0C9} Strip" => windows::strip_demo(font),
        "\u{F0CE} Table" => windows::table_demo(font),
        "\u{F036} Text Layout" => windows::text_layout(font),
        "\u{F0E2} Undo Redo" => windows::undo_redo(font),
        "\u{F013} Window Options" => windows::window_options(font),
        "\u{F2D0} Modals" => windows::modals_demo(font),
        "\u{F0A4} Multi Touch" => windows::multi_touch(font),
        // 3D Animation title is matched in the caller; fallthrough here is fine.
        _ => windows::coming_soon(),
    }
}
