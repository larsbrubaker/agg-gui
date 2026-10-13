//! `RichMarkdownEditor` — the WYSIWYG Markdown editor as a host places it: the
//! formatting toolbar above the editing widget, the toolbar acting on the
//! widget's [`RichEditorState`](super::RichEditorState) through
//! [`RichEditCommands`](super::RichEditCommands) and the widget's Cmd/Ctrl+K
//! opening the toolbar's link row.  Ports agg-sharp's `RichMarkdownEditor.cs`.
//!
//! A host with its own link dialog adds a handler with
//! [`RichMarkdownEditor::on_link_requested`]; both the button and the
//! shortcut then go to it.
//!
//! The toolbar does not hold the editor (see `format_toolbar`): this widget
//! hands it the editor's state after every event that reaches it, at every
//! layout, and in each of its own calls ([`RichMarkdownEditor::sync`]), so a
//! click queued by a toolbar button runs within the frame, before the paint.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::draw_ctx::DrawCtx;
use crate::event::{Event, EventResult, Key, Modifiers};
use crate::geometry::{Rect, Size};
use crate::layout_props::{HAnchor, VAnchor, WidgetBase};
use crate::text::Font;
use crate::widget::Widget;

use super::edit_error::RichEditError;
use super::edit_widget::RichMarkdownEditWidget;
use super::format_toolbar::{
    MarkdownFormatButtonKind, MarkdownFormatButtonState, MarkdownFormatToolbar,
    MarkdownLinkRequestHandler,
};
use super::layout_style::{RichLayoutFonts, RichThemeColors};
use super::selection::RichSelection;

/// The toolbar over the editing widget; see the module header.
pub struct RichMarkdownEditor {
    bounds: Rect,
    base: WidgetBase,
    /// The toolbar, then the editing widget; always both, in that order.
    children: Vec<Box<dyn Widget>>,
    link_requested: Rc<Cell<bool>>,
}

impl RichMarkdownEditor {
    /// An editor over `fonts` in `theme`'s colours (C#'s default theme when
    /// `None`), with the toolbar's icons from `icon_font` (Font Awesome 4.7,
    /// [`crate::fonts::FONT_AWESOME_4_7`]).  It stretches both ways.
    pub fn new(
        fonts: RichLayoutFonts,
        icon_font: Arc<Font>,
        theme: Option<RichThemeColors>,
    ) -> Self {
        let regular = Arc::clone(&fonts.regular);
        let mut editor = RichMarkdownEditWidget::new(fonts, theme);
        let link_requested = Rc::new(Cell::new(false));
        let flag = Rc::clone(&link_requested);
        editor.on_link_requested(Box::new(move || flag.set(true)));
        let mac = editor.state().use_mac_key_bindings;
        let mut toolbar = MarkdownFormatToolbar::new(regular, icon_font, mac);
        toolbar.attach(editor.state_mut());
        let mut base = WidgetBase::new();
        base.h_anchor = HAnchor::STRETCH;
        base.v_anchor = VAnchor::STRETCH;
        Self {
            bounds: Rect::default(),
            base,
            children: vec![Box::new(toolbar), Box::new(editor)],
            link_requested,
        }
    }

    fn parts(&mut self) -> (&mut MarkdownFormatToolbar, &mut RichMarkdownEditWidget) {
        let (toolbar, editor) = self.children.split_at_mut(1);
        let toolbar = toolbar[0]
            .as_any_mut()
            .and_then(|w| w.downcast_mut::<MarkdownFormatToolbar>())
            .expect("child 0 is the toolbar");
        let editor = editor[0]
            .as_any_mut()
            .and_then(|w| w.downcast_mut::<RichMarkdownEditWidget>())
            .expect("child 1 is the editing widget");
        (toolbar, editor)
    }

    /// The formatting toolbar.
    pub fn toolbar(&self) -> &MarkdownFormatToolbar {
        self.children[0]
            .as_any()
            .and_then(|w| w.downcast_ref::<MarkdownFormatToolbar>())
            .expect("child 0 is the toolbar")
    }

    /// The editing widget.
    pub fn editor(&self) -> &RichMarkdownEditWidget {
        self.children[1]
            .as_any()
            .and_then(|w| w.downcast_ref::<RichMarkdownEditWidget>())
            .expect("child 1 is the editing widget")
    }

    /// The editing widget, for its handlers and settings.  Call
    /// [`Self::sync`] after changing its document or selection directly.
    pub fn editor_mut(&mut self) -> &mut RichMarkdownEditWidget {
        self.parts().1
    }

    /// Runs what the toolbar queued (clicks, size picks, the link row) and
    /// what the editor asked of it (Cmd/Ctrl+K, a refresh after a change).
    pub fn sync(&mut self) -> Result<(), RichEditError> {
        let link = self.link_requested.replace(false);
        let (toolbar, editor) = self.parts();
        if link {
            toolbar.show_link_editor(editor.state_mut());
        }
        toolbar.sync(editor.state_mut())
    }

    /// The document as markdown.
    pub fn markdown(&self) -> String {
        self.editor().markdown()
    }

