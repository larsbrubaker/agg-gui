//! Configuration of a `MarkdownView`: the builder methods and the plain
//! setters and getters a host uses to pick fonts, padding, the markdown text,
//! the highlighted matching text, image sources, link and image callbacks and
//! anchoring.
//!
//! The widget struct and its constructor live in `markdown.rs`; options that
//! belong to one feature (colours, emphasis, spacing, remote images) sit with
//! that feature's module.

use std::sync::Arc;

use crate::layout_props::{HAnchor, Insets, VAnchor};
use crate::text::Font;

use super::{HtmlStyleTheme, MarkdownView};

impl MarkdownView {
    /// The theme copied HTML is styled with (C#'s `MarkdownWidget.Theme`,
    /// which `StyleHtmlFragment` reads); [`HtmlStyleTheme::default`] when
    /// not set.
    pub fn with_html_theme(mut self, theme: HtmlStyleTheme) -> Self {
        self.html_theme = theme;
        self
    }

    /// Change the theme copied HTML is styled with.
    pub fn set_html_theme(&mut self, theme: HtmlStyleTheme) {
        self.html_theme = theme;
    }

    /// Draw code blocks with `font`, a monospace face (MarkdigAgg's
    /// `AggCodeBlockRenderer.GetMonoTypeFace`, Liberation Mono), so their
    /// leading spaces line up. Without one, code blocks use the body face.
    pub fn with_code_font(mut self, font: Arc<Font>) -> Self {
        self.code_font = Some(font);
        self
    }

    /// The face code blocks measure and draw with: the code font, else the
    /// body face.
    pub fn code_font(&self) -> Arc<Font> {
        match &self.code_font {
            Some(font) => Arc::clone(font),
            None => self.active_font(),
        }
    }

    pub fn with_font_size(mut self, size: f64) -> Self {
        self.font_size = size;
        self
    }
    pub fn with_padding(mut self, p: f64) -> Self {
        self.padding = p;
        self
    }

    /// The markdown source the view renders.
    pub fn markdown(&self) -> &str {
        &self.markdown
    }

    /// Replace the markdown in place (see "Updating in place" in the
    /// `markdown` module docs). A selection keeps its offsets into the
    /// rendered text, so text that is only appended to (a streaming reply)
    /// keeps the same words selected; offsets past the new end clamp to it.
    /// Setting the same text again does nothing.
    pub fn set_markdown(&mut self, markdown: &str) {
        if self.markdown == markdown {
            return;
        }
        self.markdown = markdown.to_string();
        crate::animation::request_layout();
    }

    /// Highlight every occurrence of `text` (agg-sharp's
    /// `MarkdownWidget.MatchingText`; rules in `matching.rs`).
    pub fn with_matching_text(mut self, text: impl Into<String>) -> Self {
        self.matching_text = Some(text.into());
        self
    }

    /// Set or clear the highlighted matching text on a live view, e.g. when a
    /// help search result opens an article. Setting the same value does
    /// nothing; `None` or whitespace-only text highlights nothing.
    pub fn set_matching_text(&mut self, text: Option<&str>) {
        if self.matching_text.as_deref() == text {
            return;
        }
        self.matching_text = text.map(str::to_string);
        crate::animation::request_layout();
    }

    /// The matching text set by [`set_matching_text`](Self::set_matching_text).
    pub fn matching_text(&self) -> Option<&str> {
        self.matching_text.as_deref()
    }

    /// Supply an image provider closure.
    ///
    /// The closure receives a URL/path string from the Markdown source and must
    /// return `Some((rgba_bytes, width, height))` or `None`.
    pub fn with_image_provider(
        mut self,
        provider: impl Fn(&str) -> Option<(Vec<u8>, u32, u32)> + 'static,
    ) -> Self {
        self.image_provider = Some(Box::new(provider));
        self
    }

    pub fn on_link_click(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_link_click = Some(Box::new(cb));
        self
    }

    pub fn on_image_open(mut self, cb: impl FnMut(&str) + 'static) -> Self {
        self.on_image_open = Some(Box::new(cb));
        self
    }

    pub fn with_margin(mut self, m: Insets) -> Self {
        self.base.margin = m;
        self
    }
    pub fn with_h_anchor(mut self, h: HAnchor) -> Self {
        self.base.h_anchor = h;
        self
    }
    pub fn with_v_anchor(mut self, v: VAnchor) -> Self {
        self.base.v_anchor = v;
        self
    }
}
