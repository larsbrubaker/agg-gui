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
    Color, ColorWheelPicker, DrawCtx, Event, EventResult, FlexColumn, Font, Point, Rebuilder, Rect,
    RichCommand, RichDoc, RichEditHandle, RichTextEdit, Size, SizedBox, Stack, Widget,
};

use crate::windows::system_fonts::{
    font_cache_epoch, font_option_index, load_font_by_name, request_font_by_index,
};

/// Which colour the floating picker is currently editing (if any).
#[derive(Clone, Copy, PartialEq, Eq)]
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
        Box::new(SizedBox::new().with_height(320.0).with_child(Box::new(
            RichEditHost::new(editor),
        ))),
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
    let allow_none = kind == PickerKind::Highlight;
    let initial = Color::rgb(0.2, 0.45, 0.88);
    let sel_handle = handle.clone();
    let sel_picker = Rc::clone(picker);
    let cancel_picker = Rc::clone(picker);
    let widget = ColorWheelPicker::new(initial, Arc::clone(font))
        .with_allow_none(allow_none)
        .with_show_alpha(true)
        .with_font_size(12.0)
        .on_select(move |opt| {
            match kind {
                PickerKind::TextColor => {
                    if let Some(c) = opt {
                        sel_handle.exec(&RichCommand::SetTextColor(c));
                    }
                }
                PickerKind::Highlight => {
                    sel_handle.exec(&RichCommand::SetHighlight(opt));
                }
                PickerKind::None => {}
            }
            sel_picker.set(PickerKind::None);
            agg_gui::animation::request_draw();
        })
        .on_cancel(move || {
            cancel_picker.set(PickerKind::None);
            agg_gui::animation::request_draw();
        });
    let title = match kind {
        PickerKind::Highlight => "Highlight colour",
        _ => "Text colour",
    };
    agg_gui::color_wheel_picker_dialog(widget, title)
}

// ── RichEditHost: re-invalidate the editor's layout on font-cache changes ──

/// Thin wrapper owning a [`RichTextEdit`] that watches the font-cache epoch and
/// invalidates the editor's cached layout when a new face loads, so
/// asynchronously-loaded fonts are picked up (the editor otherwise caches its
/// layout against `(width, doc_revision)` only).
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
}
