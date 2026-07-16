//! Basic dialog-style demos: Undo/Redo and Window Options.
//!
//! Both mirror their egui counterparts:
//! - `undo_redo` snapshots the *whole* demo state `{toggle, text}` through a
//!   single time-coalescing [`Undoer`](agg_gui::undo::Undoer) (egui's
//!   `undo_redo.rs`), so Undo/Redo revert both controls together.
//! - `window_options` drives the *real* host [`agg_gui::Window`] flags via
//!   shared cells (see [`WindowOptionCells`]); `app_builder` wires the same
//!   cells into the window it creates, so the checkboxes actually change the
//!   window's behaviour instead of writing to dead cells.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use agg_gui::{
    Button, Checkbox, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Label, Rect,
    Separator, Size, SizedBox, TextField, Widget,
};

use agg_gui::undo::Undoer;

// ---------------------------------------------------------------------------
// Undo Redo
// ---------------------------------------------------------------------------

/// The complete demo state the [`Undoer`] snapshots — matching egui's
/// `undo_redo::State { toggle_value, text }`.
#[derive(Clone, PartialEq)]
struct UndoState {
    toggle: bool,
    text: String,
}

/// Feeds the shared [`Undoer`] the latest `{toggle, text}` every layout pass,
/// exactly like egui calls `undoer.feed_state(time, &state)` every frame.
/// While the state is mid-change it schedules follow-up frames so the
/// time-based coalescing (create an undo point after `stable_time`) can fire
/// even when the user has stopped interacting.
struct UndoRedoView {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    undoer: Rc<RefCell<Undoer<UndoState>>>,
    toggle: Rc<Cell<bool>>,
    text: Rc<RefCell<String>>,
    start: Instant,
}

impl UndoRedoView {
    fn current(&self) -> UndoState {
        UndoState {
            toggle: self.toggle.get(),
            text: self.text.borrow().clone(),
        }
    }
}

