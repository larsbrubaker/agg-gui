//! What a list shows in place of a folder whose listing is only a message
//! (`BrowserState::listing_message`: "Loading..." while the listing is on its way, or why it
//! could not be listed): [`FolderMessage`], the text wrapped onto as many lines as it needs
//! within the list's width and centred in the list, under the look's warning icon for a refusal
//! (no icon while loading). It never reaches past the list, which a long translation or a phone
//! on its side can make short: the icon is left out when it and the text don't both fit, and
//! text taller than the list fills it and scrolls. A folder refused for the sign-in shows the
//! app's Sign In under its text (`BrowserActions::sign_in`, asked again at each layout so the
//! text and button follow the sign-in as it goes on); the button keeps its room, the text giving
//! up its room first. An app that cannot sign in shows the message alone.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{
    Button, DrawCtx, Event, EventResult, Label, LabelAlign, Rect, ScrollView, Size, VAnchor, Widget,
};

use crate::actions::SignInOffer;
use crate::look::{BrowserIcon, BrowserNames, BrowserStyle};
use crate::state::{BrowserState, ListingMessage};

use super::tile_art::{paint_art, Art};

/// The icon's side, in logical units.
const ICON_SIZE: f64 = 32.0;
/// The room between the icon and the text.
const ICON_GAP: f64 = 8.0;
/// The room kept clear each side of the text.
const SIDE_MARGIN: f64 = 16.0;
/// The widest the text runs, so a wide list still reads as a short paragraph.
const MAX_TEXT_WIDTH: f64 = 420.0;
/// The room between the text and the Sign In button.
const BUTTON_GAP: f64 = 10.0;

/// What the message and its button show, as last built.
#[derive(Clone, PartialEq)]
struct Shown {
    text: String,
    action: Option<String>,
}

/// A folder's message, centred in the list (see the module header).
pub struct FolderMessage {
    bounds: Rect,
    /// The warning icon over a refusal, none while loading.
    icon: Option<Art>,
    icon_rect: Rect,
    icon_fits: bool,
    /// The `ScrollView` over the wrapped text, then the Sign In button when there is one.
    children: Vec<Box<dyn Widget>>,
    /// The browser, asked for its Sign In when the folder was refused for the sign-in.
    sign_in: Option<Rc<RefCell<BrowserState>>>,
    /// The folder's own message, what the Sign In is asked about.
    folder_text: String,
    shown: Shown,
    /// Whether the button takes a press now, read at each layout.
    enabled: Rc<std::cell::Cell<bool>>,
    style: BrowserStyle,
    names: BrowserNames,
    font: Arc<Font>,
}

impl FolderMessage {
    /// The message `message` of the folder `state` shows, drawn in `style` with `font` and its
    /// widgets named from `names`; `icon` is the look's warning icon (shown for a refusal).
    pub fn new(
        state: &Rc<RefCell<BrowserState>>,
        message: &ListingMessage,
        style: BrowserStyle,
        names: BrowserNames,
        font: Arc<Font>,
        icon: Art,
    ) -> Self {
        let shown = Shown {
            text: message.text.clone(),
            action: None,
        };
        let mut view = Self {
            bounds: Rect::default(),
            icon: (!message.loading).then_some(icon),
            icon_rect: Rect::default(),
            icon_fits: false,
            children: Vec::new(),
            sign_in: message.sign_in.then(|| Rc::clone(state)),
            folder_text: message.text.clone(),
            shown,
            enabled: Rc::new(std::cell::Cell::new(true)),
            style,
            names,
            font,
        };
        view.children = vec![view.text_view(&message.text)];
        view.follow_sign_in();
        view
    }

    /// The text the message shows now.
    pub fn text(&self) -> &str {
        &self.shown.text
    }

    /// The Sign In's title, when the message shows one.
    pub fn sign_in_action(&self) -> Option<&str> {
        self.shown.action.as_deref()
    }

    /// Asks the app for its Sign In (with the state let go) and builds the text and button
    /// again when what they show changed.
    fn follow_sign_in(&mut self) {
        let Some(state) = &self.sign_in else {
            return;
        };
        let (actions, snapshot) = {
            let s = state.borrow();
            (Rc::clone(&s.config().actions), s.snapshot())
        };
        let offer = actions.sign_in(&snapshot, &self.folder_text);
        let shown = match &offer {
            Some(offer) => Shown {
                text: offer.text.clone(),
                action: Some(offer.action.clone()),
            },
            None => Shown {
                text: self.folder_text.clone(),
                action: None,
            },
        };
        self.enabled
            .set(offer.as_ref().is_some_and(|offer| offer.enabled));
        if shown == self.shown && self.children.len() == 1 + usize::from(offer.is_some()) {
            return;
        }
        let mut children = vec![self.text_view(&shown.text)];
        if let Some(offer) = offer {
            children.push(self.button(offer));
        }
        self.children = children;
        self.shown = shown;
    }

