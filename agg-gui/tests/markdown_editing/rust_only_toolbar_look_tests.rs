//! The formatting toolbar's look, as agg-gui's own rich editor draws it (no C# counterpart; the
//! look is agg-gui's, built from its theme's visuals): every button group and the size list stand
//! on one baseline at one height with even gaps, the active style is a light accent tint over a
//! deeper accent icon, the strokes (outline, dividers, icon bars) fill whole device pixels at 1x,
//! 1.5x and 2x, and an empty editor shows its hint.

use agg_gui::widgets::markdown_editing::{
    group_height, DocPosition, MarkdownFormatButtonKind as K, RichMarkdownEditor, RichSelection,
};
use std::sync::Arc;

use agg_gui::text::Font;
use agg_gui::{App, Color, Framebuffer, GfxCtx, Rect, Size, Widget};

use super::rich_markdown_edit_widget_tests::fonts;

const LOGICAL: Size = Size {
    width: 640.0,
    height: 200.0,
};

fn icons() -> Arc<Font> {
    Arc::new(Font::from_slice(agg_gui::fonts::FONT_AWESOME_4_7).expect("font awesome"))
}

fn at_scale<T>(scale: f64, run: impl FnOnce() -> T) -> T {
    agg_gui::set_device_scale(scale);
    // Painted on white, so the light theme (visuals are per thread, so tests do not leak them).
    agg_gui::theme::set_visuals(agg_gui::theme::Visuals::light());
    let result = run();
    agg_gui::set_device_scale(1.0);
    result
}

fn editor_app(markdown: &str, hint: Option<&str>) -> App {
    let mut editor = RichMarkdownEditor::new(fonts(), icons(), None);
    editor.set_markdown(markdown).unwrap();
    editor.set_empty_hint(hint.map(str::to_string));
    editor
        .select(RichSelection::at(DocPosition::new(0, 0)))
        .unwrap();
    App::new(Box::new(editor))
}

/// The app laid out and painted at `scale` into a white device-pixel framebuffer (Y up).
fn paint(app: &mut App, scale: f64) -> Framebuffer {
    let (w, h) = (LOGICAL.width * scale, LOGICAL.height * scale);
    app.layout(Size::new(w, h));
    let mut fb = Framebuffer::new(w as u32, h as u32);
    {
        let mut ctx = GfxCtx::new(&mut fb);
        ctx.clear(Color::white());
        app.paint(&mut ctx);
    }
    fb
}

fn walk<'a>(widget: &'a dyn Widget, origin: (f64, f64), out: &mut Vec<(&'a dyn Widget, Rect)>) {
    for child in widget.children() {
        let b = child.bounds();
        let at = (origin.0 + b.x, origin.1 + b.y);
        out.push((child.as_ref(), Rect::new(at.0, at.1, b.width, b.height)));
        walk(child.as_ref(), at, out);
    }
}

/// The rects of every widget of `type_name`, left to right, in the app's logical Y-up space.
fn rects(app: &App, type_name: &str) -> Vec<Rect> {
    let mut all = Vec::new();
    walk(app.root(), (0.0, 0.0), &mut all);
    let mut found: Vec<Rect> = all
        .into_iter()
        .filter(|(w, _)| w.type_name() == type_name)
        .map(|(_, r)| r)
        .collect();
    found.sort_by(|a, b| a.x.total_cmp(&b.x));
    found
}

/// The RGB of the device pixel `(x, y)` (Y up).
fn rgb(fb: &Framebuffer, x: i64, y: i64) -> [u8; 3] {
    let i = ((y as usize * fb.width() as usize) + x as usize) * 4;
    let p = fb.pixels();
    [p[i], p[i + 1], p[i + 2]]
}

fn luma(c: [u8; 3]) -> u32 {
    (u32::from(c[0]) * 3 + u32::from(c[1]) * 6 + u32::from(c[2])) / 10
}

fn cell_index(kind: K) -> usize {
    K::ALL.iter().position(|k| *k == kind).expect("a button")
}

const SCALES: [f64; 3] = [1.0, 1.5, 2.0];

