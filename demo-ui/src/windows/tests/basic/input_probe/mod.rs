//! Interaction probe — the agg-gui analogue of egui's `tests/input_test.rs`.
//!
//! egui's Input Test is a Sense × Response matrix: four buttons, each with a
//! different `Sense`, that print what `egui::Response` reports (clicked,
//! double-clicked, dragged, …).  agg-gui has no `Response`/`Sense` abstraction —
//! widgets classify raw [`Event`]s themselves and signal handling through
//! [`EventResult`].  So instead of faking a `Sense` API this window builds four
//! side-by-side **probe areas**, each a custom [`Widget`] with a different
//! consumption profile:
//!
//! * **Hover** — `on_event` returns `Ignored` for everything; only tracks
//!   contains-pointer / hover enter & leave.
//! * **Click** — consumes `MouseDown`/`MouseUp`, classifies click / double /
//!   triple click per mouse button.
//! * **Drag** — consumes the press (so the framework captures the pointer) and
//!   reports drag started / dragged(Δx,Δy) / stopped, even outside its bounds.
//! * **Click + Drag** — both of the above.
//!
//! Each probe records its classified interactions into a per-probe deduplicated
//! history (the same ×N coalescing used by `input_event_history` in
//! `controls.rs`).  The classification itself lives in the sibling
//! [`classifier`] module and is unit-tested there; this file is just the
//! event/paint shell plus the window builder.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use web_time::Instant;

use agg_gui::{
    Button, Checkbox, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Font, Label, Point, Rect,
    Separator, Size, SizedBox, Widget,
};

mod classifier;

use classifier::{describe, Interaction, InteractionClassifier, ProbeKind};

/// A deduplicated history row — consecutive identical `summary`s coalesce.
struct HistoryEntry {
    summary: String,
    full: String,
    count: usize,
}

/// Mutable state owned by one probe. Shared via `Rc<RefCell<…>>` so the Clear
/// button can reach every probe.
struct ProbeState {
    classifier: InteractionClassifier,
    /// Newest at index 0.
    history: Vec<HistoryEntry>,
    /// Live contains-pointer flag, shown in the header (never logged).
    inside: bool,
}

impl ProbeState {
    fn new(kind: ProbeKind) -> Self {
        Self {
            classifier: InteractionClassifier::new(kind),
            history: Vec::new(),
            inside: false,
        }
    }

    fn clear(&mut self, kind: ProbeKind) {
        self.classifier = InteractionClassifier::new(kind);
        self.history.clear();
        self.inside = false;
    }

    /// Coalesce with the newest entry when the summary matches, else prepend.
    fn add(&mut self, summary: String, full: String) {
        if let Some(first) = self.history.first_mut() {
            if first.summary == summary {
                first.count += 1;
                first.full = full;
                return;
            }
        }
        self.history.insert(
            0,
            HistoryEntry {
                summary,
                full,
                count: 1,
            },
        );
        self.history.truncate(PROBE_HISTORY_CAP);
    }
}

const PROBE_HISTORY_CAP: usize = 200;
const PROBE_HEADER_H: f64 = 64.0;
const PROBE_LINE_H: f64 = 16.0;

/// A single interaction-probe area.
struct ProbeWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    kind: ProbeKind,
    state: Rc<RefCell<ProbeState>>,
    include_hover: Rc<Cell<bool>>,
    start: Instant,
}

impl ProbeWidget {
    fn now_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// Event positions are widget-local (origin at the probe's own corner), so
    /// "inside" is simply within `[0, width] × [0, height]`.
    fn contains(&self, p: Point) -> bool {
        p.x >= 0.0 && p.x <= self.bounds.width && p.y >= 0.0 && p.y <= self.bounds.height
    }

