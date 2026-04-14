//! Shared demo UI — identical widget tree for both native and WASM targets.
//!
//! Implements the egui-style three-panel layout:
//! - **Left sidebar** (~220 px): heading, "Organize Windows" button, scrollable
//!   checkbox list grouped by category.
//! - **Central canvas**: floating `Window` widgets, one per demo.
//! - **Right panel**: Inspector (toggled via shared `show_inspector` cell).
//!
//! The only platform-specific piece is the 3D cube widget, passed by the caller.

mod sidebar;
mod windows;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    App, Color, DrawCtx, Event, EventResult,
    FlexRow, Font, InspectorNode, InspectorPanel,
    Rect, Size, SizedBox, Stack, Widget, Window,
};

use sidebar::{SidebarEntry, build_sidebar};

// ── Canvas background ──────────────────────────────────────────────────────────

struct CanvasBg { bounds: Rect, children: Vec<Box<dyn Widget>> }

impl CanvasBg {
    fn new() -> Self { Self { bounds: Rect::default(), children: Vec::new() } }
}

impl Widget for CanvasBg {
    fn type_name(&self) -> &'static str { "CanvasBg" }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }
    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        ctx.set_fill_color(Color::rgb(0.10, 0.10, 0.12));
        ctx.begin_path();
        ctx.rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        ctx.fill();
    }
    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }
}

// ── Inspector overlay (right edge of canvas) ──────────────────────────────────

struct InspectorOverlay {
    bounds:         Rect,
    show:           Rc<Cell<bool>>,
    children:       Vec<Box<dyn Widget>>,
}

impl Widget for InspectorOverlay {
    fn type_name(&self) -> &'static str { "InspectorOverlay" }
    fn is_visible(&self) -> bool { self.show.get() }
    fn bounds(&self) -> Rect { self.bounds }
    fn set_bounds(&mut self, b: Rect) { self.bounds = b; }
    fn children(&self) -> &[Box<dyn Widget>] { &self.children }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> { &mut self.children }