/// The groups and the size list stand on one baseline at one height, with the same gap between
/// each neighbouring pair, at every scale.
#[test]
fn rust_only_the_groups_and_the_size_list_share_one_height_baseline_and_gap() {
    for scale in SCALES {
        at_scale(scale, || {
            let mut app = editor_app("Hello\n", None);
            paint(&mut app, scale);
            let mut items = rects(&app, "MarkdownFormatGroup");
            items.extend(rects(&app, "MarkdownSizeList"));
            items.sort_by(|a, b| a.x.total_cmp(&b.x));
            assert_eq!(items.len(), 5, "four groups and the size list");
            for r in &items {
                assert_eq!(r.height, group_height(), "one height at {scale}");
                assert_eq!(r.y, items[0].y, "one baseline at {scale}");
            }
            let gaps: Vec<f64> = items
                .windows(2)
                .map(|p| p[1].x - (p[0].x + p[0].width))
                .collect();
            for gap in &gaps {
                assert_eq!(*gap, gaps[0], "even gaps {gaps:?} at {scale}");
            }
            assert!(gaps[0] >= 4.0, "the groups do not touch: {gaps:?}");
        });
    }
}

/// The active alignment button is a light tint of the accent with a deeper accent icon, never a
/// solid fill; its neighbour is untinted.
#[test]
fn rust_only_the_active_align_button_is_a_light_tint_with_a_deeper_icon() {
    for scale in [1.0, 2.0] {
        at_scale(scale, || {
            let mut app = editor_app("Hello\n", None);
            let fb = paint(&mut app, scale);
            let cells = rects(&app, "MarkdownFormatButton");
            let (left, centre) = (
                cells[cell_index(K::AlignLeft)],
                cells[cell_index(K::AlignCenter)],
            );
            let sample = |cell: Rect| {
                rgb(
                    &fb,
                    (cell.x * scale) as i64 + 2,
                    ((cell.y + cell.height * 0.5) * scale) as i64,
                )
            };
            let tint = sample(left);
            assert!(
                tint.iter().all(|c| *c >= 190) && tint[2] >= tint[0] + 8,
                "a light blue tint, not a solid fill: {tint:?} at {scale}"
            );
            assert_eq!(sample(centre), [255, 255, 255], "the neighbour is untinted");
            // The icon: the darkest pixel inside the cell is the deeper accent, darker than the tint.
            let (x0, y0) = ((left.x * scale) as i64, (left.y * scale) as i64);
            let (x1, y1) = (
                ((left.x + left.width) * scale) as i64,
                ((left.y + left.height) * scale) as i64,
            );
            let mut darkest = [255u8; 3];
            for y in y0 + 2..y1 - 2 {
                for x in x0 + 2..x1 - 2 {
                    if luma(rgb(&fb, x, y)) < luma(darkest) {
                        darkest = rgb(&fb, x, y);
                    }
                }
            }
            assert!(
                luma(darkest) < luma(tint) - 60,
                "icon {darkest:?} vs tint {tint:?}"
            );
            assert!(
                darkest[2] > darkest[0],
                "the icon is an accent, not black: {darkest:?}"
            );
        });
    }
}

/// An empty editor shows its hint in the text area, with the keyboard or without it; without a
/// hint it is blank.
#[test]
fn rust_only_an_empty_editor_shows_its_hint() {
    agg_gui::theme::set_visuals(agg_gui::theme::Visuals::light());
    let hint = "No notes yet. Type here what this design is for.";
    let inked = |app: &mut App| {
        let fb = paint(app, 1.0);
        let area = rects(app, "RichMarkdownEditWidget");
        let area = area[0];
        let mut count = 0;
        for y in (area.y as i64)..((area.y + area.height) as i64) {
            for x in (area.x as i64)..((area.x + area.width) as i64) {
                if luma(rgb(&fb, x, y)) < 235 {
                    count += 1;
                }
            }
        }
        count
    };
    let with_hint = inked(&mut editor_app("", Some(hint)));
    let mut focused_app = editor_app("", Some(hint));
    focused_app.layout(Size::new(LOGICAL.width, LOGICAL.height));
    {
        let editor = focused_app
            .root_mut()
            .as_any_mut()
            .and_then(|w| w.downcast_mut::<RichMarkdownEditor>())
            .expect("the root is the editor");
        editor.editor_mut().on_event(&agg_gui::Event::FocusGained);
        assert!(editor.editor().is_focused());
    }
    let focused = inked(&mut focused_app);
    assert!(
        focused > 100,
        "the hint stays while the editor has the keyboard ({focused} inked pixels)"
    );
    let without = inked(&mut editor_app("", None));
    assert!(
        with_hint > 100,
        "the hint is drawn ({with_hint} inked pixels)"
    );
    assert!(
        without < with_hint / 10,
        "no hint, nothing drawn ({without})"
    );
}