    /// Push classified interactions into the history (respecting the hover
    /// checkbox) and request a repaint if anything was logged.
    fn log(&mut self, interactions: Vec<Interaction>) {
        if interactions.is_empty() {
            return;
        }
        let include_hover = self.include_hover.get();
        let mut st = self.state.borrow_mut();
        let mut logged = false;
        for i in interactions {
            let (summary, full, is_hover) = describe(i);
            if is_hover && !include_hover {
                continue;
            }
            st.add(summary, full);
            logged = true;
        }
        drop(st);
        if logged {
            agg_gui::animation::request_draw();
        }
    }
}

impl Widget for ProbeWidget {
    fn type_name(&self) -> &'static str {
        "ProbeWidget"
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
        let v = ctx.visuals();
        let w = self.bounds.width;
        let h = self.bounds.height;
        let st = self.state.borrow();

        // Background + border. A live contains-pointer highlight uses the accent
        // colour so the hover probe visibly reacts even though it consumes
        // nothing.
        ctx.set_fill_color(v.widget_bg);
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.fill();
        if st.inside {
            ctx.set_stroke_color(v.accent);
            ctx.set_line_width(2.0);
        } else {
            ctx.set_stroke_color(v.widget_stroke);
            ctx.set_line_width(1.0);
        }
        ctx.begin_path();
        ctx.rect(0.0, 0.0, w, h);
        ctx.stroke();

        // Header: title, profile, live contains-pointer.
        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(12.5);
        ctx.set_fill_color(v.text_color);
        ctx.fill_text(self.kind.title(), 8.0, h - 18.0);

        ctx.set_font_size(9.5);
        ctx.set_fill_color(v.text_dim);
        ctx.fill_text(self.kind.profile(), 8.0, h - 33.0);
        ctx.fill_text(
            &format!("contains pointer: {}", if st.inside { "yes" } else { "no" }),
            8.0,
            h - 48.0,
        );

