#![allow(unused_imports)]
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::framebuffer::unpremultiply_rgba_inplace;
use agg_gui::widget::paint_subtree;
use agg_gui::{
    render_svg_at_size, render_svg_to_framebuffer_at_size, render_svg_to_lcd_buffer_at_size,
    set_cursor_icon, Color, Container, CursorIcon, DrawCtx, Event, EventResult, FlexColumn,
    FlexRow, Font, Hyperlink, Label, MouseButton, Point, Rect, Resize, ScrollBarVisibility,
    ScrollView, Separator, Size, SizedBox, TextArea, TextField, Visuals, Widget,
};

// ---------------------------------------------------------------------------
// Window Resize Test
// ---------------------------------------------------------------------------

// Short and long Lorem Ipsum strings — mirrors the egui reference constants.
const LOREM_IPSUM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing \
elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim \
ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea \
commodo consequat.";

const LOREM_IPSUM_LONG: &str = "Lorem ipsum dolor sit amet, consectetur \
adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip \
ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit \
esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat cupidatat non \
proident, sunt in culpa qui officia deserunt mollit anim id est laborum.\n\n\
Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
doloremque laudantium, totam rem aperiam, eaque ipsa quae ab illo inventore \
veritatis et quasi architecto beatae vitae dicta sunt explicabo. Nemo enim ipsam \
voluptatem quia voluptas sit aspernatur aut odit aut fugit, sed quia consequuntur \
magni dolores eos qui ratione voluptatem sequi nesciunt.\n\n\
At vero eos et accusamus et iusto odio dignissimos ducimus qui blanditiis \
praesentium voluptatum deleniti atque corrupti quos dolores et quas molestias \
excepturi sint occaecati cupiditate non provident, similique sunt in culpa qui \
officia deserunt mollitia animi, id est laborum et dolorum fuga.";

/// One entry returned from [`window_resize_sub_windows`].  The caller
/// wraps `content` in a `Window` and applies the flags to its builder
/// (`with_auto_size`, `with_resizable` / `with_resizable_axes`) so each
/// sub-window demonstrates the exact egui behaviour it's named after.
pub struct ResizeTestWindow {
    pub title: String,
    pub content: Box<dyn Widget>,
    pub initial_rect: Rect,
    /// Window fits tightly to its content; ignores `resizable_*`.
    pub auto_size: bool,
    /// Master user-resize toggle.  `false` → no handles active.
    pub resizable: bool,
    /// Axis-specific locks (only consulted when `resizable` is `true`).
    pub resizable_h: bool,
    pub resizable_v: bool,
    /// Wrap content in a built-in vertical `ScrollView` at window
    /// build time.  Matches egui's `Window::vscroll(true)`.
    pub vscroll: bool,
    /// Resize floor + ceiling follow content natural height.
    /// Matches egui's no-scroll-no-clip-no-whitespace contract for
    /// W4 (window snaps to content height in both directions).
    pub tight_fit: bool,
    /// Resize FLOOR only follows content height; user can pull the
    /// window taller (whitespace below).  Used for W5 where a
    /// flex-fill `TextArea` absorbs extra space.
    pub floor_fit: bool,
}

impl ResizeTestWindow {
    fn new(title: &str, content: Box<dyn Widget>, initial_rect: Rect) -> Self {
        Self {
            title: title.into(),
            content,
            initial_rect,
            auto_size: false,
            resizable: true,
            resizable_h: true,
            resizable_v: true,
            vscroll: false,
            tight_fit: false,
            floor_fit: false,
        }
    }
    fn auto_sized(mut self) -> Self {
        self.auto_size = true;
        self.resizable = false;
        self
    }
    fn with_vscroll(mut self) -> Self {
        self.vscroll = true;
        self
    }
    fn with_tight_fit(mut self) -> Self {
        self.tight_fit = true;
        self
    }
    fn with_floor_fit(mut self) -> Self {
        self.floor_fit = true;
        self
    }
}

/// Helper: a small "(source code)" hyperlink that opens the test
/// source file in a browser.  Callers push this as the final child
/// of each sub-window's root column, just like egui's demo.
///
/// Delegates to the shared [`crate::windows::helpers::source_link`] so the
/// URL tracks the real implementation file (`tests/resize.rs`) after the
/// `tests.rs` module split, matching egui's `egui_github_link_file!` footer.
fn source_link(font: Arc<Font>) -> Box<dyn Widget> {
    crate::windows::helpers::source_link("tests/resize.rs", font)
}

