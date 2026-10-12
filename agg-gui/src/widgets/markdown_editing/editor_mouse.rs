//! The rich editor's mouse — a port of agg-sharp's `RichEditorMouse.cs`: a
//! click places the caret, a drag extends the selection (scrolling while the
//! mouse is held past the view's edge), shift-click extends from the anchor,
//! a double click selects a word and a triple click the block's (or table
//! cell's) text, and dragging after either grows the selection by whole words
//! or blocks.  A Raw block is selected whole by a click and raises the
//! raw-block-activated handlers on a double click.  Cmd/Ctrl+click on a link
//! (in a Raw block too) raises the link-clicked handlers; a plain click on
//! one is just a click.
//!
//! Differences from C#, where the mouse handler was part of the widget:
//! - Positions are editor coordinates (see `editor_state`); the widget
//!   passes the press's click count from agg-gui's
//!   [`crate::event::current_click_count`] and the event's modifiers, so no
//!   handler keeps its own double-click timing or reads a global key state.
//! - A press on the scroll bar never reaches the state (C#'s `OverText`):
//!   the widget keeps it.
//! - The auto-scroll timer is the widget's: while
//!   [`RichEditorState::auto_scrolling`] it calls
//!   [`RichEditorState::auto_scroll_step`] every 50 ms (C#'s
//!   `UiThread.RunOnIdle(AutoScrollStep, 0.05)`).
//! - The mouse cursor is answered by [`RichEditorState::cursor_at`] rather
//!   than set; links in a Raw block's rendered host are found through the
//!   widget's lookup (`set_raw_link_lookup`), since the hosts are widgets.
//! - A drag held past the edge of a view under 2 px tall keeps the floor
//!   of C#'s `Math.Clamp(y, 1, Height - 1)`, which threw there.
//! - The link modifier is the editor's command key: Cmd with mac bindings,
//!   Ctrl otherwise (C#'s hosts folded Command onto Control).  The Windows
//!   key is never read, as in C#.

use crate::cursor::CursorIcon;
use crate::event::{Modifiers, MouseButton};
use crate::geometry::Point;
use crate::layout_props::clamp_to_limits;

use super::block::{RichBlock, RichBlockKind};
use super::block_layout::RichBlockGeometry;
use super::block_painter::RichLayout;
use super::caret_navigator::block_text;
use super::char_class::is_letter_or_digit;
use super::document::DocPosition;
use super::edit_error::RichEditError;
use super::edit_operations::whole_block;
use super::editor_state::RichEditorState;
use super::layout_line::RichCaret;
use super::markdown_writer;
use super::selection::RichSelection;

// The most one auto-scroll step moves, so a mouse flung far past the edge still scrolls at a readable pace.
const MAX_AUTO_SCROLL_STEP: f64 = 40.0;

/// What the mouse remembers between events.
pub(crate) struct MouseState {
    dragging: bool,
    auto_scrolling: bool,
    last_drag: Point,
    // 1 by characters, 2 by words, 3 by blocks (cells in a table).
    select_clicks: u32,
    // The unit the press selected (a word, a block's text, a Raw block, or one position for a plain click):
    // a drag keeps it selected and grows away from it.
    pivot_start: DocPosition,
    pivot_end: DocPosition,
    pressed_raw_block: Option<usize>,
}

impl Default for MouseState {
    fn default() -> Self {
        Self {
            dragging: false,
            auto_scrolling: false,
            last_drag: Point::new(0.0, 0.0),
            select_clicks: 1,
            pivot_start: DocPosition::default(),
            pivot_end: DocPosition::default(),
            pressed_raw_block: None,
        }
    }
}

impl RichEditorState {
    // Nothing to hit test until the view has a width and its blocks are laid out.
    fn laid_out(&self) -> bool {
        self.view.width() > 0.0
            && self.view.style().is_some()
            && self.view.laid_out_count() == self.document.blocks.len()
    }

    fn link_modifier(&self, modifiers: Modifiers) -> bool {
        if self.use_mac_key_bindings {
            modifiers.meta
        } else {
            modifiers.ctrl
        }
    }

