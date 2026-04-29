use agg_gui::Rect;

// ── Window tiling ──────────────────────────────────────────────────────────────

const WIN_COLS: usize = 4;
const WIN_W: f64 = 360.0;
const WIN_H: f64 = 290.0;
const WIN_GAP_X: f64 = 20.0;
const WIN_GAP_Y: f64 = 20.0;
const WIN_ORIGIN_X: f64 = 20.0;
const WIN_ORIGIN_Y: f64 = 20.0; // from the TOP of the canvas (Y-down thinking)

/// Compute the tiled rect for demo index `i` given canvas `height` (Y-up space).
pub(crate) fn tile_rect(i: usize, canvas_height: f64, win_w: f64, win_h: f64) -> Rect {
    let col = i % WIN_COLS;
    let row = i / WIN_COLS;
    let x = WIN_ORIGIN_X + col as f64 * (WIN_W + WIN_GAP_X);
    let y_down = WIN_ORIGIN_Y + row as f64 * (WIN_H + WIN_GAP_Y);
    let y = (canvas_height - y_down - win_h).max(4.0);
    Rect::new(x, y, win_w, win_h)
}

// ── Demo window list ───────────────────────────────────────────────────────────

pub(crate) struct DemoSpec {
    pub(crate) title: &'static str,
    pub(crate) label: &'static str,
    /// Logical grouping shown as a collapsible section in the sidebar.
    /// Values: "Widgets", "Layout", "Graphics", "Interaction", "Tests", "Tools".
    pub(crate) group: &'static str,
    pub(crate) open: bool,
    pub(crate) win_w: f64,
    pub(crate) win_h: f64,
}

// Exact egui demo list (alphabetical) with egui's original icon prefixes.
// Default open matches egui: Code Example + Widget Gallery.  3D Animation is our
// addition and is open by default as the showcase feature.
// Font Awesome 4 codepoints used as icon prefixes.
// All in the Unicode Private Use Area (U+F000–U+F2FF) so they never
// conflict with regular text characters.
pub(crate) const DEMOS: &[DemoSpec] = &[
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
        title: "\u{F0C9} Menus",
        label: "\u{F0C9} Menus",
        group: "Widgets",
        open: false,
        win_w: 520.0,
        win_h: 320.0,
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
pub(crate) const TESTS: &[DemoSpec] = &[
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
pub(crate) fn find_cube_idx() -> usize {
    DEMOS
        .iter()
        .position(|d| d.title == CUBE_TITLE)
        .expect("DEMOS must contain the 3D Animation entry (CUBE_TITLE)")
}
