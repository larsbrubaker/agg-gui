//! RichTextEdit demo window — a formatting toolbar above an interactive,
//! scrollable [`agg_gui::RichTextEdit`], seeded with a small styled document.
//!
//! # Font resolver
//!
//! The library's layout is catalog-agnostic: it asks a resolver to map each
//! run's [`InlineStyle`](agg_gui::InlineStyle) (family + bold/italic) to a
//! concrete [`Font`]. [`make_resolver`] backs that with the demo's system-font
//! catalog ([`crate::windows::system_fonts`]): it prefers a real variant face
//! (`"Nunito Bold"`, `"Arial Italic"`, …) and falls back to the nearest
//! available face, finally the base font.
//!
//! ## Known limitation (flagged honestly)
//!
//! The catalog ships real Italic faces for several families (Arial, Georgia,
//! Liberation, Times, Verdana) and a full Bold / Bold-Italic set only for
//! Nunito. There is **no per-run faux bold/italic** available through this
//! path — synthetic styling in `agg-gui` is a *global* `font_settings` toggle,
//! not a per-face render option — so a bold run in a family without a Bold face
//! renders in the regular weight rather than a synthesised bold. We pick the
//! nearest real variant and leave it at that rather than mutating global font
//! settings. Likewise the catalog loads fonts asynchronously; the editor is
//! wrapped in [`RichEditHost`], which re-invalidates its layout when the font
//! cache epoch advances so a just-loaded face is picked up.

mod toolbar;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::widgets::rich_text::{Block, InlineStyle, ListKind, TextRun};
use agg_gui::{
    CloseReason, Color, ColorWheelPicker, DrawCtx, Event, EventResult, FlexColumn, Font, Point,
    Rebuilder, Rect, RichCommand, RichDoc, RichEditHandle, RichTextEdit, Size, SizedBox, Stack,
    Widget,
};

use crate::windows::system_fonts::{
    font_cache_epoch, font_option_index, load_font_by_name, request_font_by_index,
};

/// Which colour the floating picker is currently editing (if any).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickerKind {
    None,
    TextColor,
    Highlight,
}

/// Request the demo's font loader to fetch `name` and its style variants so the
/// resolver can pick them up. No-op when the System window's cells are absent.
pub(crate) fn request_font(name: &str) {
    let Some(cells) = crate::windows::system::try_cells() else {
        return;
    };
    // Request the family and any catalog entry that is one of its variants
    // ("Arial", "Arial Italic", "Nunito Bold", …).
    for candidate in crate::windows::system_fonts::font_option_names() {
        if candidate == name || candidate.starts_with(&format!("{name} ")) {
            if let Some(idx) = font_option_index(candidate) {
                request_font_by_index(&cells, idx);
            }
        }
    }
}

/// Build a run-style → font resolver backed by the system-font catalog, falling
/// back to `base` when a requested face is not (yet) loaded.
fn make_resolver(base: Arc<Font>) -> agg_gui::SharedResolver {
    Rc::new(move |style: &InlineStyle| {
        let family = style.font_family.as_deref().unwrap_or("Nunito");
        for candidate in variant_candidates(family, style.bold, style.italic) {
            if let Some(font) = load_font_by_name(&candidate) {
                return font;
            }
            // Not loaded yet: enqueue an async fetch for this variant. The
            // catalog loads faces lazily, so without this a bold/italic run
            // would fall back to `base` forever because its face is never
            // requested. When the bytes arrive, `install_font_bytes` bumps the
            // font-cache epoch and `RichEditHost` re-invalidates layout, so the
            // run re-resolves to the real face and re-rasters (see the
            // module-level "font resolver" notes).
            request_font(&candidate);
        }
        Arc::clone(&base)
    })
}

/// Ordered list of catalog face names to try for a family + bold/italic, most
/// specific first, ending with the plain family.
fn variant_candidates(family: &str, bold: bool, italic: bool) -> Vec<String> {
    let mut v = Vec::new();
    match (bold, italic) {
        (true, true) => {
            v.push(format!("{family} Bold Italic"));
            v.push(format!("{family} Bold"));
            v.push(format!("{family} Italic"));
        }
        (true, false) => {
            v.push(format!("{family} Bold"));
            v.push(format!("{family} SemiBold"));
        }
        (false, true) => v.push(format!("{family} Italic")),
        (false, false) => {}
    }
    v.push(family.to_string());
    v
}