    /// Loads a new document (and clears the undo history).  Loading is not
    /// an edit and raises no handler, so the toolbar is told directly.
    pub fn set_markdown(&mut self, markdown: &str) -> Result<(), RichEditError> {
        let (toolbar, editor) = self.parts();
        let result = editor.set_markdown(markdown);
        toolbar.refresh(editor.state_mut());
        result
    }

    /// Sets the selection, as a click in the text would.
    pub fn select(&mut self, selection: RichSelection) -> Result<(), RichEditError> {
        self.editor_mut().select(selection);
        self.sync()
    }

    /// The hint an empty document shows, focused or not.
    pub fn empty_hint(&self) -> Option<&str> {
        self.editor().empty_hint()
    }

    /// Sets the empty hint.
    pub fn set_empty_hint(&mut self, hint: Option<String>) {
        self.editor_mut().set_empty_hint(hint);
    }

    /// The editing area's theme colours.
    pub fn theme(&self) -> Option<RichThemeColors> {
        self.editor().theme()
    }

    /// Sets the editing area's theme.  The toolbar draws with agg-gui's
    /// visuals, so it follows the app's theme on its own.
    pub fn set_theme(&mut self, theme: Option<RichThemeColors>) -> Result<(), RichEditError> {
        self.editor_mut().set_theme(theme)
    }

    /// Presses a toolbar button (C#'s `Toolbar.XButton.InvokeClick()`).
    pub fn click(&mut self, kind: MarkdownFormatButtonKind) -> Result<(), RichEditError> {
        self.sync()?;
        let (toolbar, editor) = self.parts();
        toolbar.click(kind, editor.state_mut())
    }

    /// Picks a size-list entry, as the user would.
    pub fn set_size_index(&mut self, index: usize) -> Result<(), RichEditError> {
        self.sync()?;
        let (toolbar, editor) = self.parts();
        toolbar.set_size_index(index, editor.state_mut())
    }

    /// What toolbar button `kind` shows, after any pending refresh.
    pub fn button(&mut self, kind: MarkdownFormatButtonKind) -> MarkdownFormatButtonState {
        let _ = self.sync();
        self.toolbar().button(kind)
    }

    /// Sends a key to the editing widget (as when it has the focus), then
    /// lets the toolbar follow.
    pub fn key_down(&mut self, key: Key, modifiers: Modifiers) -> EventResult {
        let result = self
            .editor_mut()
            .on_event(&Event::KeyDown { key, modifiers });
        let _ = self.sync();
        result
    }

    /// The link row's OK (or Enter) with the url in its box.
    pub fn accept_link_editor(&mut self) -> Result<(), RichEditError> {
        self.sync()?;
        let (toolbar, editor) = self.parts();
        toolbar.accept_link_editor(editor.state_mut())
    }

    /// The link row's Remove.
    pub fn remove_link_editor(&mut self) -> Result<(), RichEditError> {
        self.sync()?;
        let (toolbar, editor) = self.parts();
        toolbar.remove_link_editor(editor.state_mut())
    }

    /// The link row's Cancel (or Escape).
    pub fn cancel_link_editor(&mut self) -> Result<(), RichEditError> {
        self.sync()?;
        let (toolbar, editor) = self.parts();
        toolbar.cancel_link_editor(editor.state_mut())
    }

    /// Links the selection to `url` from a host's own dialog (empty removes
    /// the link).
    pub fn apply_link(&mut self, url: &str) -> Result<(), RichEditError> {
        self.sync()?;
        let (toolbar, editor) = self.parts();
        toolbar.apply_link(url, editor.state_mut())
    }

    /// Removes the link at the caret or over the selection.
    pub fn remove_link(&mut self) -> Result<(), RichEditError> {
        self.sync()?;
        let (toolbar, editor) = self.parts();
        toolbar.remove_link(editor.state_mut())
    }

    /// Adds a handler that replaces the link row with a host's own dialog,
    /// raised with the current link's url by the Link button and Cmd/Ctrl+K.
    pub fn on_link_requested(&mut self, handler: MarkdownLinkRequestHandler) {
        self.parts().0.on_link_requested(handler);
    }
}

impl Widget for RichMarkdownEditor {
    crate::widgets::widget_as_any!();

    fn type_name(&self) -> &'static str {
        "RichMarkdownEditor"
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
    fn widget_base(&self) -> Option<&WidgetBase> {
        Some(&self.base)
    }
    fn widget_base_mut(&mut self) -> Option<&mut WidgetBase> {
        Some(&mut self.base)
    }

    // The toolbar on top at its own height; the editing widget fills the rest.
    fn layout(&mut self, available: Size) -> Size {
        let _ = self.sync();
        let (width, height) = (available.width, available.height);
        let (toolbar, editor) = self.parts();
        let bar = toolbar.layout(Size::new(width, height)).height.min(height);
        toolbar.set_bounds(Rect::new(0.0, height - bar, width, bar));
        let rest = (height - bar).max(0.0);
        editor.layout(Size::new(width, rest));
        editor.set_bounds(Rect::new(0.0, 0.0, width, rest));
        available
    }

    fn paint(&mut self, _ctx: &mut dyn DrawCtx) {}

    // Every event reaching here (a key the editor left, a click on the toolbar's
    // background) lets the toolbar catch up; a consumed click runs at the next layout.
    fn on_event(&mut self, _event: &Event) -> EventResult {
        let _ = self.sync();
        EventResult::Ignored
    }
}
