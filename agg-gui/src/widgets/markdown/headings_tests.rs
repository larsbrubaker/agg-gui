//! Tests for `headings.rs`: the docs site's heading anchor rule
//! ([`heading_slug`]), each heading's anchor and place in a laid-out view
//! ([`MarkdownView::headings`]), and scrolling a scroll view so a heading
//! shows at its top ([`ScrollView::scroll_to_markdown_heading`]).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::color::Color;
use crate::framebuffer::Framebuffer;
use crate::geometry::Size;
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{App, Widget};
use crate::widgets::{FlexColumn, Label, ScrollView};

use super::{heading_slug, MarkdownColors, MarkdownView};

const TEST_FONT: &[u8] = include_bytes!("../../../../demo/assets/LiberationSans-Regular.ttf");
const SIDE: f64 = 300.0;

fn font() -> Arc<Font> {
    Arc::new(Font::from_slice(TEST_FONT).expect("test font"))
}

fn laid_out(markdown: &str, width: f64) -> MarkdownView {
    crate::widget::set_current_viewport(Size::new(800.0, 600.0));
    let mut view = MarkdownView::new(markdown, font());
    view.layout(Size::new(width, 4000.0));
    view
}

#[test]
fn a_slug_is_lower_case_with_dashes_for_spaces() {
    assert_eq!(heading_slug("Getting Started"), "getting-started");
    assert_eq!(heading_slug("STEP 2"), "step-2");
    assert_eq!(
        heading_slug("already-dashed_and_underscored"),
        "already-dashed_and_underscored"
    );
}

#[test]
fn a_slug_drops_punctuation_but_keeps_each_space_as_a_dash() {
    assert_eq!(heading_slug("Set Up: Step 2!"), "set-up-step-2");
    assert_eq!(heading_slug("What's new?"), "whats-new");
    assert_eq!(heading_slug("Cut & Paste"), "cut--paste");
    assert_eq!(heading_slug("A -- B"), "a----b");
    assert_eq!(heading_slug("(Optional) Settings."), "optional-settings");
}

#[test]
fn a_slug_keeps_letters_of_every_script_and_drops_emoji() {
    assert_eq!(heading_slug("Größe ändern"), "größe-ändern");
    assert_eq!(heading_slug("Привет Мир"), "привет-мир");
    assert_eq!(heading_slug("設定の変更"), "設定の変更");
    assert_eq!(heading_slug("🚀 Launch"), "-launch");
    assert_eq!(heading_slug("Done ✅"), "done-");
}

#[test]
fn headings_carry_unique_anchors_made_from_their_source() {
    let view = laid_out(
        "# Intro\n\nSome text.\n\n## Setup\n\n## Setup ##\n\nSetup\n-----\n\n\
         ## Use **bold** and `code`\n\n## See [the notes](notes.md)\n\n\
         <h2 align=\"center\">Centered</h2>\n\n## Größe\n\n## Setup-1\n",
        600.0,
    );
    let headings = view.headings();
    let slugs: Vec<&str> = headings.iter().map(|h| h.slug.as_str()).collect();
    // A repeat counts up from -1; an explicit "Setup-1" after the counted
    // one repeats it as the site's per-anchor count does. The HTML heading
    // has no anchor on the site and is left out.
    assert_eq!(
        slugs,
        vec![
            "intro",
            "setup",
            "setup-1",
            "setup-2",
            "use-bold-and-code",
            "see-the-notesnotesmd",
            "größe",
            "setup-1",
        ]
    );
    let levels: Vec<u8> = headings.iter().map(|h| h.level).collect();
    assert_eq!(levels, vec![1, 2, 2, 2, 2, 2, 2, 2]);
    assert_eq!(headings[4].text, "Use bold and code");
    assert_eq!(headings[5].text, "See the notes");
    assert_eq!(
        view.heading_offsets()
            .into_iter()
            .map(|(slug, _)| slug)
            .collect::<Vec<_>>(),
        slugs
    );
}