// ── Seed document (mirrors the owner's reference image) ────────────────────

fn heading(text: &str) -> Block {
    Block::from_run(TextRun::new(
        text,
        InlineStyle {
            bold: true,
            font_size: Some(24.0),
            ..Default::default()
        },
    ))
}

fn ordered(text: &str) -> Block {
    Block {
        runs: vec![TextRun::plain(text)],
        list: ListKind::Ordered,
        ..Block::new()
    }
}

fn bullet(text: &str) -> Block {
    Block {
        runs: vec![TextRun::plain(text)],
        list: ListKind::Bullet,
        ..Block::new()
    }
}

fn seed_doc() -> RichDoc {
    RichDoc::from_blocks(vec![
        heading("Toolbar"),
        ordered("Toggle bold, italic, underline and strikethrough."),
        ordered("Choose a font family and size."),
        ordered("Set the text colour or a highlight."),
        heading("Links"),
        bullet("Select some text and format it with the toolbar above."),
    ])
}

// ── Window ────────────────────────────────────────────────────────────────

/// Build the RichTextEdit demo window body.
pub fn rich_text_edit(font: Arc<Font>) -> Box<dyn Widget> {
    let resolver = make_resolver(Arc::clone(&font));
    let editor = RichTextEdit::new(seed_doc(), resolver).with_font_size(16.0);
    let handle = editor.handle();

    let picker: Rc<Cell<PickerKind>> = Rc::new(Cell::new(PickerKind::None));

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(12.0)
        .with_panel_bg();

    col.push(
        toolbar::rich_toolbar(&font, handle.clone(), Rc::clone(&picker)),
        0.0,
    );

    // The editor fills the remaining height; its own internal scrollbar handles
    // overflow. Wrapped in a host that refreshes layout when fonts load.
    col.push(
        Box::new(
            SizedBox::new()
                .with_height(320.0)
                .with_child(Box::new(RichEditHost::new(editor))),
        ),
        1.0,
    );

    col.push(
        crate::windows::helpers::source_link("rich_text_demo/mod.rs", Arc::clone(&font)),
        0.0,
    );

    // Overlay the floating colour picker when a swatch button opened it.
    let overlay = color_picker_overlay(&font, handle, Rc::clone(&picker));

    Box::new(
        Stack::new()
            .with_hit_children_only(false)
            .add(Box::new(col))
            .add_aligned(overlay),
    )
}

/// A [`Rebuilder`] that shows a floating [`color_wheel_picker_dialog`] while a
/// swatch is open, applying the chosen colour through the handle on select.
fn color_picker_overlay(
    font: &Arc<Font>,
    handle: RichEditHandle,
    picker: Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    let ver_picker = Rc::clone(&picker);
    let build_font = Arc::clone(font);
    Box::new(Rebuilder::new(
        move || match ver_picker.get() {
            PickerKind::None => 0,
            PickerKind::TextColor => 1,
            PickerKind::Highlight => 2,
        },
        move || build_picker(&build_font, &handle, &picker),
    ))
}

