//! agg-sharp's `RichMarkdownParserTests`
//! (`Tests/Agg.Tests/MarkdigAgg/RichMarkdownParserTests.cs`), ported 1:1
//! against `markdown_editing::markdown_parser`.

use agg_gui::widgets::markdown_editing::markdown_parser::parse;
use agg_gui::widgets::markdown_editing::markdown_writer;
use agg_gui::widgets::markdown_editing::{
    InlineAtomKind, RichBlock, RichBlockKind, RichDocument, RichInline,
};

use crate::rich_markdown_parser_code_table_tests::SAMPLES;

// Each string stresses one way the block partition or span arithmetic could drift by a byte.
fn corpus() -> Vec<&'static str> {
    let mut corpus = vec![
        "",
        "\n",
        "\n\n\n",
        "Just text",
        "Just text\n",
        "Line one\nline two\n\n\n\nAfter three blank lines\n\n",
        "---\ntitle: Notes\ntags: [a, b]\n---\n# Heading\n\nBody\n",
        "---\r\ntitle: x\r\n---\r\n\r\nText\r\n",
        "---\nnot closed\n\nText\n",
        "# One\r\n\r\nA **bold** and *it*\r\nsecond line\r\n\r\n## Two\r\n",
        "No trailing newline after **this**",
        "Setext One\n==========\n\nSetext Two\n---\n\nAfter\n",
        "See [the docs][docs] and [inline](http://x.com).\n\n[docs]: http://docs.example.com\n",
        "[docs]: http://docs.example.com\n\nUse [docs].\n",
        "[a]: /a\n[b]: /b \"Title\"\nText after defs\n",
        "***both*** **bold *nested italic* bold** _under_ __double__ ~~gone~~\n",
        "Inline <span style=\"color:red\">html</span> here\n",
        "An image ![alt text](images/a.png \"t\") and <http://auto.link> and www.bare.com\n",
        "Escapes \\*not italic\\* \\[x\\] &amp; &copy; `code ``with`` ticks`\n",
        "Hard  \nbreak and back\\\nslash break\n",
        "- item\n- item two\n\n> quote\n> more\n\n```cs\ncode\n```\n\n    indented code\n\n---\n\n| a | b |\n|---|:-:|\n| 1 | 2 |\n",
        "<div align=\"center\">\n\nCentered\n\n</div>\n",
        "  Indented paragraph\n\n   ### Indented heading ###   \n",
        "Para\n- list right after\n\nText ^sup^ and ~sub~ ==m== ++i++ **^b^**\r\n",
        "* [ ] task\n* [x] done\n\n\n",
        "Title link [t](/u \"title\")\n",
        "trailing spaces   \n\n  \t\n",
        // Lists, quotes and alignment groups (see rich_markdown_parser_block_tests for their shapes).
        "- a\n  - b\n    - c\n  - d\n- e\n",
        "3. three\n4. four\n\n10) ten\n11) eleven\n",
        "* star\n* star two\n\n+ plus\n+ plus two\r\n",
        "- loose\n\n- items\n\n\n- far apart\n",
        "- [ ] task\n- [x] done\n  wrapped line\n",
        "- one para\n\n  second para\n- next\n",
        "- item\n\n  ```\n  code\n  ```\n",
        "> one para\n> two lines\n",
        "> first\n>\n> second\n>\n",
        "  > indented\r\n  >\r\n  > quote\r\n",
        "> outer\n>\n> > nested\n",
        "<div align=\"center\">\n\n# Title\n\nCentered **text**\n\nMore\n\n</div>\n\nAfter\n",
        "<div align=\"right\">\r\n\r\nRight\r\n\r\n</div>",
        "<div align=\"center\">\n\nNever closed\n",
        "<div>\n\n<div align=\"center\">\n\nNested\n\n</div>\n\n</div>\n",
        "<div align=\"center\">\n\n- list\n\n</div>\n",
    ];
    // Code blocks and tables (see rich_markdown_parser_code_table_tests for their shapes).
    corpus.extend(SAMPLES);
    corpus
}

fn shape(document: &RichDocument) -> String {
    document
        .blocks
        .iter()
        .map(|b| format!("{:?}:{}", b.kind, b.original_source))
        .collect::<Vec<_>>()
        .join("|")
}

