//! `TreeView` timing benchmark: a large, mostly collapsed tree.
//!
//! 100 000 nodes, of which about 5 000 rows are visible (expanded), shown in
//! a 400 x 600 viewport — the shape of a lazily populated file tree such as
//! HDTreeMap's folder tree.  Times the operations a user repeats: an idle
//! re-layout, a selection change (arrow key and click), expanding and
//! collapsing a folder, and scrolling, each followed by a software paint.
//!
//! Timing is machine-dependent, so the test is `#[ignore]`d; run it with
//! `cargo test -p agg-gui --release tree_view_perf -- --ignored --nocapture`.
//! The deterministic guarantees (O(visible) row widgets, no rebuild on a
//! selection change) are covered by `tree_view_api.rs`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::event::{Event, Key, Modifiers, MouseButton};
use crate::framebuffer::Framebuffer;
use crate::geometry::{Point, Rect, Size};
use crate::gfx_ctx::GfxCtx;
use crate::text::Font;
use crate::widget::{paint_subtree, Widget};
use crate::widgets::tree_view::{NodeIcon, TreeNode, TreeView};

const TOTAL_NODES: usize = 100_000;
const TOP_FOLDERS: usize = 1_000;
const OPEN_FOLDERS: usize = 4;
const OPEN_FOLDER_CHILDREN: usize = 1_000;
const W: f64 = 400.0;
const H: f64 = 600.0;

/// Root (expanded) → 1 000 folders; the first four are expanded with 1 000
/// files each; the rest of the 100 000 nodes are spread over the collapsed
/// folders.  Visible rows: 1 + 1 000 + 4 000 = 5 001.
pub(super) fn build_big_tree(font: Arc<Font>) -> TreeView {
    let mut tree = TreeView::new(font).with_row_height(20.0);
    let mut root = TreeNode::new("Root", NodeIcon::Folder, None, 0);
    root.is_expanded = true;
    tree.nodes.push(root);
    for f in 0..TOP_FOLDERS {
        let mut folder = TreeNode::new(format!("Folder {f}"), NodeIcon::Folder, Some(0), f as u32);
        folder.is_expanded = f < OPEN_FOLDERS;
        tree.nodes.push(folder);
    }
    for f in 0..OPEN_FOLDERS {
        for c in 0..OPEN_FOLDER_CHILDREN {
            let label = format!("file {f}-{c}.txt");
            tree.nodes
                .push(TreeNode::new(label, NodeIcon::File, Some(1 + f), c as u32));
        }
    }
    let mut i = 0usize;
    while tree.nodes.len() < TOTAL_NODES {
        let folder = 1 + OPEN_FOLDERS + i % (TOP_FOLDERS - OPEN_FOLDERS);
        let order = (i / (TOP_FOLDERS - OPEN_FOLDERS)) as u32;
        tree.nodes.push(TreeNode::new(
            format!("hidden {i}"),
            NodeIcon::File,
            Some(folder),
            order,
        ));
        i += 1;
    }
    tree
}

fn frame(tree: &mut TreeView, fb: &mut Framebuffer) {
    tree.layout(Size::new(W, H));
    tree.set_bounds(Rect::new(0.0, 0.0, W, H));
    let mut ctx = GfxCtx::new(fb);
    paint_subtree(tree, &mut ctx);
}

fn key(k: Key) -> Event {
    Event::KeyDown {
        key: k,
        modifiers: Modifiers::default(),
    }
}

/// Median of `iters` runs of `op` followed by a full frame.
fn time(
    tree: &mut TreeView,
    fb: &mut Framebuffer,
    iters: usize,
    mut op: impl FnMut(&mut TreeView, usize),
) -> Duration {
    let mut samples: Vec<Duration> = (0..iters)
        .map(|i| {
            let t = Instant::now();
            op(tree, i);
            frame(tree, fb);
            t.elapsed()
        })
        .collect();
    samples.sort();
    samples[samples.len() / 2]
}

/// Time the repeated operations on `tree` and print them under `title`.
fn run_suite(title: &str, mut tree: TreeView, build: Duration) {
    let mut fb = Framebuffer::new(W as u32, H as u32);
    let t = Instant::now();
    frame(&mut tree, &mut fb);
    let first = t.elapsed();
    let iters = std::env::var("TREE_PERF_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(7);

    tree.on_event(&Event::FocusGained);
    let idle = time(&mut tree, &mut fb, iters, |_, _| {});
    let mut layouts: Vec<Duration> = (0..iters)
        .map(|_| {
            let t = Instant::now();
            tree.layout(Size::new(W, H));
            t.elapsed()
        })
        .collect();
    layouts.sort();
    let idle_layout = layouts[layouts.len() / 2];
    let arrow = time(&mut tree, &mut fb, iters, |t, _| {
        t.on_event(&key(Key::ArrowDown));
    });
    let click = time(&mut tree, &mut fb, iters, |t, i| {
        let pos = Point::new(100.0, H - 10.0 - 20.0 * (i % 5) as f64);
        let mods = Modifiers::default();
        t.on_event(&Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: mods,
        });
        t.on_event(&Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers: mods,
        });
    });
    let toggle = time(&mut tree, &mut fb, iters, |t, i| {
        // Folder 10 (node 11): expand on even runs, collapse on odd.
        t.nodes[11].is_expanded = i % 2 == 0;
    });
    let scroll = time(&mut tree, &mut fb, iters, |t, _| {
        t.on_event(&Event::MouseWheel {
            pos: Point::new(100.0, 300.0),
            delta_y: -3.0,
            delta_x: 0.0,
            modifiers: Modifiers::default(),
        });
    });

    println!(
        "TreeView perf ({title}, {W}x{H}):\n  \
         build {build:?}\n  first frame {first:?}\n  idle frame {idle:?}\n  \
         idle layout only {idle_layout:?}\n  \
         selection (arrow) {arrow:?}\n  selection (click) {click:?}\n  \
         expand/collapse {toggle:?}\n  scroll {scroll:?}\n  \
         row widgets {}",
        tree.children().len()
    );
}

#[test]
#[ignore = "timing benchmark; run with --release --ignored --nocapture"]
fn tree_view_perf_100k_nodes_5k_open() {
    let font = Arc::new(Font::from_slice(super::TEST_FONT).expect("font"));
    let t = Instant::now();
    let tree = build_big_tree(font);
    run_suite("100k nodes, ~5k open", tree, t.elapsed());
}

/// HDTreeMap's reported case: about 700 nodes held, all open — a selection
/// change cost ~12 ms on the software renderer before row widgets stopped
/// being rebuilt on selection.
#[test]
#[ignore = "timing benchmark; run with --release --ignored --nocapture"]
fn tree_view_perf_700_nodes_open() {
    let font = Arc::new(Font::from_slice(super::TEST_FONT).expect("font"));
    let t = Instant::now();
    let mut tree = TreeView::new(font).with_row_height(20.0);
    let mut root = TreeNode::new("Root", NodeIcon::Folder, None, 0);
    root.is_expanded = true;
    tree.nodes.push(root);
    for f in 0..20 {
        let mut folder = TreeNode::new(format!("Folder {f}"), NodeIcon::Folder, Some(0), f);
        folder.is_expanded = true;
        tree.nodes.push(folder);
    }
    for i in 0..680u32 {
        let parent = 1 + (i % 20) as usize;
        let label = format!("file {i}.dat  -  {} MB", i * 7 % 1000);
        tree.nodes
            .push(TreeNode::new(label, NodeIcon::File, Some(parent), i / 20));
    }
    run_suite("700 nodes, all open", tree, t.elapsed());
}