fn build_picker(
    font: &Arc<Font>,
    handle: &RichEditHandle,
    picker: &Rc<Cell<PickerKind>>,
) -> Box<dyn Widget> {
    let kind = picker.get();
    if kind == PickerKind::None {
        // Nothing open — an empty, zero-size layer.
        return Box::new(SizedBox::new().with_width(0.0).with_height(0.0));
    }
    // The "No Color (Pass Through)" checkbox is OFF for both rich-text pickers:
    // the owner reported it as confusing in this context. The core
    // `ColorWheelPicker` still supports it via `with_allow_none` for other hosts.
    let allow_none = false;

    // Snapshot the committed document + selection and suspend undo feeding for
    // the duration of the dialog. Live previewing exec's a fresh `SetTextColor`
    // / `SetHighlight` on every drag frame, so without this the rapid mutations
    // would each seed an undo step and a Cancel would leave a stray entry.
    // `commit_preview` (Select) collapses the whole drag into one undo step;
    // `cancel_preview` (Cancel / Escape / close) restores this exact snapshot.
    handle.begin_preview();

    // Seed the wheel from the selection's current colour so the dialog opens on
    // what's already there. The selection stays visible (the editor paints its
    // band even while unfocused) so the user sees what they are recolouring.
    let common = handle.common_style_of_selection();
    let initial = match kind {
        PickerKind::TextColor => match common.text_color {
            Some(Some(c)) => c,
            // Uniform default / mixed: fall back to a neutral starting colour.
            _ => Color::rgb(0.2, 0.45, 0.88),
        },
        PickerKind::Highlight => match common.highlight {
            Some(Some(c)) => c,
            // No existing / mixed highlight: open on a visible default. With the
            // "No Color" checkbox gone (allow_none off) nothing can clear the
            // picker's pass_through flag, so a zero-alpha seed would strand it
            // emitting None forever and a highlight could never be applied.
            _ => Color::rgb(1.0, 0.92, 0.23),
        },
        PickerKind::None => unreachable!("guarded above"),
    };

    let change_handle = handle.clone();
    let sel_handle = handle.clone();
    let cancel_handle = handle.clone();
    let close_handle = handle.clone();
    let sel_picker = Rc::clone(picker);
    let cancel_picker = Rc::clone(picker);
    let close_picker = Rc::clone(picker);

    let widget = ColorWheelPicker::new(initial, Arc::clone(font))
        .with_allow_none(allow_none)
        .with_show_alpha(true)
        .with_font_size(12.0)
        // Live preview: recolour the selection in context as the user drags.
        .on_change(move |opt| apply_color(&change_handle, kind, opt))
        // Select = commit: apply the final colour, then bank one undo step.
        .on_select(move |opt| {
            apply_color(&sel_handle, kind, opt);
            sel_handle.commit_preview();
            sel_picker.set(PickerKind::None);
            agg_gui::animation::request_draw();
        })
        // Cancel button = restore the pre-dialog snapshot.
        .on_cancel(move || {
            cancel_handle.cancel_preview();
            cancel_picker.set(PickerKind::None);
            agg_gui::animation::request_draw();
        });
    let title = match kind {
        PickerKind::Highlight => "Highlight colour",
        _ => "Text colour",
    };
    // The window's × button, Escape, and click-away close the dialog through a
    // route that bypasses the picker's Cancel button, so forward each to the
    // right teardown — otherwise the preview session would dangle (undo
    // suspended forever, the previewed colour stuck) and the swatch would stay
    // dead. Click-away commits a changed colour as one undo step (Ctrl+Z
    // reverts it); an untouched session closes silently; × / Escape cancel.
    agg_gui::color_wheel_picker_dialog_with_on_close(widget, title, move |reason| {
        match reason {
            CloseReason::ClickAway if close_handle.is_preview_dirty() => {
                close_handle.commit_preview();
            }
            _ => close_handle.cancel_preview(),
        }
        close_picker.set(PickerKind::None);
        agg_gui::animation::request_draw();
    })
}

/// Apply a picker colour to the selection for the given `kind`. Text colour
/// only applies a concrete colour (a text run always has one); highlight
/// forwards `opt` through `SetHighlight`, which clears on `None` (the picker no
/// longer offers a "No Color" choice here, so `opt` is always `Some`).
fn apply_color(handle: &RichEditHandle, kind: PickerKind, opt: Option<Color>) {
    match kind {
        PickerKind::TextColor => {
            if let Some(c) = opt {
                handle.exec(&RichCommand::SetTextColor(c));
            }
        }
        PickerKind::Highlight => handle.exec(&RichCommand::SetHighlight(opt)),
        PickerKind::None => {}
    }
}

// ── RichEditHost: re-invalidate the editor's layout on font-cache changes ──