#[test]
fn heading_offsets_increase_down_the_document() {
    let view = laid_out(
        "# One\n\nA paragraph.\n\n## Two\n\nMore text\nover two lines.\n\n### Three\n\n```\ncode\n```\n\n## Four\n",
        400.0,
    );
    let tops: Vec<f64> = view
        .heading_offsets()
        .into_iter()
        .map(|(_, top)| top)
        .collect();
    assert_eq!(tops.len(), 4);
    // The first heading starts on the view's top padding.
    assert!((tops[0] - 8.0).abs() < 1e-9, "{tops:?}");
    assert!(tops.windows(2).all(|w| w[1] > w[0]), "{tops:?}");
    assert!(tops[3] < view.bounds().height);
    assert_eq!(view.heading_top("three"), Some(tops[2]));
    assert_eq!(view.heading_top("missing"), None);
}

/// Rows (top-down, as offsets from the view's top) that hold ink.
fn ink_rows(view: &mut MarkdownView) -> Vec<f64> {
    let size = view.bounds();
    let (w, h) = (size.width as u32, size.height as u32);
    let mut fb = Framebuffer::new(w, h);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.set_fill_color(Color::white());
        ctx.begin_path();
        ctx.rect(0.0, 0.0, size.width, size.height);
        ctx.fill();
        view.paint(&mut ctx);
    }
    // Framebuffer rows are bottom-up: row r spans y-up r..r+1.
    fb.pixels()
        .chunks_exact(w as usize * 4)
        .enumerate()
        .filter(|(_, row)| row.chunks_exact(4).any(|p| p[0] < 128))
        .map(|(r, _)| size.height - (r as f64 + 0.5))
        .collect()
}

#[test]
fn heading_offsets_match_where_the_headings_draw() {
    let mut view = MarkdownView::new("# First\n\n## Second\n\n### Third\n\n#### Fourth\n", font())
        .with_colors(MarkdownColors {
            text: Color::black(),
            link: Color::black(),
            code_background: Color::white(),
            rule: Color::black(),
            background: None,
        });
    crate::widget::set_current_viewport(Size::new(800.0, 600.0));
    view.layout(Size::new(300.0, 4000.0));
    let headings = view.headings();
    assert_eq!(headings.len(), 4);
    let ink = ink_rows(&mut view);
    assert!(!ink.is_empty());
    // A document of headings only: every inked row is in a heading's band...
    for y in &ink {
        assert!(
            headings
                .iter()
                .any(|h| *y >= h.top && *y <= h.top + h.height),
            "ink at {y} lies outside every heading: {headings:?}"
        );
    }
    // ...and every heading's band has ink, its glyphs in the band's upper part.
    for h in &headings {
        let rows: Vec<f64> = ink
            .iter()
            .copied()
            .filter(|y| *y >= h.top && *y <= h.top + h.height)
            .collect();
        assert!(!rows.is_empty(), "no ink for {h:?}");
        let first = rows.iter().copied().fold(f64::INFINITY, f64::min);
        assert!(first < h.top + h.height * 0.5, "{h:?} inks from {first}");
    }
}

#[test]
fn a_layout_at_a_new_width_moves_the_offsets() {
    let markdown = format!("{}\n\n## Below\n", "word ".repeat(80));
    crate::widget::set_current_viewport(Size::new(800.0, 600.0));
    let mut view = MarkdownView::new(markdown, font());
    view.layout(Size::new(600.0, 4000.0));
    let wide = view.heading_top("below").expect("heading");
    view.layout(Size::new(200.0, 4000.0));
    let narrow = view.heading_top("below").expect("heading");
    assert!(
        narrow > wide,
        "the paragraph wraps to more rows: {wide} -> {narrow}"
    );
}

fn long_document() -> String {
    let mut markdown = String::from("# Title\n\nOpening text.\n");
    for section in 1..=6 {
        markdown.push_str(&format!("\n## Section {section}\n"));
        for line in 0..8 {
            markdown.push_str(&format!("\nParagraph {line} of section {section}.\n"));
        }
    }
    markdown
}