    /// A press at `position` (editor coordinates) with agg-gui's click count
    /// for it (`current_click_count`).
    pub fn mouse_down(
        &mut self,
        position: Point,
        button: MouseButton,
        clicks: u32,
        modifiers: Modifiers,
    ) -> Result<(), RichEditError> {
        if button != MouseButton::Left || !self.laid_out() {
            return Ok(());
        }

        super::editor_state::raise(&mut self.focus_requested);
        let point = self.to_view(position);
        let block_index = self.block_at(point.y);
        if self.link_modifier(modifiers) {
            if let Some(url) = self.link_at(block_index, point) {
                for handler in &mut self.link_clicked {
                    handler(&url);
                }
                return Ok(());
            }
        }

        self.mouse.dragging = true;
        self.mouse.pressed_raw_block = None;
        let hit = self.hit_test(block_index, point);
        let at = DocPosition::in_cell(block_index, hit.offset, hit.row, hit.column);
        if modifiers.shift {
            // The anchor stays where the selection started; only the caret follows the click.
            self.mouse.select_clicks = 1;
            self.mouse.pivot_start = self.selection.anchor;
            self.mouse.pivot_end = self.selection.anchor;
            self.extend_to(at, hit.at_line_end);
            return Ok(());
        }

        if self.document.blocks[block_index].kind == RichBlockKind::Raw {
            self.mouse.select_clicks = 1;
            self.mouse.pressed_raw_block = Some(block_index);
            let whole = whole_block(&self.document, block_index);
            self.mouse.pivot_start = whole.start();
            self.mouse.pivot_end = whole.end();
            self.set_selection_by_user(whole, false);
            if clicks == 2 {
                let offset = markdown_writer::source_offset_of(&self.document, block_index);
                for handler in &mut self.raw_block_activated {
                    handler(offset);
                }
            }
            return Ok(());
        }

        self.mouse.select_clicks = clicks.clamp(1, 3);
        let (start, end) = self.unit_at(at);
        self.mouse.pivot_start = start;
        self.mouse.pivot_end = end;
        if self.mouse.select_clicks == 1 {
            self.set_selection_by_user(RichSelection::at(at), hit.at_line_end);
        } else {
            self.set_selection_by_user(RichSelection::new(start, end), false);
        }
        Ok(())
    }

    /// The mouse moved to `position` (editor coordinates): a drag extends the
    /// selection, or starts auto-scrolling when held past the view's edge.
    pub fn mouse_move(&mut self, position: Point) {
        if !self.laid_out() || !self.mouse.dragging {
            return;
        }
        self.mouse.last_drag = position;
        if self.past_edge(position.y) != 0.0 {
            // Held past the edge, the view keeps scrolling until the mouse comes back or is released.
            if !self.mouse.auto_scrolling {
                self.auto_scroll_step();
            }
            return;
        }
        self.extend_to_point(position);
    }

    /// The button was released.
    pub fn mouse_up(&mut self) {
        self.mouse.dragging = false;
        self.mouse.pressed_raw_block = None;
    }

    /// Whether a drag held past the edge is scrolling: the widget then calls
    /// [`Self::auto_scroll_step`] every 50 ms.
    pub fn auto_scrolling(&self) -> bool {
        self.mouse.auto_scrolling
    }

    /// One auto-scroll step: scrolls toward the mouse held past the edge and
    /// extends the selection.  Stops (and returns false) once the mouse is
    /// back, released, or the document's end is reached.
    pub fn auto_scroll_step(&mut self) -> bool {
        let past = self.past_edge(self.mouse.last_drag.y);
        if !self.mouse.dragging || past == 0.0 {
            self.mouse.auto_scrolling = false;
            return false;
        }

        let before = self.scroll_offset_from_top();
        let step = (past / 2.0).clamp(-MAX_AUTO_SCROLL_STEP, MAX_AUTO_SCROLL_STEP);
        self.set_scroll_offset_from_top(before - step);
        // C# `Math.Clamp(lastDrag.Y, 1, editor.Height - 1)` throws once the
        // view is under 2 px tall (as `f64::clamp` panics); the floor wins.
        let y = clamp_to_limits(self.mouse.last_drag.y, 1.0, self.viewport_height - 1.0);
        self.extend_to_point(Point::new(self.mouse.last_drag.x, y));
        if self.scroll_offset_from_top() == before {
            // At the top or bottom of the document: nothing more to scroll to.
            self.mouse.auto_scrolling = false;
            return false;
        }
        self.mouse.auto_scrolling = true;
        true
    }

    /// The cursor to show over `position` (editor coordinates): a hand over a
    /// link while the link modifier is held, an arrow over a Raw block, an
    /// I-beam over text.
    pub fn cursor_at(&self, position: Point, modifiers: Modifiers) -> CursorIcon {
        if !self.laid_out() {
            return CursorIcon::Text;
        }
        let point = self.to_view(position);
        let block_index = self.block_at(point.y);
        if self.link_modifier(modifiers) && self.link_at(block_index, point).is_some() {
            CursorIcon::PointingHand
        } else if self.document.blocks[block_index].kind == RichBlockKind::Raw {
            CursorIcon::Default
        } else {
            CursorIcon::Text
        }
    }

    fn extend_to_point(&mut self, position: Point) {
        let point = self.to_view(position);
        let block_index = self.block_at(point.y);
        let hit = self.hit_test(block_index, point);
        self.extend_to(
            DocPosition::in_cell(block_index, hit.offset, hit.row, hit.column),
            hit.at_line_end,
        );
    }

    /// Grows the selection from the pressed unit to the unit at `position`,
    /// so the word, block or Raw block first selected stays selected
    /// whichever way the drag goes.
    fn extend_to(&mut self, position: DocPosition, at_line_end: bool) {
        if self.mouse.pressed_raw_block == Some(position.block_index) {
            // Still on the Raw block it started on: it stays selected whole.
            return;
        }
        let (unit_start, unit_end) = self.unit_at(position);
        let selection = if position >= self.mouse.pivot_end {
            RichSelection::new(self.mouse.pivot_start, unit_end)
        } else {
            RichSelection::new(self.mouse.pivot_end, unit_start)
        };
        let line_end = self.mouse.select_clicks == 1 && at_line_end;
        if selection != self.selection || line_end != self.caret_at_line_end {
            self.set_selection_by_user(selection, line_end);
        }
    }