    /// The Sign In button for `offer`.
    fn button(&self, offer: SignInOffer) -> Box<dyn Widget> {
        let shown = Rc::clone(&self.enabled);
        let enabled: Rc<dyn Fn() -> bool> = Rc::new(move || shown.get());
        let name = &self.names.sign_in_button;
        let look = self
            .sign_in
            .as_ref()
            .map(|state| Rc::clone(&state.borrow().config().look));
        let made = look.and_then(|look| {
            look.message_button(
                &offer.action,
                name,
                Rc::clone(&enabled),
                Rc::clone(&offer.press),
            )
        });
        if let Some(button) = made {
            return button;
        }
        let press = Rc::clone(&offer.press);
        let mut button = Button::new(offer.action, Arc::clone(&self.font))
            .with_font_size(self.style.font_size)
            .with_enabled_fn(move || enabled())
            .on_click(move || press());
        // Named on the button itself, so what finds it by name reads the button's own state
        // (whether it takes a press).
        button.set_name(Some(self.names.sign_in_button.clone()));
        Box::new(button)
    }

    /// The wrapped `text` in its `ScrollView`.
    fn text_view(&self, text: &str) -> Box<dyn Widget> {
        let label = Label::new(text.to_string(), Arc::clone(&self.font))
            .with_wrap(true)
            .with_align(LabelAlign::Center)
            .with_font_size(self.style.font_size)
            .with_color(self.style.text_color)
            .with_v_anchor(VAnchor::TOP);
        let text = MessageText {
            bounds: Rect::default(),
            id: self.names.folder_message_text.clone(),
            children: vec![Box::new(label)],
        };
        Box::new(ScrollView::new(Box::new(text)))
    }
}

/// The icon the message shows over a refusal: the look's warning in the text colour.
pub fn warning_icon(look: &dyn crate::look::BrowserLook, style: &BrowserStyle) -> Art {
    let size = Size::new(ICON_SIZE, ICON_SIZE);
    match look.tinted_icon(BrowserIcon::Warning, size, style.text_color) {
        Some(image) => Art::Image(image),
        None => Art::Glyph {
            icon: BrowserIcon::Warning,
            color: style.text_color,
            side: ICON_SIZE,
        },
    }
}

impl Widget for FolderMessage {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    fn type_name(&self) -> &'static str {
        "FolderMessage"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.names.folder_message)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        self.follow_sign_in();
        // The Sign In keeps its room under the text; the text gives up its room first.
        let button = self.children.get_mut(1).map(|button| {
            let size = button.layout(available);
            Size::new(size.width.min(available.width), size.height)
        });
        let button_room = button.map_or(0.0, |b| b.height + BUTTON_GAP);
        let text_area = Size::new(available.width, (available.height - button_room).max(0.0));
        let Some(scroll) = self.children.first_mut() else {
            return available;
        };
        let text_height = match scroll.children_mut().first_mut() {
            Some(text) => text.layout(text_area).height,
            None => 0.0,
        };
        self.icon_fits =
            self.icon.is_some() && ICON_SIZE + ICON_GAP + text_height <= text_area.height;
        let icon_height = match self.icon_fits {
            true => ICON_SIZE + ICON_GAP,
            false => 0.0,
        };
        let block = icon_height + text_height;
        // Y up: text that fits sits centred with the icon above it and the button under it;
        // taller text fills the room above the button and scrolls.
        let text_box = match block <= text_area.height {
            true => Rect::new(
                0.0,
                (text_area.height - block) / 2.0 + button_room,
                available.width,
                text_height,
            ),
            false => Rect::new(0.0, button_room, available.width, text_area.height),
        };
        scroll.layout(Size::new(text_box.width, text_box.height));
        scroll.set_bounds(text_box);
        if let (Some(size), Some(button)) = (button, self.children.get_mut(1)) {
            button.set_bounds(Rect::new(
                ((available.width - size.width) / 2.0).max(0.0),
                (text_box.y - BUTTON_GAP - size.height).max(0.0),
                size.width,
                size.height,
            ));
        }
        self.icon_rect = Rect::new(
            (available.width - ICON_SIZE) / 2.0,
            text_box.y + text_box.height + ICON_GAP,
            ICON_SIZE,
            ICON_SIZE,
        );
        available
    }
    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        if let Some(icon) = self.icon.as_ref().filter(|_| self.icon_fits) {
            paint_art(ctx, icon, self.icon_rect);
        }
    }
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
    fn properties(&self) -> Vec<(&'static str, String)> {
        vec![
            ("text", self.shown.text.clone()),
            ("icon", self.icon_fits.to_string()),
            ("sign_in", self.shown.action.clone().unwrap_or_default()),
        ]
    }
}

/// The message's wrapped text, inside its `ScrollView`: the label wrapped to the text's width
/// (the list's less [`SIDE_MARGIN`] each side, at most [`MAX_TEXT_WIDTH`]) and centred across
/// the width it is given, as tall as its lines.
struct MessageText {
    bounds: Rect,
    id: String,
    children: Vec<Box<dyn Widget>>,
}

impl Widget for MessageText {
    fn type_name(&self) -> &'static str {
        "FolderMessageText"
    }
    fn id(&self) -> Option<&str> {
        Some(&self.id)
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }
    fn layout(&mut self, available: Size) -> Size {
        let text_width = (available.width - 2.0 * SIDE_MARGIN).clamp(1.0, MAX_TEXT_WIDTH);
        let Some(label) = self.children.first_mut() else {
            return Size::new(available.width, 0.0);
        };
        let text = label.layout(Size::new(text_width, available.height));
        let left = ((available.width - text_width) / 2.0).max(0.0);
        label.set_bounds(Rect::new(left, 0.0, text_width, text.height));
        Size::new(available.width, text.height)
    }
    fn paint(&mut self, _: &mut dyn DrawCtx) {}
    fn on_event(&mut self, _: &Event) -> EventResult {
        EventResult::Ignored
    }
}