impl Widget for UndoRedoView {
    fn type_name(&self) -> &'static str {
        "UndoRedoView"
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
        let time = self.start.elapsed().as_secs_f64();
        let state = self.current();
        {
            let mut u = self.undoer.borrow_mut();
            u.feed_state(time, &state);
            // Keep frames coming while coalescing so `stable_time` can elapse
            // and an undo point gets committed even after the user idles.
            if u.is_in_flux() {
                agg_gui::animation::request_draw_after(Duration::from_millis(120));
            }
        }
        let s = if let Some(child) = self.children.first_mut() {
            let sz = child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, sz.height));
            sz
        } else {
            Size::new(available.width, 0.0)
        };
        self.bounds = Rect::new(0.0, 0.0, available.width, s.height);
        Size::new(available.width, s.height)
    }

    // Children paint through the framework's tree walk.
    fn paint(&mut self, _: &mut dyn DrawCtx) {}

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Undo Redo demo — a checkbox and a text field whose combined
/// state is versioned by one shared [`Undoer`].  Undo/Redo revert both.
pub fn undo_redo(font: Arc<Font>) -> Box<dyn Widget> {
    let toggle = Rc::new(Cell::new(false));
    let text = Rc::new(RefCell::new("Text with undo/redo".to_string()));
    let undoer: Rc<RefCell<Undoer<UndoState>>> = Rc::new(RefCell::new(Undoer::default()));

    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    col.push(
        Box::new(Label::new("Undo Redo", Arc::clone(&font)).with_font_size(13.0)),
        0.0,
    );

    col.push(
        Box::new(
            Checkbox::new("Checkbox with undo/redo", Arc::clone(&font), toggle.get())
                .with_font_size(13.0)
                .with_state_cell(Rc::clone(&toggle)),
        ),
        0.0,
    );

    col.push(
        Box::new(
            SizedBox::new().with_height(34.0).with_child(Box::new(
                TextField::new(Arc::clone(&font))
                    .with_font_size(13.0)
                    .with_text_cell(Rc::clone(&text)),
            )),
        ),
        0.0,
    );

    // Undo / Redo buttons — enabled state and click both consult the shared
    // Undoer against the *current* combined state.
    let mut buttons = FlexRow::new().with_gap(8.0);
    {
        let undoer_en = Rc::clone(&undoer);
        let t_en = Rc::clone(&toggle);
        let tx_en = Rc::clone(&text);
        let undoer_cl = Rc::clone(&undoer);
        let t_cl = Rc::clone(&toggle);
        let tx_cl = Rc::clone(&text);
        buttons.push(
            Box::new(
                Button::new("\u{27F2} Undo", Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_enabled_fn(move || {
                        let cur = UndoState {
                            toggle: t_en.get(),
                            text: tx_en.borrow().clone(),
                        };
                        undoer_en.borrow().has_undo(&cur)
                    })
                    .on_click(move || {
                        let cur = UndoState {
                            toggle: t_cl.get(),
                            text: tx_cl.borrow().clone(),
                        };
                        let prev = undoer_cl.borrow_mut().undo(&cur).cloned();
                        if let Some(p) = prev {
                            t_cl.set(p.toggle);
                            *tx_cl.borrow_mut() = p.text;
                        }
                        agg_gui::animation::request_draw();
                    }),
            ),
            0.0,
        );
    }
    {
        let undoer_en = Rc::clone(&undoer);
        let t_en = Rc::clone(&toggle);
        let tx_en = Rc::clone(&text);
        let undoer_cl = Rc::clone(&undoer);
        let t_cl = Rc::clone(&toggle);
        let tx_cl = Rc::clone(&text);
        buttons.push(
            Box::new(
                Button::new("\u{27F3} Redo", Arc::clone(&font))
                    .with_font_size(12.0)
                    .with_enabled_fn(move || {
                        let cur = UndoState {
                            toggle: t_en.get(),
                            text: tx_en.borrow().clone(),
                        };
                        undoer_en.borrow().has_redo(&cur)
                    })
                    .on_click(move || {
                        let cur = UndoState {
                            toggle: t_cl.get(),
                            text: tx_cl.borrow().clone(),
                        };
                        let next = undoer_cl.borrow_mut().redo(&cur).cloned();
                        if let Some(n) = next {
                            t_cl.set(n.toggle);
                            *tx_cl.borrow_mut() = n.text;
                        }
                        agg_gui::animation::request_draw();
                    }),
            ),
            0.0,
        );
    }
    col.push(Box::new(buttons), 0.0);

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "One shared Undoer snapshots the whole state {toggle, text} with \
         time-based coalescing (rapid edits collapse into a single undo point). \
         Undo and Redo revert both controls together, matching egui's Undoer<State>.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);

    Box::new(UndoRedoView {
        bounds: Rect::default(),
        children: vec![Box::new(col)],
        undoer,
        toggle,
        text,
        start: Instant::now(),
    })
}

// ---------------------------------------------------------------------------
// Window Options
// ---------------------------------------------------------------------------

/// Shared cells that connect the Window Options demo's checkboxes/field to the
/// real host [`agg_gui::Window`].  `app_builder` constructs one of these,
/// builds the demo content with it ([`window_options_with_cells`]) AND wires
/// the same cells into the window (`Window::with_resizable_cell`, etc.), so
/// the controls drive live window behaviour instead of dead state.
#[derive(Clone)]
pub struct WindowOptionCells {
    pub resizable: Rc<Cell<bool>>,
    pub collapsible: Rc<Cell<bool>>,
    pub auto_size: Rc<Cell<bool>>,
    pub title: Rc<RefCell<String>>,
}

impl WindowOptionCells {
    pub fn new(initial_title: &str) -> Self {
        Self {
            resizable: Rc::new(Cell::new(true)),
            collapsible: Rc::new(Cell::new(true)),
            auto_size: Rc::new(Cell::new(false)),
            title: Rc::new(RefCell::new(initial_title.to_string())),
        }
    }
}

/// Live "Current window size: W × H" label.  Reads the window's position cell
/// (which [`agg_gui::Window`] writes its bounds into every layout) and keeps a
/// [`Label`] child in sync — replacing the old hard-coded string.
struct WindowSizeLabel {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    pos: Rc<Cell<Rect>>,
}

impl WindowSizeLabel {
    fn text(&self) -> String {
        let r = self.pos.get();
        format!(
            "Current window size: {} \u{00d7} {}",
            r.width.round() as i64,
            r.height.round() as i64
        )
    }
}

impl Widget for WindowSizeLabel {
    fn type_name(&self) -> &'static str {
        "WindowSizeLabel"
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
        let text = self.text();
        let s = if let Some(child) = self.children.first_mut() {
            child.set_label_text(&text);
            child.layout(Size::new(available.width, 20.0))
        } else {
            Size::new(0.0, 18.0)
        };
        let h = s.height.max(18.0);
        self.bounds = Rect::new(0.0, 0.0, available.width, h);
        if let Some(child) = self.children.first_mut() {
            child.set_bounds(Rect::new(0.0, (h - s.height) * 0.5, s.width, s.height));
        }
        Size::new(available.width, h)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let color = ctx.visuals().text_color;
        if let Some(child) = self.children.first_mut() {
            child.set_label_color(color);
        }
        // Label child paints itself via the framework's tree walk.
    }

    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}