    // The unit a position falls in for the current click count; a Raw block is always one unit.
    fn unit_at(&self, position: DocPosition) -> (DocPosition, DocPosition) {
        let block = &self.document.blocks[position.block_index];
        let with = |offset| DocPosition { offset, ..position };
        if block.kind == RichBlockKind::Raw {
            return (with(0), with(block.text_length(0, 0)));
        }
        match self.mouse.select_clicks {
            2 => {
                let (start, end) = word_at(block, position);
                (with(start), with(end))
            }
            3 => (
                with(0),
                with(block.text_length(position.row, position.column)),
            ),
            _ => (position, position),
        }
    }

    // How far above (positive) or below (negative) the view a y in editor coordinates is.
    fn past_edge(&self, y: f64) -> f64 {
        if y > self.viewport_height {
            y - self.viewport_height
        } else if y < 0.0 {
            y
        } else {
            0.0
        }
    }

    /// The block at a y in view coordinates: the one whose box holds it, else
    /// the nearest (above the first block is the first, below the last is the
    /// last).
    fn block_at(&self, y: f64) -> usize {
        let mut nearest = 0;
        let mut nearest_distance = f64::MAX;
        for i in 0..self.document.blocks.len() {
            let Some(bounds) = self.view.block_bounds(i) else {
                continue;
            };
            let (bottom, top) = (bounds.y, bounds.y + bounds.height);
            let distance = if y > top {
                y - top
            } else if y < bottom {
                bottom - y
            } else {
                0.0
            };
            if distance < nearest_distance {
                nearest = i;
                nearest_distance = distance;
            }
        }
        nearest
    }

    fn hit_test(&self, block_index: usize, point: Point) -> RichCaret {
        match (
            self.view.layout_of(block_index),
            self.view.origin_of(block_index),
        ) {
            (Some(layout), Some(origin)) => layout.hit_test(Point::new(point.x, point.y - origin)),
            _ => RichCaret::default(),
        }
    }

    /// The url of the link drawn under `point` (view coordinates).  Found from
    /// the drawn fragment rather than the hit-test caret, which can land just
    /// past the link's last character.
    fn link_at(&self, block_index: usize, point: Point) -> Option<String> {
        let block = &self.document.blocks[block_index];
        if block.kind == RichBlockKind::Raw {
            return self
                .raw_link_at
                .as_ref()
                .and_then(|lookup| lookup(block_index, point));
        }
        if !block.is_text_block() && block.kind != RichBlockKind::Table {
            return None;
        }

        let in_block = Point::new(point.x, point.y - self.view.origin_of(block_index)?);
        let (lines, inlines) = match self.view.layout_of(block_index)? {
            RichLayout::Table(table) => {
                let caret = table.hit_test(in_block);
                let cell = table.cell(caret.row, caret.column).ok()?;
                (&cell.lines, block.inlines_at(caret.row, caret.column)?)
            }
            RichLayout::Text(layout) => (&layout.lines, &block.inlines),
        };
        for line in lines {
            if in_block.y < line.bottom || in_block.y > line.top {
                continue;
            }
            for fragment in &line.fragments {
                let Some(inline) = fragment.inline_index.and_then(|i| inlines.get(i)) else {
                    continue;
                };
                if in_block.x >= fragment.x && in_block.x <= fragment.x + fragment.width {
                    if let Some(url) = inline.link_url.as_ref().filter(|u| !u.is_empty()) {
                        return Some(url.clone());
                    }
                }
            }
        }
        None
    }
}

/// The word (or run of spaces, or single other character) at a caret, as
/// offsets in its block or cell.  A caret just after a word, as a click on
/// its last letter's right half gives, selects that word.
pub fn word_at(block: &RichBlock, position: DocPosition) -> (usize, usize) {
    let text: Vec<char> = block_text(block, position.row, position.column)
        .chars()
        .collect();
    if text.is_empty() {
        return (0, 0);
    }

    let offset = position.offset;
    let mut index = offset.min(text.len() - 1);
    if offset > 0
        && class_of(text[(offset - 1).min(text.len() - 1)]) == 1
        && (offset >= text.len() || class_of(text[offset]) != 1)
    {
        index = (offset - 1).min(text.len() - 1);
    }

    let kind = class_of(text[index]);
    if kind >= 3 {
        return (index, index + 1);
    }
    let mut start = index;
    while start > 0 && class_of(text[start - 1]) == kind {
        start -= 1;
    }
    let mut end = index + 1;
    while end < text.len() && class_of(text[end]) == kind {
        end += 1;
    }
    (start, end)
}

// 1 word, 2 spaces, 3 anything else (selected one at a time: punctuation, line breaks and atoms).
fn class_of(c: char) -> u8 {
    if is_letter_or_digit(c) || c == '_' {
        1
    } else if c == ' ' || c == '\t' {
        2
    } else {
        3
    }
}