fn app_with(content: Box<dyn Widget>) -> (App, Rc<Cell<f64>>) {
    crate::widget::set_current_viewport(Size::new(SIDE, SIDE));
    let offset = Rc::new(Cell::new(0.0));
    let scroll = ScrollView::new(content).with_offset_cell(Rc::clone(&offset));
    let mut app = App::new(Box::new(scroll));
    app.layout(Size::new(SIDE, SIDE));
    (app, offset)
}

fn scroll_view(app: &mut App) -> &mut ScrollView {
    app.root_mut()
        .as_any_mut()
        .expect("as_any_mut")
        .downcast_mut::<ScrollView>()
        .expect("scroll view")
}

/// The markdown view under the scroll view and its bottom in the scroll
/// view's own space.
fn markdown_in(app: &App) -> (&MarkdownView, f64) {
    let content = &app.root().children()[0];
    let mut y = content.bounds().y;
    let mut widget: &dyn Widget = content.as_ref();
    loop {
        if let Some(view) = widget
            .as_any()
            .and_then(|a| a.downcast_ref::<MarkdownView>())
        {
            return (view, y);
        }
        widget = widget.children().last().expect("a child").as_ref();
        y += widget.bounds().y;
    }
}

/// Where the heading `slug` starts, down from the top of the scroll view.
fn heading_on_screen(app: &App, slug: &str) -> f64 {
    let (view, bottom) = markdown_in(app);
    let top = view.heading_top(slug).expect("heading");
    SIDE - (bottom + view.bounds().height - top)
}

#[test]
fn scrolling_to_a_heading_brings_it_to_the_top() {
    let (mut app, offset) = app_with(Box::new(MarkdownView::new(long_document(), font())));
    assert!(
        heading_on_screen(&app, "section-3") > SIDE,
        "starts below the view"
    );

    assert!(scroll_view(&mut app).scroll_to_markdown_heading("section-3"));
    app.layout(Size::new(SIDE, SIDE));

    let at = heading_on_screen(&app, "section-3");
    assert!(at.abs() <= 1.0, "the heading is at the top: {at}");
    assert!(offset.get() > 0.0);
}

#[test]
fn scrolling_to_a_heading_under_a_title_counts_the_title() {
    let column = FlexColumn::new()
        .add(Box::new(Label::new("Article title", font())))
        .add(Box::new(MarkdownView::new(long_document(), font())));
    let (mut app, _) = app_with(Box::new(column));

    assert!(scroll_view(&mut app).scroll_to_markdown_heading("section-2"));
    app.layout(Size::new(SIDE, SIDE));

    let at = heading_on_screen(&app, "section-2");
    assert!(at.abs() <= 1.0, "the heading is at the top: {at}");
}

#[test]
fn scrolling_to_the_last_heading_stops_at_the_content_end() {
    // A last section of one line: its heading cannot reach the top.
    let markdown = format!("{}\n## Last\n\nOne line.\n", long_document());
    let (mut app, offset) = app_with(Box::new(MarkdownView::new(markdown, font())));

    assert!(scroll_view(&mut app).scroll_to_markdown_heading("last"));
    app.layout(Size::new(SIDE, SIDE));

    // The scroll view keeps whole-pixel offsets, so the end is within one.
    let max = scroll_view(&mut app).max_scroll_value();
    assert!(
        (offset.get() - max).abs() <= 1.0,
        "scrolled to the end: {} vs {max}",
        offset.get()
    );
    let at = heading_on_screen(&app, "last");
    assert!(
        at > 100.0 && at < SIDE,
        "as near the top as the end allows: {at}"
    );
}

#[test]
fn scrolling_to_an_unknown_heading_leaves_the_scroll_alone() {
    let (mut app, offset) = app_with(Box::new(MarkdownView::new(long_document(), font())));
    assert!(scroll_view(&mut app).scroll_to_markdown_heading("section-4"));
    app.layout(Size::new(SIDE, SIDE));
    let before = offset.get();

    assert!(!scroll_view(&mut app).scroll_to_markdown_heading("no-such-section"));
    app.layout(Size::new(SIDE, SIDE));
    assert_eq!(offset.get(), before);
}