/// Build the Window Options demo content wired to `cells` and the window's
/// `pos` cell.  Mirrors egui's window_options.rs for the subset of flags our
/// `Window` supports; see the module doc for what is intentionally skipped.
pub fn window_options_with_cells(
    font: Arc<Font>,
    cells: &WindowOptionCells,
    pos: Rc<Cell<Rect>>,
) -> Box<dyn Widget> {
    let mut col = FlexColumn::new()
        .with_gap(12.0)
        .with_padding(16.0)
        .with_panel_bg();

    // title: <text edit>  (egui's `ui.text_edit_singleline(title)`)
    let title_row = FlexRow::new()
        .with_gap(8.0)
        .add(Box::new(
            Label::new("title:", Arc::clone(&font)).with_font_size(13.0),
        ))
        .add_flex(
            Box::new(
                SizedBox::new().with_height(30.0).with_child(Box::new(
                    TextField::new(Arc::clone(&font))
                        .with_font_size(13.0)
                        .with_text_cell(Rc::clone(&cells.title)),
                )),
            ),
            1.0,
        );
    col.push(Box::new(title_row), 0.0);

    // Supported window-flag checkboxes.
    for (label, cell) in [
        ("resizable", Rc::clone(&cells.resizable)),
        ("collapsible", Rc::clone(&cells.collapsible)),
        ("auto-size", Rc::clone(&cells.auto_size)),
    ] {
        col.push(
            Box::new(
                Checkbox::new(label, Arc::clone(&font), cell.get())
                    .with_font_size(13.0)
                    .with_state_cell(cell),
            ),
            0.0,
        );
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Live window size read straight from the window's position cell.
    col.push(
        Box::new(WindowSizeLabel {
            bounds: Rect::default(),
            children: vec![Box::new(
                Label::new("Current window size: —", Arc::clone(&font)).with_font_size(12.0),
            )],
            pos,
        }),
        0.0,
    );

    col.push(Box::new(Separator::horizontal()), 0.0);
    col.push(
        Box::new(
            Label::new(
                "resizable, collapsible and auto-size drive the real host window. \
         auto-size is agg-gui's own addition (not in egui). Skipped vs egui: \
         title_bar / closable / constrain / hscroll / vscroll and the anchor \
         controls — the Window widget has no runtime hook for those yet. \
         Editing title changes the visible title only; the window keeps its \
         original identity so saved layout/z-order stay intact.",
                Arc::clone(&font),
            )
            .with_font_size(11.0)
            .with_wrap(true),
        ),
        0.0,
    );

    col.push(Box::new(SizedBox::new().with_height(8.0)), 0.0);
    Box::new(col)
}

/// Fallback builder used by the generic content dispatcher.  Creates local
/// cells not wired to a real window, so the checkboxes are inert here — the
/// live version is built by `app_builder` via [`window_options_with_cells`].
pub fn window_options(font: Arc<Font>) -> Box<dyn Widget> {
    let cells = WindowOptionCells::new("\u{F013} Window Options");
    window_options_with_cells(font, &cells, Rc::new(Cell::new(Rect::default())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_font() -> Arc<Font> {
        const BYTES: &[u8] = include_bytes!("../../../../../demo/assets/CascadiaCode.ttf");
        Arc::new(Font::from_slice(BYTES).expect("parse CascadiaCode.ttf"))
    }

    #[test]
    fn window_size_label_reads_position_cell() {
        let pos = Rc::new(Cell::new(Rect::new(10.0, 20.0, 360.0, 290.0)));
        let mut label = WindowSizeLabel {
            bounds: Rect::default(),
            children: vec![Box::new(Label::new("x", test_font()).with_font_size(12.0))],
            pos: Rc::clone(&pos),
        };
        label.layout(Size::new(300.0, 40.0));
        assert_eq!(label.text(), "Current window size: 360 \u{00d7} 290");

        pos.set(Rect::new(0.0, 0.0, 400.0, 250.0));
        label.layout(Size::new(300.0, 40.0));
        assert_eq!(label.text(), "Current window size: 400 \u{00d7} 250");
    }

    #[test]
    fn undo_redo_view_feeds_and_reverts_combined_state() {
        let toggle = Rc::new(Cell::new(false));
        let text = Rc::new(RefCell::new("Text with undo/redo".to_string()));
        let undoer: Rc<RefCell<Undoer<UndoState>>> = Rc::new(RefCell::new(Undoer::default()));
        let view = UndoRedoView {
            bounds: Rect::default(),
            children: vec![Box::new(
                Label::new("placeholder", test_font()).with_font_size(12.0),
            )],
            undoer: Rc::clone(&undoer),
            toggle: Rc::clone(&toggle),
            text: Rc::clone(&text),
            start: Instant::now(),
        };

        // Baseline snapshot via the first feed.
        view.undoer.borrow_mut().feed_state(0.0, &view.current());

        // Change both fields and let the state stabilise into an undo point.
        toggle.set(true);
        *text.borrow_mut() = "edited".to_string();
        let changed = view.current();
        view.undoer.borrow_mut().feed_state(0.1, &changed);
        view.undoer.borrow_mut().feed_state(2.0, &changed);
        assert!(view.undoer.borrow().has_undo(&changed));

        // Undo should revert BOTH controls.
        let prev = view.undoer.borrow_mut().undo(&changed).cloned().unwrap();
        assert!(!prev.toggle);
        assert_eq!(prev.text, "Text with undo/redo");
    }
}
