//! One piece of a text-bearing block: a styled run of text or an atom the
//! caret steps over whole.  Ports agg-sharp's `RichInline.cs` (`RichInline`,
//! `RichRun`, `InlineAtomKind`, `InlineAtom`).
//!
//! C#'s abstract `RichInline` with its two subclasses becomes one struct
//! holding the shared style plus a [`RichInlineContent`] enum, so a list of
//! inlines is a plain `Vec<RichInline>` and cloning it is a deep copy.

use std::fmt;

/// A run's text or an atom's markdown: what C#'s `RichRun` / `InlineAtom`
/// subclasses add to the shared [`RichInline`] style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RichInlineContent {
    /// Text sharing one style.
    Run(RichRun),
    /// An inline the rich view does not edit inside.
    Atom(InlineAtom),
}

/// Text sharing one style.  Text never contains markdown syntax; the writer
/// adds delimiters and escaping.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichRun {
    /// The run's characters.
    pub text: String,
    /// Inline code (`` `code` ``).
    pub code: bool,
}

/// Which kind of markdown an [`InlineAtom`] holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InlineAtomKind {
    /// An image (`![alt](src)`).
    #[default]
    Image,
    /// Inline HTML.
    Html,
    /// An autolink (`<https://...>`).
    Autolink,
    /// A hard line break.
    HardBreak,
    /// Any other inline the model does not cover (mark, sub/superscript, task
    /// checkbox, footnote reference...), kept as its markdown so it survives a
    /// rewrite of its block.
    Raw,
}

/// An inline the rich view does not edit inside (image, inline HTML, autolink,
/// hard break).  It keeps its markdown verbatim so writing a dirty block
/// reproduces it exactly.  Always one caret position: the caret can sit before
/// or after an atom but never inside it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InlineAtom {
    /// What the markdown is.
    pub kind: InlineAtomKind,
    /// The atom's markdown, verbatim.
    pub raw_markdown: String,
}

/// One inline: its style plus its content.
///
/// The style lives here, not only on runs, because an atom keeps only its own
/// markdown: an image inside `**bold**` or a link must remember the bold or
/// link, or rewriting its block would drop it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichInline {
    /// Bold (`**`).
    pub bold: bool,
    /// Italic (`*`).
    pub italic: bool,
    /// Strikethrough (`~~`).
    pub strike: bool,
    /// The link target when this inline is (part of) a link's text.
    pub link_url: Option<String>,
    /// The link's title (`[text](url "title")`), or `None` when it has none.
    pub link_title: Option<String>,
    /// The reference label as written when the link is a reference link
    /// (`[text][label]`, `[label][]` or `[label]`), or `None` for an inline
    /// link.  When set, the writer emits `[text][label]` so the definition
    /// elsewhere in the document stays the link's source of truth; `link_url`
    /// is then only the resolved target, for clicking.
    pub link_label: Option<String>,
    /// A run's text or an atom's markdown.
    pub content: RichInlineContent,
}

impl RichInline {
    fn with_content(content: RichInlineContent) -> Self {
        Self {
            bold: false,
            italic: false,
            strike: false,
            link_url: None,
            link_title: None,
            link_label: None,
            content,
        }
    }

    /// An unstyled run of `text` (C#'s `new RichRun(text)`).
    pub fn run(text: impl Into<String>) -> Self {
        Self::with_content(RichInlineContent::Run(RichRun {
            text: text.into(),
            code: false,
        }))
    }

    /// An unstyled atom (C#'s `new InlineAtom(kind, rawMarkdown)`).
    pub fn atom(kind: InlineAtomKind, raw_markdown: impl Into<String>) -> Self {
        Self::with_content(RichInlineContent::Atom(InlineAtom {
            kind,
            raw_markdown: raw_markdown.into(),
        }))
    }

    /// This inline made bold (initializer sugar for `{ Bold = true }`).
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// This inline made italic.
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// This inline made strikethrough.
    pub fn strike(mut self) -> Self {
        self.strike = true;
        self
    }

    /// This run made inline code (no effect on an atom).
    pub fn code(mut self) -> Self {
        if let RichInlineContent::Run(run) = &mut self.content {
            run.code = true;
        }
        self
    }

    /// This inline linked to `url`.
    pub fn link(mut self, url: impl Into<String>) -> Self {
        self.link_url = Some(url.into());
        self
    }

    /// The run, when this inline is one (C#'s `as RichRun`).
    pub fn as_run(&self) -> Option<&RichRun> {
        match &self.content {
            RichInlineContent::Run(run) => Some(run),
            RichInlineContent::Atom(_) => None,
        }
    }

    /// The run, mutably, when this inline is one.
    pub fn as_run_mut(&mut self) -> Option<&mut RichRun> {
        match &mut self.content {
            RichInlineContent::Run(run) => Some(run),
            RichInlineContent::Atom(_) => None,
        }
    }

    /// The atom, when this inline is one (C#'s `as InlineAtom`).
    pub fn as_atom(&self) -> Option<&InlineAtom> {
        match &self.content {
            RichInlineContent::Atom(atom) => Some(atom),
            RichInlineContent::Run(_) => None,
        }
    }

    /// True for a run.
    pub fn is_run(&self) -> bool {
        self.as_run().is_some()
    }

    /// A run's text, or `None` for an atom.
    pub fn text(&self) -> Option<&str> {
        self.as_run().map(|run| run.text.as_str())
    }

    /// How many caret positions this inline occupies in its block's plain
    /// text: a run's character count, 1 for an atom.
    pub fn len(&self) -> usize {
        match &self.content {
            RichInlineContent::Run(run) => run.text.chars().count(),
            RichInlineContent::Atom(_) => 1,
        }
    }

    /// True for an empty run (an atom is never empty).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// True when the two inlines carry the same style (a run's code included;
    /// an atom has no code flag, so a run and an atom compare their code as
    /// `Some(false)` against `None`).  Only runs ever merge.
    pub fn has_same_style(&self, other: &RichInline) -> bool {
        self.bold == other.bold
            && self.italic == other.italic
            && self.strike == other.strike
            && self.link_url == other.link_url
            && self.link_title == other.link_title
            && self.link_label == other.link_label
            && self.as_run().map(|r| r.code) == other.as_run().map(|r| r.code)
    }

    /// A run with this inline's style and the given text (C#'s
    /// `RichRun.WithText`).  On an atom it gives a run with the atom's style.
    pub fn with_text(&self, text: impl Into<String>) -> RichInline {
        let code = self.as_run().map(|r| r.code).unwrap_or(false);
        RichInline {
            content: RichInlineContent::Run(RichRun {
                text: text.into(),
                code,
            }),
            ..self.clone()
        }
    }
}

impl fmt::Display for RichInline {
    /// A run's text, or an atom's markdown (C#'s `ToString`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.content {
            RichInlineContent::Run(run) => f.write_str(&run.text),
            RichInlineContent::Atom(atom) => f.write_str(&atom.raw_markdown),
        }
    }
}