/// Set `AGG_GUI_TOOLBAR_SNAPSHOT` to a folder to save the editor at each scale there as
/// `toolbar_<scale>x.png`, for looking at.
#[test]
fn rust_only_toolbar_snapshots() {
    let Ok(dir) = std::env::var("AGG_GUI_TOOLBAR_SNAPSHOT") else {
        return;
    };
    for scale in SCALES {
        at_scale(scale, || {
            let mut app = editor_app("", Some("No notes yet. Type here what this design is for."));
            let fb = paint(&mut app, scale);
            let rgba = fb.pixels_flipped();
            let png =
                agg_gui::screenshot::encode_png_rgba(&rgba, fb.width(), fb.height()).expect("png");
            std::fs::write(format!("{dir}/toolbar_{scale}x.png"), png).expect("writes");
        });
    }
}

/// The outline, the dividers and the icon bars are whole device pixels: a column and a row cut
/// through a group hold only the background and the stroke colour, and the align icon's centre
/// column holds only solid ink and background, with no half-covered pixel between. All of it is
/// checked at 1x, 1.5x and 2x: the icons are drawn, so their bars are whole pixels at any scale.
#[test]
fn rust_only_strokes_fill_whole_device_pixels() {
    for scale in SCALES {
        at_scale(scale, || {
            let mut app = editor_app("Hello\n", None);
            let fb = paint(&mut app, scale);
            let groups = rects(&app, "MarkdownFormatGroup");
            let links = groups[3];
            let stroke = rgb(&fb, (links.x * scale) as i64, (links.y * scale) as i64 + 20);
            // Within 4/255 of a colour counts as that colour: an edge on a pixel boundary can bleed
            // a rounding error's worth of coverage into its neighbour.
            let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(p, q)| p.abs_diff(q) <= 4);
            let in_two = |colors: Vec<[u8; 3]>, what: &str| {
                assert!(
                    colors
                        .iter()
                        .all(|c| near(*c, [255, 255, 255]) || near(*c, stroke)),
                    "{what} at {scale}x holds a half-covered pixel: {colors:?}"
                );
                assert!(
                    colors.iter().any(|c| near(*c, stroke)),
                    "{what} meets the stroke"
                );
            };
            // A column through the group's left cell, clear of the corner curve and the icon.
            let x = ((links.x + 7.25) * scale) as i64;
            let (y0, y1) = (
                (links.y * scale) as i64 - 2,
                ((links.y + links.height) * scale) as i64 + 2,
            );
            in_two(
                (y0..y1).map(|y| rgb(&fb, x, y)).collect(),
                "a column across the outline",
            );
            // A row through the group below the icons, across its dividers.
            let y = ((links.y + 3.0) * scale) as i64;
            let (x0, x1) = (
                ((links.x + 9.0) * scale) as i64,
                ((links.x + links.width - 9.0) * scale) as i64,
            );
            in_two(
                (x0..x1).map(|x| rgb(&fb, x, y)).collect(),
                "a row across the dividers",
            );
        });
    }
    for scale in SCALES {
        at_scale(scale, || {
            let mut app = editor_app("Hello\n", None);
            let fb = paint(&mut app, scale);
            let cells = rects(&app, "MarkdownFormatButton");
            let c = cells[cell_index(K::AlignCenter)];
            let x = ((c.x + c.width * 0.5) * scale) as i64;
            for y in ((c.y * scale) as i64 + 3)..(((c.y + c.height) * scale) as i64 - 3) {
                let l = luma(rgb(&fb, x, y));
                assert!(
                    l <= 80 || l >= 250,
                    "a half-covered pixel ({l}) in the icon at {scale}x, row {y}"
                );
            }
        });
    }
}