        // Separator under the header.
        ctx.set_stroke_color(v.separator);
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, h - PROBE_HEADER_H);
        ctx.line_to(w, h - PROBE_HEADER_H);
        ctx.stroke();

        // History, newest first, below the header.
        ctx.set_font_size(11.0);
        if st.history.is_empty() {
            ctx.set_fill_color(v.text_dim);
            ctx.fill_text("(interact here)", 8.0, h - PROBE_HEADER_H - 16.0);
            return;
        }
        for (idx, entry) in st.history.iter().enumerate() {
            let y = h - PROBE_HEADER_H - (idx as f64 + 1.0) * PROBE_LINE_H;
            if y < 0.0 {
                break;
            }
            ctx.set_fill_color(v.text_color);
            ctx.fill_text(&entry.full, 8.0, y + 2.0);
            if entry.count >= 2 {
                let sw = ctx
                    .measure_text(&entry.full)
                    .map(|m| m.width)
                    .unwrap_or(0.0);
                ctx.set_fill_color(v.text_dim);
                ctx.fill_text(
                    &format!(" \u{00d7}{}", entry.count),
                    8.0 + sw + 2.0,
                    y + 2.0,
                );
            }
        }
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        let now = self.now_ms();
        match event {
            Event::MouseMove { pos } => {
                let inside = self.contains(*pos);
                let (interactions, consume) = {
                    let mut st = self.state.borrow_mut();
                    let interactions = st.classifier.on_move(*pos, inside, now);
                    st.inside = inside;
                    // While a drag gesture is captured, keep consuming moves so
                    // the framework routes them here; hover never consumes.
                    let consume = matches!(self.kind, ProbeKind::Drag | ProbeKind::ClickAndDrag)
                        && st.classifier.is_pressed();
                    (interactions, consume)
                };
                self.log(interactions);
                // Hover-state change still needs a repaint even when the event
                // is not consumed (Ignored does not auto-request one).
                agg_gui::animation::request_draw();
                if consume {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Event::MouseDown { pos, button, .. } => {
                if !self.contains(*pos) {
                    return EventResult::Ignored;
                }
                let interactions = self
                    .state
                    .borrow_mut()
                    .classifier
                    .on_down(*button, *pos, now);
                self.log(interactions);
                if self.kind == ProbeKind::Hover {
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                }
            }
            Event::MouseUp { pos, button, .. } => {
                let had_press = self.state.borrow().classifier.is_pressed();
                let interactions = self.state.borrow_mut().classifier.on_up(*button, *pos, now);
                self.log(interactions);
                if self.kind != ProbeKind::Hover && had_press {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn hit_test(&self, p: Point) -> bool {
        self.contains(p)
    }
}

/// Build one probe widget.
fn probe(
    font: &Arc<Font>,
    kind: ProbeKind,
    state: &Rc<RefCell<ProbeState>>,
    include_hover: &Rc<Cell<bool>>,
) -> Box<dyn Widget> {
    Box::new(ProbeWidget {
        bounds: Rect::default(),
        children: Vec::new(),
        font: Arc::clone(font),
        kind,
        state: Rc::clone(state),
        include_hover: Rc::clone(include_hover),
        start: Instant::now(),
    })
}

/// Build the Input Test — an interaction probe with four differently-sensing
/// custom widgets side by side. This is the honest agg-gui counterpart of
/// egui's Sense × Response `input_test.rs`.
pub fn input_test(font: Arc<Font>) -> Box<dyn Widget> {
    let include_hover = Rc::new(Cell::new(false));
    let kinds = [
        ProbeKind::Hover,
        ProbeKind::Click,
        ProbeKind::Drag,
        ProbeKind::ClickAndDrag,
    ];
    let states: Vec<Rc<RefCell<ProbeState>>> = kinds
        .iter()
        .map(|&k| Rc::new(RefCell::new(ProbeState::new(k))))
        .collect();

    let mut col = FlexColumn::new()
        .with_gap(8.0)
        .with_padding(10.0)
        .with_panel_bg();

    col.push(
        Box::new(
            Label::new(
                "This tests how agg-gui widgets classify raw pointer events via \
                 Widget::on_event + EventResult. Each probe below consumes a \
                 different set of events. Try clicking, double-clicking, \
                 triple-clicking, and dragging each one with any mouse button.",
                Arc::clone(&font),
            )
            .with_font_size(11.5)
            .with_wrap(true),
        ),
        0.0,
    );

    // Clear + Include-hover row.
    {
        let clear_states: Vec<Rc<RefCell<ProbeState>>> = states.iter().map(Rc::clone).collect();
        let clear_kinds = kinds;
        let clear_btn = SizedBox::new()
            .with_width(70.0)
            .with_height(28.0)
            .with_child(Box::new(
                Button::new("Clear", Arc::clone(&font))
                    .with_font_size(12.0)
                    .on_click(move || {
                        for (st, &k) in clear_states.iter().zip(clear_kinds.iter()) {
                            st.borrow_mut().clear(k);
                        }
                        agg_gui::animation::request_draw();
                    }),
            ));

        let row = FlexRow::new()
            .with_gap(12.0)
            .add(Box::new(clear_btn))
            .add(Box::new(
                SizedBox::new().with_height(28.0).with_child(Box::new(
                    Checkbox::new(
                        "Include hover events",
                        Arc::clone(&font),
                        include_hover.get(),
                    )
                    .with_font_size(12.0)
                    .with_state_cell(Rc::clone(&include_hover)),
                )),
            ));
        col.push(Box::new(row), 0.0);
    }

    col.push(Box::new(Separator::horizontal()), 0.0);

    // Four probes side by side, each filling an equal share of the width.
    let mut row = FlexRow::new().with_gap(8.0);
    for (&kind, state) in kinds.iter().zip(states.iter()) {
        row = row.add_flex(probe(&font, kind, state, &include_hover), 1.0);
    }
    col.push(Box::new(row), 1.0);

    Box::new(col)
}
