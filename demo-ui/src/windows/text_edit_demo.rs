//! TextEdit demo — a close reproduction of egui's `text_edit.rs`
//! (`crates/egui_demo_lib/src/demo/text_edit.rs`).
//!
//! Shows the "advanced usage" of a multiline text editor: hint text,
//! horizontal / vertical content alignment selectors, a prefix search icon and
//! suffix clear button around the field, a live "Selected text:" readout, the
//! Ctrl/Cmd+Y "toggle case of selection" shortcut, and "Move cursor to the:
//! start / end" buttons.
//!
//! All of this is driven by the library capabilities added to
//! [`agg_gui::TextArea`]: a shared [`agg_gui::TextEditState`] handle (read the
//! selection, mutate/clear the text from outside), live alignment cells, a
//! programmatic focus id, and a pre-default key-chord interceptor. The demo
//! tree is built once — alignment flips through cells and the selection readout
//! rebuilds through a [`Rebuilder`] keyed on the selection, so nothing here
//! recreates the editor and loses its edit state.
//!
//! Deviation from egui: egui renders the search-icon prefix and ❌ suffix
//! *inside* the TextEdit frame as aligned atoms. We approximate that with a
//! `FlexRow` (icon · field · clear button) since `TextArea` has no atom slots;
//! the icons are top-anchored rather than tracking the vertical-align setting.
//! Icons use Font Awesome glyphs (search `\u{F002}`, xmark `\u{F00D}`) per the
//! project's icon convention, in place of egui's 🔎 / ❌ emoji.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::{
    Button, Color, FlexColumn, FlexRow, Font, HAnchor, Hyperlink, Key, Label, Modifiers, Rebuilder,
    Separator, SizedBox, TextArea, TextEditState, TextField, TextHAlign, TextVAlign, VAnchor,
    Widget,
};

/// egui's default buffer contents.
const DEFAULT_TEXT: &str = "Edit this text";

/// Font Awesome glyphs standing in for egui's emoji prefix/suffix.
const ICON_SEARCH: &str = "\u{F002}";
const ICON_CLEAR: &str = "\u{F00D}";

/// Stable focus id so the "start"/"end" buttons can hand keyboard focus back to
/// the editor via [`agg_gui::focus::request_focus`].
const TEXT_EDIT_FOCUS_ID: agg_gui::focus::FocusId = 0x7E00_0001;

const SOURCE_URL: &str =
    "https://github.com/larsbrubaker/agg-gui/blob/main/demo-ui/src/windows/text_edit_demo.rs";

/// Tint used for inline `code`-style snippets (egui renders these monospace on
/// a faint background; we approximate with a distinct colour).
fn code_color() -> Color {
    Color::rgba(0.92, 0.62, 0.34, 1.0)
}

/// Dim tint for the disabled Ctrl+Y hint (no selection).
fn dim_color() -> Color {
    Color::rgba(0.5, 0.5, 0.5, 1.0)
}

/// Build the TextEdit demo.
pub fn text_edit(font: Arc<Font>) -> Box<dyn Widget> {
    // Shared, externally-mutable edit state seeded with egui's default text.
    let state = Rc::new(RefCell::new(TextEditState {
        text: DEFAULT_TEXT.to_string(),
        cursor: DEFAULT_TEXT.len(),
        anchor: DEFAULT_TEXT.len(),
        epoch: 0,
    }));
    // Live alignment bindings — flipped by the selector buttons below.
    let halign = Rc::new(Cell::new(TextHAlign::Left));
    let valign = Rc::new(Cell::new(TextVAlign::Top));

    let mut col = FlexColumn::new()
        .with_gap(10.0)
        .with_padding(16.0)
        .with_panel_bg();

    // Centered "(source code)" link, like egui's github-link header.
    col.push(source_link(Arc::clone(&font)), 0.0);

    // "Advanced usage of `TextEdit`."
    col.push(advanced_usage_row(&font), 0.0);

    // Horizontal / vertical alignment selectors.
    col.push(halign_row(&font, &halign), 0.0);
    col.push(valign_row(&font, &valign), 0.0);

    // The editor itself, framed by a prefix icon and a suffix clear button.
    col.push(editor_row(&font, &state, &halign, &valign), 0.0);

    // Live "Selected text:" readout + the Ctrl+Y hint (enabled only when a
    // selection exists). Rebuilt when the selection or text changes.
    col.push(readout_block(&font, &state), 0.0);

    // "Move cursor to the: start / end".
    col.push(move_cursor_row(&font, &state), 0.0);

    // ── Our extra: a read-only field variant (kept from the original demo) ──
    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(Label::new("Read-only field", Arc::clone(&font)).with_font_size(12.0)),
        0.0,
    );
    col.push(
        Box::new(
            SizedBox::new().with_height(32.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_text("This field is read-only")
                    .with_read_only(true),
            )),
        ),
        0.0,
    );

    Box::new(col)
}