    fn layout(&mut self, available: Size) -> Size {
        self.bounds = Rect::new(0.0, 0.0, available.width, available.height);
        let panel_w = 300.0_f64.min(available.width);
        let panel_x = available.width - panel_w;
        if let Some(child) = self.children.first_mut() {
            // Child positioned at the right edge in local coordinates.
            child.set_bounds(Rect::new(panel_x, 0.0, panel_w, available.height));
            child.layout(Size::new(panel_w, available.height));
        }
        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult { EventResult::Ignored }

    fn hit_test(&self, local_pos: agg_gui::Point) -> bool {
        if !self.show.get() { return false; }
        let panel_w = 300.0_f64.min(self.bounds.width);
        let panel_x = self.bounds.width - panel_w;
        local_pos.x >= panel_x && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0 && local_pos.y <= self.bounds.height
    }
}

// ── Window tiling ──────────────────────────────────────────────────────────────

const WIN_COLS:     usize = 4;
const WIN_W:        f64   = 360.0;
const WIN_H:        f64   = 290.0;
const WIN_GAP_X:    f64   = 20.0;
const WIN_GAP_Y:    f64   = 20.0;
const WIN_ORIGIN_X: f64   = 20.0;
const WIN_ORIGIN_Y: f64   = 20.0; // from the TOP of the canvas (Y-down thinking)

/// Compute the tiled rect for demo index `i` given canvas `height` (Y-up space).
fn tile_rect(i: usize, canvas_height: f64, win_w: f64, win_h: f64) -> Rect {
    let col = i % WIN_COLS;
    let row = i / WIN_COLS;
    let x        = WIN_ORIGIN_X + col as f64 * (WIN_W + WIN_GAP_X);
    let y_down   = WIN_ORIGIN_Y + row as f64 * (WIN_H + WIN_GAP_Y);
    let y        = (canvas_height - y_down - win_h).max(4.0);
    Rect::new(x, y, win_w, win_h)
}

// ── Demo window list ───────────────────────────────────────────────────────────

struct DemoSpec {
    title:  &'static str,
    label:  &'static str,
    open:   bool,
    win_w:  f64,
    win_h:  f64,
}

const DEMOS: &[DemoSpec] = &[
    DemoSpec { title: "Widget Gallery",       label: "Widget Gallery",       open: true,  win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Code Editor",          label: "Code Editor",          open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Code Example",         label: "Code Example",         open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Dancing Strings",      label: "Dancing Strings",      open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Drag and Drop",        label: "Drag and Drop",        open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Font Book",            label: "Font Book",            open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Frame Demo",           label: "Frame Demo",           open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Interactive Container",label: "Interactive Container", open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Modals",               label: "Modals",               open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Misc Demo",            label: "Misc Demo",            open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Multi Touch",          label: "Multi Touch",          open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Paint Bezier",         label: "Paint Bezier",         open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Painting",             label: "Painting",             open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Panels",               label: "Panels",               open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Password",             label: "Password",             open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Popups",               label: "Popups",               open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Scene",                label: "Scene",                open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Screenshot",           label: "Screenshot",           open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Scrolling",            label: "Scrolling",            open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Sliders",              label: "Sliders",              open: true,  win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Strip Demo",           label: "Strip Demo",           open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Table Demo",           label: "Table Demo",           open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Text Edit",            label: "Text Edit",            open: true,  win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Text Layout",          label: "Text Layout",          open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Toggle Switch",        label: "Toggle Switch",        open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Tooltips",             label: "Tooltips",             open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Undo / Redo",          label: "Undo / Redo",          open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Window Options",       label: "Window Options",       open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "3D Cube",              label: "3D Cube",              open: true,  win_w: 300.0, win_h: 260.0 },
    DemoSpec { title: "Text (Buffered)",      label: "Text (Buffered)",      open: false, win_w: WIN_W, win_h: WIN_H },
];

const TESTS: &[DemoSpec] = &[
    DemoSpec { title: "Layout Test",  label: "Layout Test",  open: false, win_w: WIN_W, win_h: WIN_H },
    DemoSpec { title: "Highlighting", label: "Highlighting", open: false, win_w: WIN_W, win_h: WIN_H },
];

// ── Index of the 3D Cube in DEMOS (computed once) ─────────────────────────────
const CUBE_IDX: usize = 28; // must match position of "3D Cube" in DEMOS

// ── Public API ─────────────────────────────────────────────────────────────────

/// Build the full demo `App`.
///
/// Returns:
/// - `App` — the root widget tree
/// - `show_inspector` — shared cell toggling the inspector overlay
/// - `inspector_nodes` — snapshot updated each frame when inspector is shown
/// - `hovered_bounds` — hovered-widget rect for the inspector overlay
/// - `cube_visible` — mirrors the 3D Cube window's open state; used by the
///   render loop to switch between `Poll` (animate) and `Wait` (idle)
pub fn build_demo_ui(
    font:        Arc<Font>,
    cube_widget: Box<dyn Widget>,
) -> (App, Rc<Cell<bool>>, Rc<RefCell<Vec<InspectorNode>>>, Rc<RefCell<Option<Rect>>>, Rc<Cell<bool>>) {
    let show_inspector  = Rc::new(Cell::new(false));
    let inspector_nodes = Rc::new(RefCell::new(Vec::<InspectorNode>::new()));
    let hovered_bounds  = Rc::new(RefCell::new(None::<Rect>));

    // ── Sidebar entries ────────────────────────────────────────────────────────
    let demo_entries: Vec<SidebarEntry> = DEMOS.iter()
        .map(|s| SidebarEntry::new(s.label, s.open))
        .collect();
    let test_entries: Vec<SidebarEntry> = TESTS.iter()
        .map(|s| SidebarEntry::new(s.label, s.open))
        .collect();

    // cube_visible shares the same cell as the 3D Cube sidebar entry.
    let cube_visible = Rc::clone(&demo_entries[CUBE_IDX].open);

    // ── Reset cells — one per window ───────────────────────────────────────────
    let all_specs_count = DEMOS.len() + TESTS.len();
    let reset_cells: Vec<Rc<Cell<Option<Rect>>>> = (0..all_specs_count)
        .map(|_| Rc::new(Cell::new(None)))
        .collect();

    // Default canvas height used by tile_rect. 720px is a reasonable fallback;
    // it will look correct on most 1080p+ screens after accounting for the OS bar.
    let default_canvas_h = 720.0_f64;

    // ── Organize Windows callback ──────────────────────────────────────────────
    let rc_for_cb: Vec<_> = reset_cells.iter().map(Rc::clone).collect();
    let on_organize = {
        let specs_w: Vec<f64> = DEMOS.iter().map(|s| s.win_w)
            .chain(TESTS.iter().map(|s| s.win_w))
            .collect();
        let specs_h: Vec<f64> = DEMOS.iter().map(|s| s.win_h)
            .chain(TESTS.iter().map(|s| s.win_h))
            .collect();
        move || {
            for (i, cell) in rc_for_cb.iter().enumerate() {
                let r = tile_rect(i, default_canvas_h, specs_w[i], specs_h[i]);
                cell.set(Some(r));
            }
        }
    };

    // ── Sidebar ────────────────────────────────────────────────────────────────
    let sidebar_widget = build_sidebar(
        Arc::clone(&font),
        &demo_entries,
        &test_entries,
        on_organize,
    );
    let sidebar_panel = SizedBox::new()
        .with_width(220.0)
        .with_child(sidebar_widget);

    // ── Canvas stack (floating windows) ───────────────────────────────────────
    let mut canvas = Stack::new().add(Box::new(CanvasBg::new()));

    // Add DEMO windows.
    for (i, spec) in DEMOS.iter().enumerate() {
        let open_cell  = Rc::clone(&demo_entries[i].open);
        let reset_cell = Rc::clone(&reset_cells[i]);
        let initial    = tile_rect(i, default_canvas_h, spec.win_w, spec.win_h);

        let content: Box<dyn Widget> = if i == CUBE_IDX {
            // Cube content requires the platform-provided cube_widget.
            // Use a placeholder here; replaced immediately after the loop.
            windows::coming_soon()
        } else {
            build_demo_content(spec.title, Arc::clone(&font))
        };

        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(initial.x, initial.y, spec.win_w, spec.win_h))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell);
        canvas = canvas.add(Box::new(win));
    }