/// Which straight edge of an icon is probed for crispness.
#[derive(Clone, Copy)]
enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The ink coverage of the square of the cell where its icon is drawn (the icon's box and a
/// pixel more around it), row by row from the bottom: 0 is the cell's background, 1 its darkest
/// pixel. The group's outline and dividers, and the outline's rounded corner, lie outside it.
fn cell_coverage(fb: &Framebuffer, cell: Rect, scale: f64) -> Vec<Vec<f64>> {
    let half =
        (agg_gui::widgets::markdown_editing::ICON_DESIGN_SIZE * scale / 2.0).ceil() as i64 + 2;
    let (cx, cy) = (
        ((cell.x + cell.width * 0.5) * scale).round() as i64,
        ((cell.y + cell.height * 0.5) * scale).round() as i64,
    );
    let (x0, x1, y0, y1) = (cx - half, cx + half, cy - half, cy + half);
    let background = luma(rgb(fb, x0, y0)) as f64;
    let darkest = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| luma(rgb(fb, x, y)) as f64)
        .fold(background, f64::min);
    assert!(
        background - darkest > 60.0,
        "the icon is drawn at {scale}x (background {background}, darkest {darkest})"
    );
    (y0..y1)
        .map(|y| {
            (x0..x1)
                .map(|x| (background - luma(rgb(fb, x, y)) as f64) / (background - darkest))
                .collect()
        })
        .collect()
}

/// Which way `ink_runs` projects the coverage.
#[derive(Clone, Copy)]
enum Axis {
    Rows,
    Columns,
}

