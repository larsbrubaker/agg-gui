//! The character styles a toolbar button toggles and what the toolbar shows
//! about them: [`RichInlineStyle`] (C#'s `[Flags]` enum), [`RichStyleCoverage`],
//! [`RichStyleState`] and [`RichPendingStyle`].  Ports the type half of
//! agg-sharp's `RichStyleOperations.cs`; the operations that read and change
//! styles over a selection are [`super::style_operations`].

use std::ops::{BitAnd, BitOr, BitXor, Not};

use super::inline::RichInline;

/// A set of character styles (C#'s `[Flags] enum RichInlineStyle`).  Code is
/// independent of the others: applying it keeps any bold, italic or strike
/// (markdown writes **`x`** fine), so toggling Code off gives the text back
/// unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RichInlineStyle(u8);

impl RichInlineStyle {
    /// No style.
    pub const NONE: Self = Self(0);
    /// Bold (`**`).
    pub const BOLD: Self = Self(1);
    /// Italic (`*`).
    pub const ITALIC: Self = Self(2);
    /// Inline code (`` ` ``).
    pub const CODE: Self = Self(4);
    /// Strikethrough (`~~`).
    pub const STRIKE: Self = Self(8);
    /// Every style.
    pub const ALL: Self = Self(1 | 2 | 4 | 8);

    /// True when no flag is set.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// True when every flag of `other` is set here.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// True when any flag of `other` is set here.
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl BitOr for RichInlineStyle {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitAnd for RichInlineStyle {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl BitXor for RichInlineStyle {
    type Output = Self;
    fn bitxor(self, rhs: Self) -> Self {
        Self(self.0 ^ rhs.0)
    }
}

impl Not for RichInlineStyle {
    type Output = Self;
    /// The complement within the four styles.
    fn not(self) -> Self {
        Self(!self.0 & Self::ALL.0)
    }
}

/// How much of a selection has a style: none of it, some of it, or all of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RichStyleCoverage {
    /// No character has it.
    #[default]
    None,
    /// Some characters have it.
    Mixed,
    /// Every character has it.
    All,
}

/// The style over a selection, for showing toolbar buttons pressed, unpressed
/// or mixed.  `all` holds the flags every character has, `any` the flags at
/// least one has.  `link_url` and `link_title` are set only when the whole
/// range is one link, and for a caret inside or at either edge of a link (the
/// one [`super::style_operations::set_link`] and
/// [`super::style_operations::remove_link`] would act on), so the link button
/// shows pressed and edits that link — even at its end, where typed text would
/// not extend it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RichStyleState {
    /// The flags every character has.
    pub all: RichInlineStyle,
    /// The flags at least one character has.
    pub any: RichInlineStyle,
    /// The one link's target.
    pub link_url: Option<String>,
    /// The one link's title.
    pub link_title: Option<String>,
}

impl RichStyleState {
    /// Whether `style` covers all, some or none of the selection.
    pub fn coverage(&self, style: RichInlineStyle) -> RichStyleCoverage {
        if !style.is_none() && self.all.contains(style) {
            return RichStyleCoverage::All;
        }

        if self.any.intersects(style) {
            RichStyleCoverage::Mixed
        } else {
            RichStyleCoverage::None
        }
    }
}

/// Styles toggled at a collapsed caret (Ctrl+B with nothing selected): the
/// next typed text gets them flipped relative to the style it would otherwise
/// inherit.  It is a flip mask rather than absolute flags so pressing Bold
/// twice cancels out.  The widget owns it and clears it (back to default)
/// whenever the caret moves or the selection changes, because the inherited
/// style it is relative to changes with the caret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RichPendingStyle {
    /// The flags to flip.
    pub flipped: RichInlineStyle,
}

impl RichPendingStyle {
    /// True when nothing is pending.
    pub fn is_empty(&self) -> bool {
        self.flipped.is_none()
    }

    /// This pending style with `style` flipped once more.
    pub fn toggle(&self, style: RichInlineStyle) -> Self {
        Self {
            flipped: self.flipped ^ style,
        }
    }

    /// The template to pass as `insert_text`'s style: `base_style` (normally
    /// [`super::style_operations::typing_style`]) with the pending flags
    /// flipped.  Never mutates the base.
    pub fn apply_pending(&self, base_style: &RichInline) -> RichInline {
        let mut run = base_style.with_text("");
        let current = style_of(base_style);
        set_style(&mut run, self.flipped & !current, true);
        set_style(&mut run, self.flipped & current, false);
        run
    }
}

/// The flags an inline carries; Code only on a run.
pub(crate) fn style_of(inline: &RichInline) -> RichInlineStyle {
    let mut style = RichInlineStyle::NONE;
    if inline.bold {
        style = style | RichInlineStyle::BOLD;
    }
    if inline.italic {
        style = style | RichInlineStyle::ITALIC;
    }
    if inline.as_run().is_some_and(|run| run.code) {
        style = style | RichInlineStyle::CODE;
    }
    if inline.strike {
        style = style | RichInlineStyle::STRIKE;
    }
    style
}

/// Sets (or clears) each flag in `style` on the inline, leaving the other
/// flags alone.  Code applies to runs only; an atom has no inline-code form.
/// Returns true when anything changed.
pub(crate) fn set_style(inline: &mut RichInline, style: RichInlineStyle, on: bool) -> bool {
    let before = style_of(inline);
    if style.intersects(RichInlineStyle::BOLD) {
        inline.bold = on;
    }

    if style.intersects(RichInlineStyle::ITALIC) {
        inline.italic = on;
    }

    if style.intersects(RichInlineStyle::CODE) {
        if let Some(run) = inline.as_run_mut() {
            run.code = on;
        }
    }

    if style.intersects(RichInlineStyle::STRIKE) {
        inline.strike = on;
    }

    style_of(inline) != before
}