/// Centered "(source code)" hyperlink footer/header.
fn source_link(font: Arc<Font>) -> Box<dyn Widget> {
    Box::new(
        Hyperlink::new("(source code)", font)
            .with_font_size(11.0)
            .with_h_anchor(HAnchor::CENTER)
            .on_click(|| crate::url::open_url(SOURCE_URL)),
    )
}

/// "Advanced usage of `TextEdit`." with the type name rendered code-style.
fn advanced_usage_row(font: &Arc<Font>) -> Box<dyn Widget> {
    Box::new(
        FlexRow::new()
            .with_gap(0.0)
            .add(Box::new(
                Label::new("Advanced usage of ", Arc::clone(font)).with_font_size(12.0),
            ))
            .add(Box::new(
                Label::new("TextEdit", Arc::clone(font))
                    .with_font_size(12.0)
                    .with_color(code_color()),
            ))
            .add(Box::new(
                Label::new(".", Arc::clone(font)).with_font_size(12.0),
            )),
    )
}

/// One segmented toggle button bound to a `Cell<T>` via `with_active_fn` /
/// `on_click` — the same pattern the rest of the demo uses for selectors.
fn toggle_button<T: Copy + PartialEq + 'static>(
    label: &str,
    font: &Arc<Font>,
    cell: &Rc<Cell<T>>,
    value: T,
) -> Box<dyn Widget> {
    let active = Rc::clone(cell);
    let click = Rc::clone(cell);
    Box::new(
        Button::new(label, Arc::clone(font))
            .with_font_size(12.0)
            .with_subtle()
            .with_active_fn(move || active.get() == value)
            .on_click(move || {
                if click.get() != value {
                    click.set(value);
                    agg_gui::animation::request_draw();
                }
            }),
    )
}

fn halign_row(font: &Arc<Font>, halign: &Rc<Cell<TextHAlign>>) -> Box<dyn Widget> {
    Box::new(
        FlexRow::new()
            .with_gap(6.0)
            .add(Box::new(
                Label::new("Horizontal align:", Arc::clone(font)).with_font_size(12.0),
            ))
            .add(toggle_button("Left", font, halign, TextHAlign::Left))
            .add(toggle_button("Center", font, halign, TextHAlign::Center))
            .add(toggle_button("Right", font, halign, TextHAlign::Right)),
    )
}

fn valign_row(font: &Arc<Font>, valign: &Rc<Cell<TextVAlign>>) -> Box<dyn Widget> {
    Box::new(
        FlexRow::new()
            .with_gap(6.0)
            .add(Box::new(
                Label::new("Vertical align:", Arc::clone(font)).with_font_size(12.0),
            ))
            .add(toggle_button("Top", font, valign, TextVAlign::Top))
            .add(toggle_button("Center", font, valign, TextVAlign::Center))
            .add(toggle_button("Bottom", font, valign, TextVAlign::Bottom)),
    )
}

/// The editor row: search icon · multiline `TextArea` · clear button.
fn editor_row(
    font: &Arc<Font>,
    state: &Rc<RefCell<TextEditState>>,
    halign: &Rc<Cell<TextHAlign>>,
    valign: &Rc<Cell<TextVAlign>>,
) -> Box<dyn Widget> {
    // Ctrl/Cmd+Y toggles the case of the current selection — intercepted
    // before the editor's default key handling, mirroring egui's demo.
    let state_key = Rc::clone(state);
    let key_intercept = move |key: &Key, mods: &Modifiers| -> bool {
        let is_y = matches!(key, Key::Char('y') | Key::Char('Y'));
        if is_y && (mods.ctrl || mods.meta) {
            let mut st = state_key.borrow_mut();
            if let Some((lo, hi)) = st.selection_range() {
                let sel = st.text[lo..hi].to_string();
                let upper = sel.to_uppercase();
                let new = if sel == upper {
                    sel.to_lowercase()
                } else {
                    upper
                };
                st.text.replace_range(lo..hi, &new);
                // Keep the (possibly length-changed) selection so repeated
                // presses keep toggling and the readout stays populated.
                st.anchor = lo;
                st.cursor = lo + new.len();
                st.note_text_change();
            }
            return true;
        }
        false
    };

    let editor = TextArea::new(Arc::clone(font))
        .with_font_size(13.0)
        .with_edit_state(Rc::clone(state))
        .with_hint_text("Type something!")
        .with_h_align_cell(Rc::clone(halign))
        .with_v_align_cell(Rc::clone(valign))
        .with_focus_id(TEXT_EDIT_FOCUS_ID)
        .with_key_intercept(key_intercept);

    // Clear button (❌ suffix): empties the shared buffer from outside the
    // widget; the epoch bump makes the editor re-wrap and show its hint.
    let state_clear = Rc::clone(state);
    let clear = Button::new(ICON_CLEAR, Arc::clone(font))
        .with_font_size(13.0)
        .with_subtle()
        .with_v_anchor(VAnchor::TOP)
        .on_click(move || {
            let mut st = state_clear.borrow_mut();
            st.text.clear();
            st.cursor = 0;
            st.anchor = 0;
            st.note_text_change();
            drop(st);
            agg_gui::animation::request_draw();
        });

    Box::new(
        FlexRow::new()
            .with_gap(6.0)
            .add(Box::new(
                Label::new(ICON_SEARCH, Arc::clone(font))
                    .with_font_size(13.0)
                    .with_v_anchor(VAnchor::TOP),
            ))
            .add_flex(
                Box::new(
                    SizedBox::new()
                        .with_height(130.0)
                        .with_child(Box::new(editor)),
                ),
                1.0,
            )
            .add(Box::new(clear)),
    )
}

