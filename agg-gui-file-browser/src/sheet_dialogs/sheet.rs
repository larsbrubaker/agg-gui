//! [`DialogSheet`]: the widget [`SheetDialogs`](super::SheetDialogs) show their questions on (the
//! rules are in `mod.rs`). It holds nothing while no question shows; at the layout after a
//! question is asked it builds an agg-gui `ModalSheet` for it (title, text, a name field for a
//! name, Cancel and the confirming button; for a Move, the Move page over a browser of its own
//! opened over the dialogs' move root), and drops it once the question is answered.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::focus::{request_focus, FocusId};
use agg_gui::text::Font;
use agg_gui::{
    Button, DrawCtx, Event, EventResult, FlexColumn, FlexRow, Label, ModalSheet, Point, Rect, Size,
    SizedBox, TextField, Widget, WidgetBase,
};
use agg_gui_files::tr;

use super::{answer_move, Question, Shared};
use crate::config::BrowserConfig;
use crate::look::{BrowserNames, DefaultLook};
use crate::move_dialog::{dialog_options, MoveAnswers, MovePage};
use crate::state::BrowserState;

/// The name of a name question's text field.
pub const NAME_FIELD: &str = "Browser Dialog Name Field";
/// The name of the confirming button (Create, Rename, Remove, ...).
pub const CONFIRM_BUTTON: &str = "Browser Dialog Confirm Button";
/// The name of the Cancel button.
pub const CANCEL_BUTTON: &str = "Browser Dialog Cancel Button";

/// The panel's size.
const PANEL: Size = Size::new(420.0, 190.0);
/// The Move dialog's panel: room to browse.
const MOVE_PANEL: Size = Size::new(480.0, 500.0);

/// The question showing: what hears its answer (taken when answered) and the sheet's visibility.
#[derive(Clone)]
struct Showing {
    question: Rc<RefCell<Option<Question>>>,
    visible: Rc<Cell<bool>>,
}

impl Showing {
    /// Hides the sheet and hands back the question, if it is still unanswered.
    fn take(&self) -> Option<Question> {
        self.visible.set(false);
        agg_gui::animation::request_layout();
        self.question.borrow_mut().take()
    }

    fn cancel(&self) {
        if let Some(question) = self.take() {
            question.cancel();
        }
    }

    fn confirm(&self) {
        if let Some(Question::Confirm(_, answer)) = self.take() {
            answer(true);
        }
    }

    /// Accepts `name`; an empty one accepts nothing and the sheet stays.
    fn accept_name(&self, name: &str) {
        if name.is_empty() {
            return;
        }
        if let Some(Question::AskName(_, answer)) = self.take() {
            answer(Some(name.to_string()));
        }
    }
}

/// The sheet the dialogs show on (see the module header).
pub struct DialogSheet {
    shared: Rc<Shared>,
    font: Arc<Font>,
    showing: Option<Showing>,
    focus_id: FocusId,
    bounds: Rect,
    base: WidgetBase,
    children: Vec<Box<dyn Widget>>,
}