fn plain_text(block: &RichBlock) -> String {
    block
        .inlines
        .iter()
        .map(|inline| inline.as_run().map_or("?".to_string(), |r| r.text.clone()))
        .collect()
}

fn single(document: &RichDocument) -> &RichBlock {
    assert_eq!(document.blocks.len(), 1, "{}", shape(document));
    &document.blocks[0]
}

fn runs(block: &RichBlock) -> Vec<&RichInline> {
    assert!(block.inlines.iter().all(RichInline::is_run));
    block.inlines.iter().collect()
}

fn text(inline: &RichInline) -> &str {
    inline.text().unwrap_or("")
}

#[test]
fn unedited_document_round_trips_byte_for_byte() {
    for markdown in corpus() {
        let document = parse(markdown);
        assert_eq!(markdown_writer::write(&document), markdown, "{markdown:?}");

        // Separators and trailing text are whitespace only, or content would be hidden from the rich view.
        for block in &document.blocks {
            assert!(
                block.separator_before.trim().is_empty(),
                "{markdown:?}: {}",
                shape(&document)
            );
            assert!(!block.original_source.is_empty(), "{markdown:?}");
            // A block starting on a line break means a span began early and swallowed the gap before it.
            assert!(
                !block.original_source.starts_with(['\n', '\r']),
                "{markdown:?}: {}",
                shape(&document)
            );
        }
        assert!(document.trailing_text.trim().is_empty(), "{markdown:?}");
    }
}

#[test]
fn frontmatter_is_hidden_and_not_a_block() {
    let document = parse("---\ntitle: Notes\n---\n# Heading\n\nBody\n");
    assert_eq!(document.frontmatter, "---\ntitle: Notes\n---\n");
    let kinds: Vec<_> = document.blocks.iter().map(|b| b.kind).collect();
    assert_eq!(kinds, [RichBlockKind::Heading, RichBlockKind::Paragraph]);
    assert_eq!(document.blocks[1].separator_before, "\n\n");
    assert_eq!(document.trailing_text, "\n");
}

#[test]
fn headings_keep_level_and_text() {
    let document = parse("## Two\r\n\r\nSetext\r\n---\r\n");
    assert_eq!(document.blocks[0].kind, RichBlockKind::Heading);
    assert_eq!(document.blocks[0].heading_level, 2);
    assert_eq!(plain_text(&document.blocks[0]), "Two");
    assert_eq!(document.blocks[1].kind, RichBlockKind::Heading);
    assert_eq!(document.blocks[1].heading_level, 2);
    assert_eq!(document.blocks[1].original_source, "Setext\r\n---");
    assert_eq!(plain_text(&document.blocks[1]), "Setext");
}

#[test]
fn emphasis_becomes_run_styles() {
    let document = parse("a **b *c* d** ***e*** ~~f~~ `g`\n");
    let block = single(&document);
    assert_eq!(block.kind, RichBlockKind::Paragraph);
    let runs = runs(block);
    assert_eq!(
        runs.iter().map(|r| text(r)).collect::<Vec<_>>().join("|"),
        "a |b |c| d| |e| |f| |g"
    );
    assert!(runs[1].bold && !runs[1].italic);
    assert!(runs[2].bold && runs[2].italic);
    assert!(runs[5].bold && runs[5].italic);
    assert!(runs[7].strike);
    assert!(runs[9].as_run().is_some_and(|r| r.code));
    assert!(!(runs[0].bold || runs[0].italic || runs[0].as_run().is_some_and(|r| r.code)));
}

#[test]
fn links_set_link_url_on_their_runs() {
    let document = parse("[go **there**](http://x.com) or [ref]\n\n[ref]: /r\n");
    let runs = runs(&document.blocks[0]);
    assert_eq!(text(runs[0]), "go ");
    assert_eq!(runs[0].link_url.as_deref(), Some("http://x.com"));
    assert_eq!(text(runs[1]), "there");
    assert!(runs[1].bold);
    assert_eq!(runs[1].link_url.as_deref(), Some("http://x.com"));
    assert_eq!(runs[2].link_url, None);
    assert_eq!(text(runs[3]), "ref");
    assert_eq!(runs[3].link_url.as_deref(), Some("/r"));
    assert_eq!(runs[0].link_label, None);
    assert_eq!(runs[3].link_label.as_deref(), Some("ref"));
}