/// Live selection readout + the Ctrl+Y hint, rebuilt when the selection or the
/// text changes (keyed on cursor / anchor / content epoch).
fn readout_block(font: &Arc<Font>, state: &Rc<RefCell<TextEditState>>) -> Box<dyn Widget> {
    let ver_state = Rc::clone(state);
    let build_state = Rc::clone(state);
    let build_font = Arc::clone(font);
    Box::new(Rebuilder::new(
        move || {
            let st = ver_state.borrow();
            (st.cursor as u64).wrapping_mul(1_000_003)
                ^ (st.anchor as u64).wrapping_mul(2_000_029)
                ^ st.epoch.wrapping_mul(4_000_037)
        },
        move || build_readout(&build_font, &build_state),
    ))
}

fn build_readout(font: &Arc<Font>, state: &Rc<RefCell<TextEditState>>) -> Box<dyn Widget> {
    let st = state.borrow();
    let selected = st
        .selection_range()
        .map(|(lo, hi)| st.text[lo..hi].to_string());
    let has_selection = selected.is_some();

    let mut col = FlexColumn::new().with_gap(6.0);

    // "Selected text: " + code(selected)
    let mut row = FlexRow::new().with_gap(0.0).add(Box::new(
        Label::new("Selected text: ", Arc::clone(font)).with_font_size(12.0),
    ));
    if let Some(sel) = selected {
        row = row.add(Box::new(
            Label::new(sel, Arc::clone(font))
                .with_font_size(12.0)
                .with_color(code_color()),
        ));
    }
    col.push(Box::new(row), 0.0);

    // Enabled-looking only when something is selected (egui uses add_enabled).
    let hint = Label::new(
        "Press ctrl+Y to toggle the case of selected text (cmd+Y on Mac)",
        Arc::clone(font),
    )
    .with_font_size(12.0);
    let hint = if has_selection {
        hint
    } else {
        hint.with_color(dim_color())
    };
    col.push(Box::new(hint), 0.0);

    Box::new(col)
}

/// "Move cursor to the: start / end" — mutates the shared cursor and hands
/// keyboard focus back to the editor, matching egui's demo behaviour.
fn move_cursor_row(font: &Arc<Font>, state: &Rc<RefCell<TextEditState>>) -> Box<dyn Widget> {
    let start_state = Rc::clone(state);
    let start = Button::new("start", Arc::clone(font))
        .with_font_size(12.0)
        .on_click(move || {
            {
                let mut st = start_state.borrow_mut();
                st.cursor = 0;
                st.anchor = 0;
            }
            agg_gui::focus::request_focus(TEXT_EDIT_FOCUS_ID);
        });

    let end_state = Rc::clone(state);
    let end = Button::new("end", Arc::clone(font))
        .with_font_size(12.0)
        .on_click(move || {
            {
                let mut st = end_state.borrow_mut();
                let len = st.text.len();
                st.cursor = len;
                st.anchor = len;
            }
            agg_gui::focus::request_focus(TEXT_EDIT_FOCUS_ID);
        });

    Box::new(
        FlexRow::new()
            .with_gap(6.0)
            .add(Box::new(
                Label::new("Move cursor to the:", Arc::clone(font)).with_font_size(12.0),
            ))
            .add(Box::new(start))
            .add(Box::new(end)),
    )
}