/// The `[start, end)` ranges of consecutive rows (or columns) holding any ink, in order.
fn ink_runs(rows: &[Vec<f64>], axis: Axis) -> Vec<(usize, usize)> {
    let lines: Vec<bool> = match axis {
        Axis::Rows => rows.iter().map(|r| r.iter().any(|c| *c > 0.08)).collect(),
        Axis::Columns => (0..rows[0].len())
            .map(|x| rows.iter().any(|r| r[x] > 0.08))
            .collect(),
    };
    let mut runs = Vec::new();
    let mut start = None;
    for (i, inked) in lines.iter().copied().chain([false]).enumerate() {
        match (inked, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                runs.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    runs
}

/// True when the outermost row or column of ink on `edge` holds solid pixels: a
/// straight edge on a pixel boundary has its extreme line fully inked, while one that straddles
/// two lines leaves a partly covered extreme line.
fn edge_is_crisp(rows: &[Vec<f64>], edge: Edge) -> bool {
    let inked = |c: f64| c > 0.08;
    let solid = |c: f64| c >= 0.95;
    let line: Vec<f64> = match edge {
        Edge::Top => match rows.iter().rposition(|r| r.iter().any(|c| inked(*c))) {
            Some(y) => rows[y].clone(),
            None => return false,
        },
        Edge::Bottom => match rows.iter().position(|r| r.iter().any(|c| inked(*c))) {
            Some(y) => rows[y].clone(),
            None => return false,
        },
        Edge::Left | Edge::Right => {
            let width = rows[0].len();
            let column = |x: usize| rows.iter().map(|r| r[x]).collect::<Vec<f64>>();
            let found = match edge {
                Edge::Left => (0..width).find(|x| column(*x).iter().any(|c| inked(*c))),
                _ => (0..width)
                    .rev()
                    .find(|x| column(*x).iter().any(|c| inked(*c))),
            };
            match found {
                Some(x) => column(x),
                None => return false,
            }
        }
    };
    line.iter().filter(|c| solid(**c)).count() >= 2
}

/// Every icon is drawn at every scale, and the ones with straight edges have those edges on
/// whole device pixels: the extreme row or column of ink is fully inked, with no half-covered
/// line beside it. The icons made only of bars (align, numbered list) hold no half-covered
/// pixel anywhere. The quote is two filled marks with their sides and tips on whole pixels, and the
/// numbered list is two numerals above two bars, each readable as separate ink.
#[test]
fn rust_only_every_icon_is_drawn_and_crisp_at_every_scale() {
    use Edge::{Bottom, Left, Right, Top};
    let probes: [(K, &[Edge]); 13] = [
        (K::Bold, &[Left]),
        (K::Italic, &[Top]),
        (K::Strike, &[Left, Right]),
        (K::Code, &[]),
        (K::AlignLeft, &[Top, Left]),
        (K::AlignCenter, &[Top, Left, Right]),
        (K::AlignRight, &[Top, Right]),
        (K::BulletList, &[Right]),
        (K::NumberedList, &[Right]),
        (K::Quote, &[Top, Bottom, Left, Right]),
        (K::Link, &[]),
        (K::CodeBlock, &[Top, Left]),
        (K::Table, &[Top, Left]),
    ];
    let bars_only = [K::AlignLeft, K::AlignCenter, K::AlignRight, K::NumberedList];
    for scale in SCALES {
        at_scale(scale, || {
            let mut app = editor_app("Hello\n", None);
            let fb = paint(&mut app, scale);
            let cells = rects(&app, "MarkdownFormatButton");
            for (kind, edges) in probes {
                let rows = cell_coverage(&fb, cells[cell_index(kind)], scale);
                for edge in edges {
                    assert!(
                        edge_is_crisp(&rows, *edge),
                        "{kind:?}: a soft {} edge at {scale}x",
                        match edge {
                            Edge::Top => "top",
                            Edge::Bottom => "bottom",
                            Edge::Left => "left",
                            Edge::Right => "right",
                        }
                    );
                }
                match kind {
                    K::Quote => {
                        // Two marks: two runs of inked columns with a clear gap between.
                        let runs = ink_runs(&rows, Axis::Columns);
                        assert_eq!(runs.len(), 2, "the quote is two marks at {scale}x");
                        assert_eq!(runs[0].1 - runs[0].0, runs[1].1 - runs[1].0);
                        // Each mark is a solid shape, with an inked body and a tail above it.
                        let body = rows
                            .iter()
                            .map(|r| r.iter().filter(|c| **c >= 0.95).count())
                            .max()
                            .unwrap();
                        assert!(
                            body >= 2 * (runs[0].1 - runs[0].0 - 1),
                            "the marks' bulbs are solid at {scale}x"
                        );
                    }
                    K::NumberedList => {
                        // Rows are listed from the bottom: the "2" first, then the "1".
                        let right = rows[0].len() * 3 / 5;
                        let part = |from: usize, to: usize| -> Vec<Vec<f64>> {
                            rows.iter().map(|r| r[from..to].to_vec()).collect()
                        };
                        let bars = ink_runs(&part(right, rows[0].len()), Axis::Rows);
                        assert_eq!(bars.len(), 2, "two bars at {scale}x");
                        let bar = bars[0].1 - bars[0].0;
                        assert_eq!(bar, bars[1].1 - bars[1].0, "equal bars at {scale}x");
                        // The bars are level with the bulleted list's first and third bars.
                        let bullets = cell_coverage(&fb, cells[cell_index(K::BulletList)], scale);
                        let rungs = ink_runs(
                            &bullets
                                .iter()
                                .map(|r| r[right..].to_vec())
                                .collect::<Vec<_>>(),
                            Axis::Rows,
                        );
                        assert_eq!(rungs.len(), 3, "three bullet bars at {scale}x");
                        assert_eq!(bars[0], rungs[0], "the lower bar is the third row's");
                        assert_eq!(bars[1], rungs[2], "the upper bar is the first row's");
                        // Two numerals, each at least 7 pixels tall and 4 wide, with a gap of
                        // at least one stroke between them.
                        let numerals = part(0, rows[0].len() * 2 / 5);
                        let rows_ink = ink_runs(&numerals, Axis::Rows);
                        assert_eq!(rows_ink.len(), 2, "two numerals at {scale}x: {rows_ink:?}");
                        for (lo, hi) in &rows_ink {
                            assert!(hi - lo >= 7, "numeral height {} at {scale}x", hi - lo);
                        }
                        let columns = ink_runs(&numerals, Axis::Columns);
                        let (first, last) = (columns[0].0, columns.last().unwrap().1);
                        assert!(last - first >= 4, "numeral width at {scale}x");
                        // The stroke is the 2's base: the rows from the bottom as wide as the
                        // lowest one. It matches the bars' thickness wherever the box allows
                        // (1.5x and 2x) and is one pixel at 1x.
                        let ink = |y: usize| numerals[y].iter().filter(|c| **c > 0.5).count();
                        let base = rows_ink[0].0;
                        let stroke = (base..rows_ink[0].1)
                            .take_while(|y| ink(*y) == ink(base))
                            .count();
                        assert_eq!(
                            stroke,
                            if scale >= 1.5 { bar } else { 1 },
                            "the numeral stroke at {scale}x"
                        );
                        let gap = rows_ink[1].0 - rows_ink[0].1;
                        assert!(gap >= stroke, "a gap of {gap} between numerals at {scale}x");
                    }
                    _ => {}
                }
                if bars_only.contains(&kind) {
                    let soft = rows
                        .iter()
                        .flatten()
                        .filter(|c| **c > 0.06 && **c < 0.94)
                        .count();
                    assert_eq!(
                        soft, 0,
                        "{kind:?} has {soft} half-covered pixels at {scale}x"
                    );
                }
            }
        });
    }
}