#[test]
fn reference_links_keep_their_label_as_written() {
    let document = parse(
        "[t][My Ref], [r2][] [r3] [i](/i \"T\")\n\n[my ref]: /a\n[r2]: /b \"Title\"\n[r3]: /c\n",
    );
    let links: Vec<String> = runs(&document.blocks[0])
        .into_iter()
        .filter(|r| r.link_url.is_some())
        .map(|r| {
            format!(
                "{}>{}>{}>{}",
                text(r),
                r.link_url.as_deref().unwrap_or(""),
                r.link_label.as_deref().unwrap_or("null"),
                r.link_title.as_deref().unwrap_or("null")
            )
        })
        .collect();
    assert_eq!(
        links.join("|"),
        "t>/a>My Ref>null|r2>/b>r2>null|r3>/c>r3>null|i>/i>null>T"
    );
}

#[test]
fn definitions_at_the_top_of_a_paragraph_become_their_own_raw_block() {
    let document = parse("[a]: /a\n[b]: /b \"Title\"\nText after defs\n");
    assert_eq!(
        shape(&document),
        "Raw:[a]: /a\n[b]: /b \"Title\"|Paragraph:Text after defs"
    );
    assert_eq!(document.blocks[1].separator_before, "\n");
    assert_eq!(plain_text(&document.blocks[1]), "Text after defs");

    let indented = parse("Intro\r\n\r\n [a]: /a\r\n  **Bold** text\r\n");
    // A Raw block keeps the indentation of its first line; a modelled block's goes to its separator.
    assert_eq!(
        shape(&indented),
        "Paragraph:Intro|Raw: [a]: /a|Paragraph:**Bold** text"
    );
    assert_eq!(indented.blocks[2].separator_before, "\r\n  ");
}

#[test]
fn heading_closing_sequence_belongs_to_the_heading() {
    let document = parse("# H #\n## Two ##   \r\n### Three   \nText\n");
    assert_eq!(
        shape(&document),
        "Heading:# H #|Heading:## Two ##   |Heading:### Three   |Paragraph:Text"
    );
    assert_eq!(plain_text(&document.blocks[0]), "H");
    assert_eq!(plain_text(&document.blocks[1]), "Two");
}

#[test]
fn reference_definitions_survive_as_raw_blocks() {
    let document = parse("Use [docs].\n\n[docs]: http://d.com\n");
    assert_eq!(
        shape(&document),
        "Paragraph:Use [docs].|Raw:[docs]: http://d.com"
    );
}

#[test]
fn atoms_keep_their_exact_source() {
    let document = parse("![a](b.png) <span>x</span> <http://a.com> www.c.com end  \nnext\n");
    let block = single(&document);
    let atoms: Vec<_> = block
        .inlines
        .iter()
        .filter_map(RichInline::as_atom)
        .collect();
    let kinds: Vec<_> = atoms.iter().map(|a| a.kind).collect();
    assert_eq!(
        kinds,
        [
            InlineAtomKind::Image,
            InlineAtomKind::Html,
            InlineAtomKind::Html,
            InlineAtomKind::Autolink,
            InlineAtomKind::Autolink,
            InlineAtomKind::HardBreak,
        ]
    );
    assert_eq!(atoms[0].raw_markdown, "![a](b.png)");
    assert_eq!(atoms[1].raw_markdown, "<span>");
    assert_eq!(atoms[2].raw_markdown, "</span>");
    assert_eq!(atoms[3].raw_markdown, "<http://a.com>");
    assert_eq!(atoms[4].raw_markdown, "www.c.com");
    assert_eq!(atoms[5].raw_markdown, "  \n");
    assert_eq!(plain_text(block), "? ?x? ? ? end?next");
}