    // Replace the placeholder cube window with the real GL cube content.
    // Children layout: [0] = CanvasBg, [1..=30] = DEMOS windows in order.
    {
        let open_cell  = Rc::clone(&demo_entries[CUBE_IDX].open);
        let reset_cell = Rc::clone(&reset_cells[CUBE_IDX]);
        let spec       = &DEMOS[CUBE_IDX];
        let initial    = tile_rect(CUBE_IDX, default_canvas_h, spec.win_w, spec.win_h);
        let content    = windows::cube_content(Arc::clone(&font), cube_widget);
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(initial.x, initial.y, spec.win_w, spec.win_h))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell);
        // Replace index 1 + CUBE_IDX (offset by the CanvasBg at [0]).
        canvas.children_mut()[1 + CUBE_IDX] = Box::new(win);
    }

    // Add TEST windows.
    for (i, spec) in TESTS.iter().enumerate() {
        let total_i    = DEMOS.len() + i;
        let open_cell  = Rc::clone(&test_entries[i].open);
        let reset_cell = Rc::clone(&reset_cells[total_i]);
        let initial    = tile_rect(total_i, default_canvas_h, spec.win_w, spec.win_h);
        let content    = windows::coming_soon();
        let win = Window::new(spec.title, Arc::clone(&font), content)
            .with_bounds(Rect::new(initial.x, initial.y, spec.win_w, spec.win_h))
            .with_visible_cell(open_cell)
            .with_reset_cell(reset_cell);
        canvas = canvas.add(Box::new(win));
    }

    // ── Inspector overlay ──────────────────────────────────────────────────────
    let inspector = InspectorPanel::new(
        Arc::clone(&font),
        Rc::clone(&inspector_nodes),
        Rc::clone(&hovered_bounds),
    );
    let inspector_overlay = InspectorOverlay {
        bounds:   Rect::default(),
        show:     Rc::clone(&show_inspector),
        children: vec![Box::new(inspector)],
    };

    // ── Main area: canvas + inspector overlay ──────────────────────────────────
    let main_area = Stack::new()
        .add(Box::new(canvas))
        .add(Box::new(inspector_overlay));

    // ── Root: sidebar | main area ──────────────────────────────────────────────
    let root = FlexRow::new()
        .with_gap(0.0)
        .add(Box::new(sidebar_panel))
        .add_flex(Box::new(main_area), 1.0);

    (App::new(Box::new(root)), show_inspector, inspector_nodes, hovered_bounds, cube_visible)
}

// ── Demo content dispatcher ────────────────────────────────────────────────────

fn build_demo_content(title: &str, font: Arc<Font>) -> Box<dyn Widget> {
    match title {
        "Widget Gallery" => windows::widget_gallery(font),
        "Code Editor"    => windows::code_editor(font),
        "Password"       => windows::password(font),
        "Sliders"        => windows::sliders(font),
        "Text Edit"      => windows::text_edit(font),
        "Toggle Switch"  => windows::toggle_switch_demo(font),
        "Tooltips"       => windows::tooltips(font),
        _                => windows::coming_soon(),
    }
}