/// Thin wrapper owning a [`RichTextEdit`] that watches the font-cache epoch and
/// invalidates the editor's cached layout when a new face loads, so
/// asynchronously-loaded fonts are picked up (the editor otherwise caches its
/// layout against `(width, doc_revision)` only).
///
/// It deliberately does **not** forward layout-props (margin, anchors,
/// min/max size): the editor is sized by its enclosing `SizedBox`, so the
/// wrapper only needs to pass through layout, paint, events and focus.
struct RichEditHost {
    editor: RichTextEdit,
    last_epoch: Cell<u64>,
    children: Vec<Box<dyn Widget>>, // always empty
}

impl RichEditHost {
    fn new(editor: RichTextEdit) -> Self {
        Self {
            editor,
            last_epoch: Cell::new(font_cache_epoch()),
            children: Vec::new(),
        }
    }
}

impl Widget for RichEditHost {
    fn type_name(&self) -> &'static str {
        "RichEditHost"
    }
    fn bounds(&self) -> Rect {
        self.editor.bounds()
    }
    fn set_bounds(&mut self, b: Rect) {
        self.editor.set_bounds(b);
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn is_focusable(&self) -> bool {
        self.editor.is_focusable()
    }
    fn focus_id(&self) -> Option<agg_gui::focus::FocusId> {
        self.editor.focus_id()
    }
    fn accepts_text_input(&self) -> bool {
        self.editor.accepts_text_input()
    }
    fn text_input_value(&self) -> Option<String> {
        self.editor.text_input_value()
    }
    fn needs_draw(&self) -> bool {
        self.editor.needs_draw()
    }

    // Forward the backbuffer hooks so the framework runs the cached LCD/RGBA
    // path (`paint_subtree_backbuffered`) for the editor. Without this the host
    // node returns `None`, the framework paints it directly, and the editor's
    // own cache never engages — it re-`fill_text`s every frame. The framework
    // renders `RichEditHost::paint` (which delegates to `editor.paint`) into the
    // offscreen buffer and blits it, then runs `paint_overlay` (caret + bar).
    fn backbuffer_cache_mut(&mut self) -> Option<&mut agg_gui::widget::BackbufferCache> {
        self.editor.backbuffer_cache_mut()
    }
    fn backbuffer_mode(&self) -> agg_gui::widget::BackbufferMode {
        self.editor.backbuffer_mode()
    }

    fn measure_min_height(&self, available_w: f64) -> f64 {
        self.editor.measure_min_height(available_w)
    }

    fn layout(&mut self, available: Size) -> Size {
        let epoch = font_cache_epoch();
        if epoch != self.last_epoch.get() {
            self.last_epoch.set(epoch);
            self.editor.invalidate_layout();
        }
        self.editor.layout(available)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        self.editor.paint(ctx);
    }
    fn paint_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.editor.paint_overlay(ctx);
    }
    fn on_event(&mut self, event: &Event) -> EventResult {
        self.editor.on_event(event)
    }
    fn hit_test(&self, local: Point) -> bool {
        self.editor.hit_test(local)
    }

    // Forward the right-click context-menu hooks. The editor implements both
    // (returning `context_menu.is_open()` / painting the menu at app level), but
    // the trait defaults are `false` / no-op: without forwarding, an open menu
    // never goes modal (so clicks don't route to it) and never paints (so it's
    // invisible) — which is exactly why this demo's menu didn't appear while the
    // unwrapped TextEdit/Code Editor menus did.
    fn has_active_modal(&self) -> bool {
        self.editor.has_active_modal()
    }
    fn paint_global_overlay(&mut self, ctx: &mut dyn DrawCtx) {
        self.editor.paint_global_overlay(ctx);
    }
    // Forward the caret-blink redraw deadline. The default walks children (this
    // host has none) and returns `None`, so the focused editor's blink would
    // never schedule a wake through the host.
    fn next_draw_deadline(&self) -> Option<web_time::Instant> {
        self.editor.next_draw_deadline()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/CascadiaCode.ttf");
    /// A second, distinct real face used to stand in for a loaded variant so the
    /// resolver test can prove it returns the installed face rather than `base`.
    const VARIANT_FONT: &[u8] = include_bytes!("../../../../demo/assets/Poppins.ttf");

    /// Painting the host through the framework must ENGAGE the editor's cached
    /// LCD/RGBA backbuffer. The host forwards `backbuffer_cache_mut`, so
    /// `paint_subtree_backbuffered` rasterises the editor (bg + selection +
    /// styled runs) into an offscreen buffer whose pixels then populate the
    /// cache. If the host stopped forwarding, the framework would paint it
    /// directly, the editor would `fill_text` every frame, and the cache would
    /// stay empty — this test pins the wiring so it can't silently regress.
    #[test]
    fn host_engages_editor_backbuffer_cache() {
        // Standard density so LCD is available; either mode still populates
        // pixels (the assertion only cares that the cache path ran).
        agg_gui::device_scale::set_device_scale(1.0);
        agg_gui::ux_scale::set_ux_scale(1.0);

        let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
        let resolver = make_resolver(Arc::clone(&font));
        let editor = RichTextEdit::new(seed_doc(), resolver).with_font_size(16.0);
        let mut host = RichEditHost::new(editor);

        host.layout(Size::new(400.0, 300.0));

        // Forwarding wired up: the host node exposes the editor's cache.
        assert!(
            host.backbuffer_cache_mut().is_some(),
            "RichEditHost must forward backbuffer_cache_mut to the editor"
        );

        let mut fb = agg_gui::Framebuffer::new(400, 300);
        {
            let mut ctx = agg_gui::GfxCtx::new(&mut fb);
            agg_gui::widget::paint_subtree(&mut host, &mut ctx);
        }

        let engaged = host
            .backbuffer_cache_mut()
            .map(|c| c.pixels.is_some())
            .unwrap_or(false);
        assert!(
            engaged,
            "editor backbuffer cache must populate after a framework paint — \
             the cached LCD/RGBA path did not engage"
        );
    }

    /// Right-clicking the editor *through the host wrapper* must open the
    /// context menu AND make the host report an active modal. The editor
    /// implements `has_active_modal` (returns `context_menu.is_open()`), but the
    /// trait default is `false`; if the host doesn't forward it, the open menu
    /// never captures events and — because `paint_global_overlay` is likewise a
    /// no-op by default — never paints. This pins the forwarding so the menu the
    /// demo's users right-click for can't silently vanish again.
    #[test]
    fn host_forwards_context_menu_modal_state() {
        agg_gui::widget::set_current_viewport(Size::new(800.0, 600.0));
        let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
        let resolver = make_resolver(Arc::clone(&font));
        let doc = RichDoc::from_blocks(vec![Block::from_run(TextRun::plain("hello world"))]);
        let editor = RichTextEdit::new(doc, resolver).with_font_size(16.0);
        let mut host = RichEditHost::new(editor);

        host.layout(Size::new(400.0, 120.0));
        host.on_event(&Event::FocusGained);

        let r = host.on_event(&Event::MouseDown {
            pos: Point::new(20.0, 60.0),
            button: MouseButton::Right,
            modifiers: Modifiers::default(),
        });
        assert_eq!(
            r,
            EventResult::Consumed,
            "a right-click on the editor must be consumed (opens the menu)"
        );
        assert!(
            host.has_active_modal(),
            "RichEditHost must forward has_active_modal so the open context menu \
             captures events; the trait default false leaves the menu inert"
        );
    }

    // ── Live preview: every dialog-dismissal route unwinds the session ──────
    //
    // The window's × button and Escape are a close route SEPARATE from the
    // picker's Cancel button. If they don't cancel the preview, the session
    // dangles forever (undo suspended, preview colour stuck, swatch dead).
    // These drive the REAL demo overlay through the App pipeline.

    use agg_gui::{App, Key, Modifiers, MouseButton};

    /// World-space rect of the dialog window inside the aligned overlay slot
    /// (Stack → Rebuilder → Window). Mirrors the accumulation the framework's
    /// hit-test does when routing a click down the tree.
    fn dialog_window_world(app: &App) -> Rect {
        let rb = &app.root().children()[1];
        let win = &rb.children()[0];
        let rbb = rb.bounds();
        let wb = win.bounds();
        Rect::new(rbb.x + wb.x, rbb.y + wb.y, wb.width, wb.height)
    }

    /// Build the real overlay over an editor whose whole (red) document is
    /// selected, open the text-colour dialog, and preview blue live — exactly
    /// what a wheel drag does. Returns the app, a shared handle, the picker
    /// cell, and the original colour to restore to.
    fn open_text_color_preview() -> (App, RichEditHandle, Rc<Cell<PickerKind>>, Color) {
        agg_gui::device_scale::set_device_scale(1.0);
        agg_gui::ux_scale::set_ux_scale(1.0);

        let font = Arc::new(Font::from_slice(TEST_FONT).expect("test font must load"));
        let red = Color::rgb(1.0, 0.0, 0.0);
        let doc = RichDoc::from_blocks(vec![Block::from_run(TextRun::new(
            "hello",
            InlineStyle {
                text_color: Some(red),
                ..Default::default()
            },
        ))]);
        let mut editor =
            RichTextEdit::new(doc, make_resolver(Arc::clone(&font))).with_font_size(16.0);
        let handle = editor.handle();
        // Select the whole doc so a colour preview actually mutates runs.
        editor.on_event(&Event::KeyDown {
            key: Key::Char('a'),
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        });

        let picker_cell = Rc::new(Cell::new(PickerKind::None));
        let overlay = color_picker_overlay(&font, handle.clone(), Rc::clone(&picker_cell));
        let editor_holder = SizedBox::new()
            .with_width(400.0)
            .with_height(300.0)
            .with_child(Box::new(RichEditHost::new(editor)));
        let root = Stack::new()
            .with_hit_children_only(false)
            .add(Box::new(editor_holder))
            .add_aligned(overlay);
        let mut app = App::new(Box::new(root));

        // Open the dialog (Rebuilder rebuilds → begin_preview) and drag to blue.
        picker_cell.set(PickerKind::TextColor);
        app.layout(Size::new(420.0, 520.0));
        assert!(
            handle.is_previewing(),
            "opening the dialog begins a preview"
        );
        handle.exec(&RichCommand::SetTextColor(Color::rgb(0.0, 0.0, 1.0)));
        app.layout(Size::new(420.0, 520.0));
        assert_eq!(
            handle.common_style_of_selection().text_color,
            Some(Some(Color::rgb(0.0, 0.0, 1.0))),
            "preview must mutate the document live"
        );

        (app, handle, picker_cell, red)
    }

    fn assert_preview_unwound(
        handle: &RichEditHandle,
        picker_cell: &Rc<Cell<PickerKind>>,
        red: Color,
    ) {
        assert!(
            !handle.is_previewing(),
            "closing the dialog must end the preview session (undo would stay dead otherwise)"
        );
        assert_eq!(
            picker_cell.get(),
            PickerKind::None,
            "closing the dialog must clear the picker cell so the swatch works again"
        );
        assert_eq!(
            handle.common_style_of_selection().text_color,
            Some(Some(red)),
            "closing the dialog must restore the pre-dialog colour"
        );
    }

    #[test]
    fn window_close_button_cancels_live_preview() {
        let (mut app, handle, picker_cell, red) = open_text_color_preview();

        // Click the window's × button.
        let wb = dialog_window_world(&app);
        let close = Point::new(wb.x + wb.width - 10.0, wb.y + wb.height - 14.0);
        let screen_y = 520.0 - close.y;
        app.on_mouse_down(close.x, screen_y, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(close.x, screen_y, MouseButton::Left, Modifiers::default());
        app.layout(Size::new(420.0, 520.0));

        assert_preview_unwound(&handle, &picker_cell, red);
    }

    #[test]
    fn escape_cancels_live_preview() {
        let (mut app, handle, picker_cell, red) = open_text_color_preview();

        app.on_key_down(Key::Escape, Modifiers::default());
        app.layout(Size::new(420.0, 520.0));

        assert_preview_unwound(&handle, &picker_cell, red);
    }

    /// Regression for the reported bug: a bold run rendered in the regular
    /// weight even though the catalog ships a real bold face. Root cause was the
    /// resolver returning `base` on a cache miss without ever requesting the
    /// variant load, so its bytes were never fetched. Once the variant is loaded
    /// the resolver must return the real bold face rather than falling back.
    #[test]
    fn bold_run_resolves_to_variant_once_loaded() {
        use crate::windows::system_fonts::install_font_bytes;

        // A family name unique to this test keeps the thread-local font cache
        // isolated from sibling tests (cargo reuses worker threads).
        let family = "RtBoldResolveTest";
        let base = Arc::new(Font::from_slice(TEST_FONT).expect("base font must load"));
        let resolver = make_resolver(Arc::clone(&base));
        let style = InlineStyle {
            bold: true,
            font_family: Some(family.to_string()),
            ..Default::default()
        };

        // Before the variant loads there is nothing to resolve to but `base`.
        let before = resolver(&style);
        assert!(
            Arc::ptr_eq(&before, &base),
            "an unloaded bold face must fall back to the base font"
        );

        // Install the bold face (any distinct real font under the variant name).
        install_font_bytes(&format!("{family} Bold"), VARIANT_FONT.to_vec(), None, None)
            .expect("installing the bold variant must succeed");

        // Now the resolver must pick the real bold face, not the regular base.
        let after = resolver(&style);
        assert!(
            !Arc::ptr_eq(&after, &base),
            "a loaded bold face must resolve to the variant, not the base font"
        );
    }

    /// The fix's core mechanism: resolving a run whose variant face is not yet
    /// loaded must enqueue an async fetch for it (via the platform hook and the
    /// pending-request queue). Without this the bold bytes are never requested
    /// and the run stays in the regular weight forever.
    #[test]
    fn unloaded_bold_variant_enqueues_font_request() {
        use crate::windows::system::{init_cells, SystemCells};
        use crate::windows::system_fonts::take_pending_font_request;
        use std::cell::{Cell, RefCell};

        // Clear any residual queue state from earlier work on this thread.
        while take_pending_font_request().is_some() {}

        // Register cells with a recording font-request hook; only `platform`
        // matters here, the rest are placeholders.
        let requested: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let rec = Rc::clone(&requested);
        let platform = crate::PlatformHooks::native(0, || {})
            .with_font_requester(move |name, _path| rec.borrow_mut().push(name.to_string()));
        init_cells(SystemCells {
            font_name: Rc::new(RefCell::new(None)),
            font_index: Rc::new(Cell::new(0)),
            font_size_scale: Rc::new(Cell::new(1.0)),
            lcd_enabled: Rc::new(Cell::new(false)),
            hinting_enabled: Rc::new(Cell::new(false)),
            gamma: Rc::new(Cell::new(1.0)),
            width_scale: Rc::new(Cell::new(1.0)),
            interval: Rc::new(Cell::new(0.0)),
            faux_weight: Rc::new(Cell::new(0.0)),
            faux_italic: Rc::new(Cell::new(0.0)),
            primary_weight: Rc::new(Cell::new(1.0 / 3.0)),
            system_tab: Rc::new(Cell::new(0)),
            platform,
        });

        let base = Arc::new(Font::from_slice(TEST_FONT).expect("base font must load"));
        let resolver = make_resolver(Arc::clone(&base));

        // Default family (Nunito) + bold, with the Bold face not loaded.
        let _ = resolver(&InlineStyle {
            bold: true,
            ..Default::default()
        });

        let mut names = Vec::new();
        while let Some((name, _path)) = take_pending_font_request() {
            names.push(name);
        }
        assert!(
            names.iter().any(|n| n == "Nunito Bold"),
            "resolving a bold run must enqueue a fetch for the Nunito Bold face; got {names:?}"
        );
        assert!(
            requested.borrow().iter().any(|n| n == "Nunito Bold"),
            "the platform font-request hook must be invoked for Nunito Bold"
        );
    }
}