#[test]
fn paired_style_tags_become_styles() {
    let styled_doc = parse("a<b>x<i>y</i></b><del>z</del><S>w</S>\n");
    let styled = single(&styled_doc);
    assert_eq!(
        styled
            .inlines
            .iter()
            .filter(|i| i.as_atom().is_some())
            .count(),
        0
    );
    let described: Vec<String> = runs(styled)
        .into_iter()
        .map(|r| {
            format!(
                "{}{}{}{}",
                text(r),
                if r.bold { "B" } else { "" },
                if r.italic { "I" } else { "" },
                if r.strike { "S" } else { "" }
            )
        })
        .collect();
    assert_eq!(described.join(","), "a,xB,yBI,zwS");

    // Attributes, an unpaired tag or crossed pairs keep their tags as atoms.
    for markdown in ["<b class=\"k\">x</b>\n", "a <b>x\n", "<b><i>x</b></i>\n"] {
        let document = parse(markdown);
        let block = single(&document);
        assert!(
            block
                .inlines
                .iter()
                .filter(|i| i.as_atom().is_some())
                .count()
                > 0,
            "{markdown:?}"
        );
        assert!(
            !block.inlines.iter().any(|i| i.bold || i.italic),
            "{markdown:?}"
        );
    }
}

#[test]
fn crlf_hard_breaks_keep_their_whole_source() {
    let spaces_doc = parse("a  \r\nb\r\n");
    let spaces: Vec<_> = single(&spaces_doc)
        .inlines
        .iter()
        .filter_map(RichInline::as_atom)
        .collect();
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0].raw_markdown, "  \r\n");

    let document = parse("a\\\r\nb\r\n");
    let block = single(&document);
    let atoms: Vec<_> = block
        .inlines
        .iter()
        .filter_map(RichInline::as_atom)
        .collect();
    assert_eq!(atoms.len(), 1);
    assert_eq!(atoms[0].raw_markdown, "\\\r\n");
    assert_eq!(plain_text(block), "a?b");
}

#[test]
fn escapes_and_entities_read_as_plain_text() {
    let document = parse("\\*a\\* &amp; b\nc\n");
    assert_eq!(plain_text(single(&document)), "*a* & b c");
}

#[test]
fn unmodelled_blocks_are_raw() {
    let document = parse("---\n\n+---+\n| a |\n+---+\n\n<div>\nx\n</div>\n");
    assert!(
        document.blocks.iter().all(|b| b.kind == RichBlockKind::Raw),
        "{}",
        shape(&document)
    );
    assert_eq!(document.blocks.len(), 3);
}

#[test]
fn titled_link_keeps_its_title_on_the_runs() {
    let document = parse("See [the *docs*](/u \"The title\") now\n");
    let block = single(&document);
    assert_eq!(block.kind, RichBlockKind::Paragraph);
    let runs = runs(block);
    assert_eq!(
        runs.iter().map(|r| text(r)).collect::<Vec<_>>().join("|"),
        "See |the |docs| now"
    );
    assert_eq!(runs[1].link_url.as_deref(), Some("/u"));
    assert_eq!(runs[1].link_title.as_deref(), Some("The title"));
    assert_eq!(runs[2].link_title.as_deref(), Some("The title"));
    assert!(runs[2].italic);
    assert_eq!(runs[3].link_title, None);
}

#[test]
fn unmodelled_inlines_become_raw_atoms_of_their_source() {
    let document = parse("a ^sup^ b ~sub~ c ==mark== d ++ins++ e\n");
    let block = single(&document);
    assert_eq!(block.kind, RichBlockKind::Paragraph);
    let atoms: Vec<_> = block
        .inlines
        .iter()
        .filter_map(RichInline::as_atom)
        .collect();
    assert!(atoms.iter().all(|a| a.kind == InlineAtomKind::Raw));
    let sources: Vec<_> = atoms.iter().map(|a| a.raw_markdown.as_str()).collect();
    assert_eq!(sources.join("|"), "^sup^|~sub~|==mark==|++ins++");
    assert_eq!(plain_text(block), "a ? b ? c ? d ? e");
}

#[test]
fn dirty_list_item_is_regenerated() {
    let mut document = parse("* item\n");
    document.blocks[0].dirty = true;
    assert_eq!(markdown_writer::write(&document), "* item\n");
}
