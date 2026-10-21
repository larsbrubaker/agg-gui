//! Heading anchors for [`MarkdownView`]: the anchor a docs site gives each
//! heading ([`heading_slug`]), where each heading of the last layout sits
//! ([`MarkdownView::headings`]), and scrolling a [`ScrollView`] that holds a
//! view so a heading shows at its top
//! ([`ScrollView::scroll_to_markdown_heading`]).
//!
//! A host that links into a document by anchor (a help search result that
//! opens an article at a section) makes the same anchor the website's
//! Markdown converter makes, so a link written for the site lands on the
//! same heading here. The parse pass (`parse.rs`) keeps each Markdown
//! heading's source text and the layout pass (`layout.rs`) records the row
//! it starts on; this module turns those into anchors and offsets.

use crate::widget::Widget;
use crate::widgets::ScrollView;

use super::{LayoutItem, LineRun, MarkdownView};

/// One heading of a laid-out [`MarkdownView`].
#[derive(Clone, Debug, PartialEq)]
pub struct MarkdownHeading {
    /// The heading level, 1 for `#` to 6 for `######`.
    pub level: u8,
    /// The heading's text as drawn (without Markdown marks).
    pub text: String,
    /// The heading's anchor, unique in the document: [`heading_slug`] of its
    /// source text, with `-1`, `-2`, ... on a repeat of an earlier one.
    pub slug: String,
    /// How far the heading's first row starts below the top of the view, in
    /// the view's logical units: the vertical scroll offset that puts the
    /// heading at the top of a scroll view showing the view from its top.
    pub top: f64,
    /// The height of the heading's first row.
    pub height: f64,
}

/// The anchor a docs site built with kramdown's GitHub-flavoured Markdown
/// reader (the Jekyll default) gives a heading whose source text is `text`:
/// lower-cased, every character that is not a letter, a digit, `_`, `-` or a
/// space dropped, and each space turned into `-`. `## Set Up: Step 2!` is
/// `set-up-step-2`; letters of every script are kept (`## Größe` is
/// `größe`), while punctuation and emoji are dropped.
///
/// `text` is the heading's source between its marks, so Markdown syntax in
/// it counts as the site counts it: the `*` of `**bold**` and the brackets
/// of a link drop out, but a link's address is kept as text. A heading that
/// repeats an earlier one's anchor gets `-1`, `-2`, ... appended;
/// [`MarkdownView::headings`] applies that.
///
/// "Letter or digit" is Unicode's alphabetic and numeric classes
/// (`char::is_alphanumeric`), which also keeps numeric symbols such as `²`
/// that the site's word class leaves out; headings rarely hold them.
pub fn heading_slug(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|&c| c.is_alphanumeric() || c == '_' || c == '-' || c == ' ')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

impl MarkdownView {
    /// The Markdown headings (`#` and setext) of the last layout, in
    /// document order, each with its anchor and where it sits. Headings
    /// written as HTML elements (`<h2 align="center">`) are left out: a
    /// docs site gives them no anchor. Empty before the first layout; a
    /// layout at another width moves the offsets, so ask again after one.
    pub fn headings(&self) -> Vec<MarkdownHeading> {
        // The site counts each base anchor: its first use is bare, the
        // next gets `-1`, then `-2`; insertion order keeps the output stable.
        let mut counts: Vec<(String, usize)> = Vec::new();
        let mut headings = Vec::with_capacity(self.heading_rows.len());
        for (index, source) in &self.heading_rows {
            let Some(LayoutItem::Line {
                y, height, level, ..
            }) = self.items.get(*index)
            else {
                continue;
            };
            let base = heading_slug(source);
            let slug = match counts.iter_mut().find(|(b, _)| *b == base) {
                Some((_, n)) => {
                    *n += 1;
                    format!("{base}-{n}")
                }
                None => {
                    counts.push((base.clone(), 0));
                    base
                }
            };
            headings.push(MarkdownHeading {
                level: *level,
                text: self.heading_text(*index),
                slug,
                top: self.content_h - (y + height),
                height: *height,
            });
        }
        headings
    }

    /// Each heading's anchor and how far below the view's top it starts
    /// ([`MarkdownHeading::slug`] and [`MarkdownHeading::top`] of
    /// [`headings`](Self::headings)).
    pub fn heading_offsets(&self) -> Vec<(String, f64)> {
        self.headings()
            .into_iter()
            .map(|heading| (heading.slug, heading.top))
            .collect()
    }

    /// How far below the view's top the heading with anchor `slug` starts,
    /// or `None` when no heading of the last layout has it.
    pub fn heading_top(&self, slug: &str) -> Option<f64> {
        self.headings()
            .into_iter()
            .find(|heading| heading.slug == slug)
            .map(|heading| heading.top)
    }

    /// The drawn text of the heading starting at `items[first]`: its rows'
    /// text runs, a space where a row wraps.
    fn heading_text(&self, first: usize) -> String {
        let mut text = String::new();
        for (i, item) in self.items.iter().enumerate().skip(first) {
            let LayoutItem::Line {
                runs, continues, ..
            } = item
            else {
                break;
            };
            if i > first && !continues {
                break;
            }
            if i > first {
                text.push(' ');
            }
            for run in runs {
                if let LineRun::Text { text: t, .. } = run {
                    text.push_str(t);
                }
            }
        }
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

impl ScrollView {
    /// Scroll so the heading with anchor `slug` of the first
    /// [`MarkdownView`] in this scroll view's content shows at the top of
    /// the view, or as near the top as the content's end allows. The view
    /// may be the content itself or inside it (a column with a title above
    /// the document). Returns `false`, leaving the scroll alone, when there
    /// is no such view or heading. Positions come from the last layout, so
    /// call it after the view has been laid out (and again after a layout
    /// at a new width to keep the heading at the top).
    pub fn scroll_to_markdown_heading(&mut self, slug: &str) -> bool {
        let Some(content) = self.children().first() else {
            return false;
        };
        let Some((view, bottom)) = find_markdown_view(content.as_ref(), 0.0, true) else {
            return false;
        };
        let Some(top) = view.heading_top(slug) else {
            return false;
        };
        // The view's top, down from the content's top (Y-up bounds), plus
        // the heading's place in the view.
        let view_top = content.bounds().height - (bottom + view.bounds().height);
        let offset = (view_top + top).clamp(0.0, self.max_scroll_value().max(0.0));
        self.set_scroll_offset(offset);
        crate::animation::request_layout();
        true
    }
}

/// The first [`MarkdownView`] at or under `widget` (depth first, in child
/// order) and its bottom edge in the space of `widget`'s own content: the sum
/// of the bounds' `y` below `widget` (whose own `y` is its place in the
/// scroll view, not in the content). `root` marks that first call.
fn find_markdown_view(
    widget: &dyn Widget,
    bottom: f64,
    root: bool,
) -> Option<(&MarkdownView, f64)> {
    let bottom = if root {
        0.0
    } else {
        bottom + widget.bounds().y
    };
    if let Some(view) = widget
        .as_any()
        .and_then(|any| any.downcast_ref::<MarkdownView>())
    {
        return Some((view, bottom));
    }
    widget
        .children()
        .iter()
        .find_map(|child| find_markdown_view(child.as_ref(), bottom, false))
}