impl DialogSheet {
    pub(crate) fn new(shared: Rc<Shared>, font: Arc<Font>) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        // Clear of the list's and tree's ranges (`state/gesture.rs`).
        static NEXT: AtomicU64 = AtomicU64::new(0x11B7_0000_0000);
        Self {
            shared,
            font,
            showing: None,
            focus_id: NEXT.fetch_add(1, Ordering::Relaxed),
            bounds: Rect::default(),
            base: WidgetBase::new(),
            children: Vec::new(),
        }
    }

    /// Whether a question shows.
    pub fn is_showing(&self) -> bool {
        self.showing.as_ref().is_some_and(|s| s.visible.get())
    }

    /// Shows a waiting question (the one showing, if any, answered as cancelled), and drops the
    /// sheet of one answered.
    fn sync(&mut self) {
        let pending = self.shared.pending.borrow_mut().take();
        if let Some(question) = pending {
            if let Some(showing) = self.showing.take() {
                showing.cancel();
            }
            self.show(question);
        }
        if let Some(showing) = &self.showing {
            if !showing.visible.get() {
                // Closed some other way than a button (the sheet's own Escape): cancelled.
                showing.cancel();
                self.showing = None;
                self.children.clear();
            }
        }
        self.shared.showing.set(self.showing.is_some());
    }

    fn show(&mut self, question: Question) {
        let showing = Showing {
            question: Rc::new(RefCell::new(None)),
            visible: Rc::new(Cell::new(true)),
        };
        let mut panel = PANEL;
        let content = match &question {
            Question::PickMove(request, _) => {
                panel = MOVE_PANEL;
                self.move_page(request.clone(), &showing)
            }
            Question::Confirm(confirmation, _) => {
                let text = Label::new(confirmation.message.clone(), Arc::clone(&self.font))
                    .with_wrap(true);
                self.panel(
                    &confirmation.title,
                    Box::new(text),
                    &confirmation.confirm_label,
                    &showing,
                    None,
                )
            }
            Question::AskName(request, _) => {
                let text = Rc::new(RefCell::new(request.initial_name.clone()));
                let (accept, typed) = (showing.clone(), Rc::clone(&text));
                let field = TextField::new(Arc::clone(&self.font))
                    .with_text_cell(Rc::clone(&text))
                    .with_select_all_on_focus(true)
                    .with_focus_id(self.focus_id)
                    .on_enter(move |name| accept.accept_name(name))
                    .with_name(NAME_FIELD);
                let column = FlexColumn::new()
                    .with_gap(6.0)
                    .add(Box::new(Label::new(
                        request.prompt.clone(),
                        Arc::clone(&self.font),
                    )))
                    .add(Box::new(field));
                request_focus(self.focus_id);
                self.panel(
                    &request.title,
                    Box::new(column),
                    &request.confirm_label,
                    &showing,
                    Some(typed),
                )
            }
        };
        *showing.question.borrow_mut() = Some(question);
        let (cancel, closed) = (showing.clone(), showing.clone());
        let sheet = ModalSheet::new(Rc::clone(&showing.visible), content)
            .with_panel_size(panel)
            .with_cancel_action(move || cancel.cancel())
            .with_on_close(move || closed.cancel());
        self.children = vec![Box::new(sheet)];
        self.showing = Some(showing);
    }

    /// The Move page over a new browser of the move root, answering through `showing`.
    fn move_page(
        &self,
        request: crate::dialogs::MoveRequest,
        showing: &Showing,
    ) -> Box<dyn Widget> {
        let root = self.shared.move_root.borrow().clone();
        let Some(root) = root else {
            return Box::new(SizedBox::new());
        };
        let made = self.shared.move_config.borrow().clone();
        let config = match made {
            Some(make) => make(),
            None => {
                // The page's view and sorting buttons are named apart from a browser's behind it.
                let names = BrowserNames {
                    view_options_button: "Library Browser View Options Button".into(),
                    sort_button: "Library Browser Sorting Button".into(),
                    ..BrowserNames::default()
                };
                BrowserConfig::builder()
                    .look(Rc::new(DefaultLook::with_names(names)))
                    .options(dialog_options())
                    .build()
            }
        };
        let key = self.shared.move_location_key.borrow().clone();
        let state = BrowserState::new(root(), config);
        let (accepting, cancelling) = (showing.clone(), showing.clone());
        let answers = MoveAnswers {
            accept: Rc::new(move |pick| answer_move(accepting.take(), pick)),
            cancel: Rc::new(move || cancelling.cancel()),
        };
        let font = Arc::clone(&self.font);
        Box::new(MovePage::new(
            &state,
            request,
            answers,
            font,
            key.as_deref(),
        ))
    }

    /// The panel: `title`, `body`, then Cancel and the confirming button `confirm_label` (which
    /// accepts the name in `typed` for a name question).
    fn panel(
        &self,
        title: &str,
        body: Box<dyn Widget>,
        confirm_label: &str,
        showing: &Showing,
        typed: Option<Rc<RefCell<String>>>,
    ) -> Box<dyn Widget> {
        let font = &self.font;
        let cancel = showing.clone();
        let cancel_button = Button::new(tr("Cancel"), Arc::clone(font))
            .on_click(move || cancel.cancel())
            .with_cancel_action()
            .with_name(CANCEL_BUTTON);
        let confirm = showing.clone();
        let confirm_button = Button::new(confirm_label, Arc::clone(font))
            .on_click(move || match &typed {
                Some(typed) => {
                    let name = typed.borrow().clone();
                    confirm.accept_name(&name);
                }
                None => confirm.confirm(),
            })
            .with_default_action()
            .with_name(CONFIRM_BUTTON);
        let buttons = FlexRow::new()
            .with_gap(8.0)
            .add_flex(Box::new(SizedBox::new().with_height(1.0)), 1.0)
            .add(Box::new(cancel_button))
            .add(Box::new(confirm_button));
        let heading = Label::new(title, Arc::clone(font))
            .with_font_size(16.0)
            .with_strong(true);
        Box::new(
            FlexColumn::new()
                .with_padding(16.0)
                .with_gap(12.0)
                .add(Box::new(heading))
                .add(body)
                .add_flex(Box::new(SizedBox::new()), 1.0)
                .add(Box::new(buttons)),
        )
    }
}

impl Widget for DialogSheet {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "DialogSheet"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.sync();
        for child in &mut self.children {
            child.layout(available);
            child.set_bounds(Rect::new(0.0, 0.0, available.width, available.height));
        }
        available
    }
    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}
    /// Only a showing sheet takes presses; without one, everything under it gets them.
    fn hit_test(&self, _local_pos: Point) -> bool {
        self.is_showing()
    }
    fn on_event(&mut self, _event: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![("showing", self.is_showing().to_string())]
    }
}