/// Build the six sub-windows for the Window Resize Test, mirroring
/// egui's `crates/egui_demo_lib/src/demo/tests/window_resize_test.rs`
/// one-for-one.  Each window demonstrates a specific resize + scroll +
/// content-fill combination; the caller applies the returned flags to
/// its `Window` wrapper so those behaviours surface correctly.
pub fn window_resize_sub_windows(font: Arc<Font>) -> Vec<ResizeTestWindow> {
    // Initial rects in Y-up canvas coordinates (default_canvas_h ≈ 720).
    // Staggered 3 × 2 so the windows are visible on a 1280×720 screen.
    // Ordering matches egui's source, not layout order on screen.
    let rects: &[Rect] = &[
        Rect::new(30.0, 100.0, 360.0, 240.0),  // 1. ↔ auto-sized
        Rect::new(410.0, 100.0, 300.0, 290.0), // 2. ↔ resizable + scroll
        Rect::new(730.0, 100.0, 300.0, 290.0), // 3. ↔ resizable + embedded scroll
        Rect::new(30.0, 410.0, 300.0, 290.0),  // 4. ↔ resizable without scroll
        Rect::new(410.0, 410.0, 300.0, 290.0), // 5. ↔ resizable with TextEdit
        Rect::new(730.0, 410.0, 250.0, 150.0), // 6. ↔ freely resized
    ];

    let mut out: Vec<ResizeTestWindow> = Vec::new();

    // ── 1. ↔ auto-sized ──────────────────────────────────────────────────────
    //
    // Outer window is `auto_sized()`, so it fits its content each
    // frame and disables its own user-drag resize.  The inner area is
    // the Stage-3 `Resize` widget — a user-draggable nested region.
    // Dragging its SE grip:
    //   * Grows the Resize past its content's natural size (the
    //     `Resize` widget enforces content-natural as a min, so it
    //     can never shrink past what fits).
    //   * Pushes the surrounding Window wider when the Resize
    //     demands more width than the current window inner area —
    //     via `FlexColumn::with_fit_width(true)` reporting the
    //     widest child's natural size up through `Window::auto_size`.
    //
    // Styling: `Resize` already draws its own rounded outline; no
    // `Container` wrapper needed (previously we had both, giving a
    // visible double outline).
    {
        let mut root = FlexColumn::new()
            .with_gap(6.0)
            .with_padding(10.0)
            .with_panel_bg()
            .with_fit_width(true);
        // Keep explanatory text from dictating the auto-sized
        // window's width: W1 should track the inner Resize width
        // plus padding, while fixed-width wrapped prose can grow
        // taller instead of leaving stale right-side whitespace.
        root.push(
            Box::new(
                SizedBox::new().with_width(320.0).with_child(Box::new(
                    Label::new(
                        "This window will auto-size based on its contents.",
                        Arc::clone(&font),
                    )
                    .with_font_size(12.0)
                    .with_wrap(true),
                )),
            ),
            0.0,
        );
        root.push(
            Box::new(Label::new("Resize this area:", Arc::clone(&font)).with_font_size(14.0)),
            0.0,
        );
        // The lorem ipsum INSIDE the Resize widget still wraps so it
        // reshapes as the user narrows / widens the Resize.  The
        // Resize widget enforces a content-natural minimum so the
        // wrapped text can never be clipped.  `top_anchor` keeps the
        // text at the top of the Resize frame when the user pulls it
        // taller — without this, FlexColumn's default natural-anchor
        // would leave the text pinned to the BOTTOM of the frame
        // with whitespace above (the bug visible in image #24).
        let mut inner = FlexColumn::new()
            .with_gap(4.0)
            .with_padding(8.0)
            .with_fit_width(true)
            .with_top_anchor(true);
        inner.push(
            Box::new(
                Label::new(LOREM_IPSUM, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        // No explicit max_size_hint here — we want the user to be
        // able to drag the inner Resize all the way to the canvas
        // extent, letting the outer auto-sized Window grow with it.
        // The `Window::auto_size` clamp to `available.width` caps
        // final growth at the surrounding layout's inner width.
        root.push(
            Box::new(
                Resize::new(Box::new(inner))
                    .with_default_size(Size::new(320.0, 120.0))
                    .with_min_size_hint(Size::new(120.0, 60.0))
                    .with_max_size_hint(Size::new(4000.0, 3000.0)),
            ),
            0.0,
        );
        root.push(
            Box::new(Label::new("Resize the above area!", Arc::clone(&font)).with_font_size(14.0)),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new("↔ auto-sized", Box::new(root), rects[0]).auto_sized());
    }

    // ── 2. ↔ resizable + scroll ──────────────────────────────────────────────
    //
    // Window-level vscroll (egui's `.vscroll(true)`).  No manual
    // ScrollView in the content tree — the `Window::with_vscroll(true)`
    // call in `lib.rs` (Stage 2) wraps `root` itself in a vertical
    // ScrollView at builder time.  The inner content is a single
    // overflowing FlexColumn so the scroll bar has range.
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window is resizable and has a scroll area. You can shrink it \
             to any size.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(Separator::horizontal()), 0.0);
        root.push(
            Box::new(
                Label::new(LOREM_IPSUM_LONG, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(
            ResizeTestWindow::new("↔ resizable + scroll", Box::new(root), rects[1]).with_vscroll(),
        );
    }

    // ── 3. ↔ resizable + embedded scroll ────────────────────────────────────
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window is resizable but has no built-in scroll area.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(
            Box::new(
                Label::new(
                    "However, we have a sub-region with a scroll bar:",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(Separator::horizontal()), 0.0);
        let long2 = format!("{}\n\n{}", LOREM_IPSUM_LONG, LOREM_IPSUM_LONG);
        let mut inner = FlexColumn::new().with_gap(4.0).with_padding(4.0);
        inner.push(
            Box::new(
                Label::new(&long2, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(ScrollView::new(Box::new(inner))), 1.0);
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(ResizeTestWindow::new(
            "↔ resizable + embedded scroll",
            Box::new(root),
            rects[2],
        ));
    }

    // ── 4. ↔ resizable without scroll ───────────────────────────────────────
    //
    // egui never clips window content and has no whitespace to add, so the
    // user can only shrink down to a size that still fits all content.
    // Stage 5 enforces that at the library level: `with_tight_content_fit`
    // makes the resize clamp floor honour the content's natural height
    // observed in the last layout.
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window is resizable but has no scroll area. This means it \
             can only be resized to a size where all the contents is visible.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(
            Box::new(
                Label::new(
                    "agg-gui will not clip the contents of a window, nor add \
             whitespace to it.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(Box::new(Separator::horizontal()), 0.0);
        root.push(
            Box::new(
                Label::new(LOREM_IPSUM, Arc::clone(&font))
                    .with_font_size(11.5)
                    .with_wrap(true),
            ),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(
            ResizeTestWindow::new("↔ resizable without scroll", Box::new(root), rects[3])
                .with_tight_fit(),
        );
    }

    // ── 5. ↔ resizable with TextEdit ────────────────────────────────────────
    //
    // Stage-4 multiline `TextArea` fills the remaining space — so as
    // the user resizes the window, the editor follows both axes.
    // Pre-seeded with lorem ipsum so wrap + selection are immediately
    // demonstrable.  `tight_fit` enforces the egui contract: window
    // height ≥ TextArea content height, so wrapping text never falls
    // off-screen.
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "Shows how you can fill an area with a widget.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(
            Box::new(
                TextArea::new(Arc::clone(&font))
                    .with_font_size(12.5)
                    // egui seeds this with LOREM_IPSUM_LONG, but egui's
                    // TextEdit::multiline scrolls internally so the long
                    // text stays inside the fixed-height window.  agg-gui's
                    // TextArea has no internal scroll and W5 is `floor_fit`
                    // (window grows to fit ALL content, never clips), so a
                    // LONG seed would force the window taller than its
                    // on-screen slot / the canvas — breaking the W5
                    // "no off-screen text" contract.  Keep the short seed
                    // until TextArea gains internal vertical scrolling.
                    .with_text(LOREM_IPSUM),
            ),
            1.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        out.push(
            ResizeTestWindow::new("↔ resizable with TextEdit", Box::new(root), rects[4])
                .with_floor_fit(),
        );
    }

    // ── 6. ↔ freely resized ─────────────────────────────────────────────────
    {
        let mut root = FlexColumn::new()
            .with_gap(8.0)
            .with_padding(10.0)
            .with_panel_bg();
        root.push(
            Box::new(
                Label::new(
                    "This window has empty space that fills up the available space, \
             preventing auto-shrink.",
                    Arc::clone(&font),
                )
                .with_font_size(12.0)
                .with_wrap(true),
            ),
            0.0,
        );
        root.push(source_link(Arc::clone(&font)), 0.0);
        root.push(Box::new(SizedBox::new()), 1.0);
        out.push(ResizeTestWindow::new(
            "↔ freely resized",
            Box::new(root),
            rects[5],
        ));
    }

    out
}
