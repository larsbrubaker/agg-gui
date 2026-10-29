//! Markdown placement gate (CLAUDE.md, "Documentation"). Documentation lives
//! in the code, in the header and doc comments of the file, module or crate it
//! describes, where whoever works on that code reads it. A Markdown file is
//! allowed only where it serves readers who are not reading the code:
//!
//! - `README.md` and `CLAUDE.md` at the repository root;
//! - `README.md`, `CHANGELOG.md` and `THIRD_PARTY_NOTICES.md` in a crate's
//!   own directory (beside a `Cargo.toml` with a `[package]`), or at the root;
//! - anything under `.claude/` (agent instructions and skills);
//! - a plan for open work listed in [`OPEN_WORK_PLANS`], while the work is
//!   open: when it is done, its lasting reasons move into the code, and the
//!   plan and its entry are deleted;
//! - a Markdown file a `.rs` file pulls in with `include_str!("...")`, such as
//!   the `DrawCtx` method docs in `src/draw_ctx/docs/`, which rustdoc shows as
//!   the methods' own docs. It stays allowed only while some `.rs` names it.
//!
//! Every other `.md` or `.markdown` file (any case) fails the gate, named with
//! what to do about it. The walk is of the working tree, so a new doc fails
//! before it is committed. It skips build output (`target`, `node_modules`,
//! any directory holding a `CACHEDIR.TAG`, the web demo's `dist` and `pkg`,
//! and Playwright's `test-results` and `playwright-report` beside them),
//! `.git`, and the third-party trees (`tests/resvg-test-suite` and the egui
//! and C++ reference mirrors), whose Markdown is not ours.
//!
//! The repository root is the parent of this crate's directory. A copy of the
//! crate with no `.git` there (a crates.io download) is not a checkout of the
//! repository, and the gate passes without checking.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Directories the walk never enters, relative to the root (`/`-separated).
/// `.claude` is allowed whole, and holds full agent checkouts besides.
const SKIPPED_TREES: &[&str] = &[
    ".claude",
    ".cursor",
    "egui-reference",
    "reference-egui-main",
    "agg-gui/reference-egui-main",
    "cpp-reference",
    "tests/resvg-test-suite",
    "demo/dist",
    "demo/node_modules",
    "demo/public/pkg",
    "demo/public/dist",
    // Playwright's output for the demo's browser tests: results, traces and
    // the error-context.md it writes for a failed run, and its HTML report.
    "demo/test-results",
    "demo/playwright-report",
];

/// Plans for work still open, relative to the root. Each must exist, so an
/// entry cannot outlive its plan.
const OPEN_WORK_PLANS: &[&str] = &[
    // The widgets still to make reflectable, and the inspector's open editor
    // questions.
    "bevy_reflect_to_do.md",
];

/// Directory names skipped wherever they appear: build output and git.
const SKIPPED_NAMES: &[&str] = &[".git", "target", "node_modules"];

/// What the gate makes of one repository path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    /// Not a Markdown file: the gate has nothing to say about it.
    NotMarkdown,
    /// A Markdown file in one of the allowed places.
    Allowed,
    /// A Markdown file anywhere else.
    Refused,
}

/// What the walk learns beyond the file list: which directories are crates,
/// and which Markdown files some `.rs` file includes.
#[derive(Debug, Default)]
struct Context {
    /// Crate directories, relative to the root (`""` for none at the root).
    crate_dirs: BTreeSet<String>,
    /// Markdown files named by an `include_str!` in a `.rs` file, relative to
    /// the root.
    included: BTreeSet<String>,
}

/// True for a Markdown file by its extension, whatever its case.
fn is_markdown(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => {
            ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown")
        }
        _ => false,
    }
}

/// Where `path` (relative to the root, `/`-separated) stands.
fn placement(path: &str, context: &Context) -> Placement {
    if !is_markdown(path) {
        return Placement::NotMarkdown;
    }
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let at_root = dir.is_empty();
    let crate_file = matches!(
        name,
        "README.md" | "CHANGELOG.md" | "THIRD_PARTY_NOTICES.md"
    ) && (at_root || context.crate_dirs.contains(dir));
    let allowed = (at_root && name == "CLAUDE.md")
        || crate_file
        || path
            .strip_prefix(".claude/")
            .is_some_and(|rest| !rest.is_empty())
        || OPEN_WORK_PLANS.contains(&path)
        || context.included.contains(path);
    if allowed {
        Placement::Allowed
    } else {
        Placement::Refused
    }
}

/// The string literals `include_str!` names in `source`.
fn included_literals(source: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = source;
    while let Some(at) = rest.find("include_str!") {
        rest = &rest[at + "include_str!".len()..];
        let args = rest.trim_start();
        let Some(args) = args.strip_prefix('(') else {
            continue;
        };
        let Some(literal) = args.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some(end) = literal.find('"') {
            found.push(&literal[..end]);
        }
    }
    found
}

/// `relative` resolved against the directory `dir` (both relative to the
/// root), with `.` and `..` folded; `None` when it climbs out of the root.
fn resolve(dir: &str, relative: &str) -> Option<String> {
    let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
    for part in relative.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// True when the walk must not enter the directory at `rel`.
fn skipped(rel: &str, abs: &Path) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    SKIPPED_NAMES.contains(&name)
        || SKIPPED_TREES.contains(&rel)
        || abs.join("CACHEDIR.TAG").is_file()
}

/// Every file under `root` the gate looks at, relative and `/`-separated, in
/// sorted order, and what the walk learned about them.
fn walk(root: &Path) -> (Vec<String>, Context) {
    let mut files = Vec::new();
    let mut pending: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, rel_dir)) = pending.pop() {
        let entries = fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read the directory {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|e| panic!("read an entry of {}: {e}", dir.display()));
            let name = entry.file_name().to_string_lossy().into_owned();
            let rel = if rel_dir.is_empty() {
                name
            } else {
                format!("{rel_dir}/{name}")
            };
            let file_type = entry
                .file_type()
                .unwrap_or_else(|e| panic!("read the file type of {rel}: {e}"));
            // Symlinks are neither followed nor checked: they point at trees
            // the walk reaches by their own path, or at other checkouts.
            if file_type.is_dir() {
                if !skipped(&rel, &entry.path()) {
                    pending.push((entry.path(), rel));
                }
            } else if file_type.is_file() {
                files.push(rel);
            }
        }
    }
    files.sort();

    let mut context = Context::default();
    for path in &files {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
        if name == "Cargo.toml" {
            let manifest =
                fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("read {path}: {e}"));
            if manifest.lines().any(|line| line.trim() == "[package]") {
                context.crate_dirs.insert(dir.to_string());
            }
        } else if name.ends_with(".rs") {
            let Ok(source) = fs::read_to_string(root.join(path)) else {
                continue;
            };
            for literal in included_literals(&source) {
                if is_markdown(literal) {
                    if let Some(target) = resolve(dir, literal) {
                        context.included.insert(target);
                    }
                }
            }
        }
    }
    (files, context)
}

/// The Markdown files under `root` the gate refuses.
fn misplaced(root: &Path) -> Vec<String> {
    let (files, context) = walk(root);
    files
        .into_iter()
        .filter(|path| placement(path, &context) == Placement::Refused)
        .collect()
}

/// The gate's failure message for the refused paths, empty when there are none.
fn failure_message(refused: &[String]) -> String {
    if refused.is_empty() {
        return String::new();
    }
    let mut message = format!(
        "{} Markdown file(s) outside the places Markdown may live:\n",
        refused.len()
    );
    for path in refused {
        message.push_str(&format!("  {path}\n"));
    }
    message.push_str(
        "Documentation lives in the code. Move what it explains into the header or doc \
         comments of the code it is about (CLAUDE.md, \"Documentation\"), then delete it. \
         Release notes go in the crate's CHANGELOG.md. Markdown may live only in the root \
         README.md and CLAUDE.md, a crate's README.md, CHANGELOG.md and \
         THIRD_PARTY_NOTICES.md, .claude/, a file a .rs pulls in with include_str!, and a \
         plan for open work listed in OPEN_WORK_PLANS.\n",
    );
    message
}

/// The repository root, or `None` for a copy of the crate outside a checkout.
fn repo_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    // `.git` is a directory in a clone and a file in a worktree or submodule.
    root.join(".git").exists().then_some(root)
}

/// A scratch directory tree for the walk's tests, removed when dropped.
struct TempTree(PathBuf);

impl TempTree {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "agg-gui-markdown-placement-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create the scratch tree");
        TempTree(dir)
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.0.join(rel);
        fs::create_dir_all(path.parent().expect("a file has a parent")).expect("create dirs");
        fs::write(path, text).expect("write a scratch file");
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn markdown_lives_only_in_the_allowed_places() {
    let Some(root) = repo_root() else {
        eprintln!("markdown_placement: not a checkout of the repository (no .git); skipped");
        return;
    };
    let message = failure_message(&misplaced(&root));
    assert!(message.is_empty(), "{message}");
}

#[test]
fn markdown_gate_sees_the_allowed_files() {
    // Guards against a walk that silently checks nothing (wrong root, over-broad skips).
    let Some(root) = repo_root() else {
        return;
    };
    let (files, context) = walk(&root);
    for expected in ["CLAUDE.md", "README.md", "agg-gui/CHANGELOG.md"] {
        assert!(
            files.iter().any(|p| p == expected),
            "the walk missed {expected}"
        );
    }
    assert!(context.crate_dirs.contains("agg-gui"));
    assert!(context
        .included
        .contains("agg-gui/src/draw_ctx/docs/clip_path.md"));
}

#[test]
fn every_listed_open_work_plan_exists() {
    let Some(root) = repo_root() else {
        return;
    };
    for plan in OPEN_WORK_PLANS {
        assert!(
            root.join(plan).is_file(),
            "{plan} is listed in OPEN_WORK_PLANS but does not exist: delete its entry"
        );
    }
}

#[test]
fn markdown_placement_allows_the_listed_places() {
    let context = Context {
        crate_dirs: ["agg-gui", "agg-mic"].map(String::from).into(),
        included: ["agg-gui/src/draw_ctx/docs/arc_to.md".to_string()].into(),
    };
    for path in [
        "README.md",
        "CLAUDE.md",
        "THIRD_PARTY_NOTICES.md",
        "agg-gui/README.md",
        "agg-gui/CHANGELOG.md",
        "agg-mic/THIRD_PARTY_NOTICES.md",
        ".claude/agents/implementer.md",
        ".claude/skills/acp/SKILL.md",
        "agg-gui/src/draw_ctx/docs/arc_to.md",
        "bevy_reflect_to_do.md",
    ] {
        assert_eq!(placement(path, &context), Placement::Allowed, "{path}");
    }
}

#[test]
fn markdown_placement_refuses_docs_anywhere_else() {
    let context = Context {
        crate_dirs: ["agg-gui".to_string()].into(),
        included: ["agg-gui/src/draw_ctx/docs/arc_to.md".to_string()].into(),
    };
    for path in [
        "docs/design/gui-automation.md",
        "docs/porting_update.md",
        "agg-gui/CHANGELOG-ARCHIVE.md",
        // A README or changelog only counts in a crate's own directory.
        "agg-gui/src/README.md",
        "demo/README.md",
        "CHANGELOG-NOTES.md",
        // An included doc's neighbour is not included.
        "agg-gui/src/draw_ctx/docs/ellipse.md",
        "CONTRIBUTING.md",
        "agg-gui/CLAUDE.md",
    ] {
        assert_eq!(placement(path, &context), Placement::Refused, "{path}");
    }
    // Not Markdown, so not this gate's concern.
    for path in ["agg-gui/src/lib.rs", "docs/hero_image.png", "README", ".md"] {
        assert_eq!(placement(path, &context), Placement::NotMarkdown, "{path}");
    }
}

#[test]
fn markdown_placement_matches_extensions_in_any_case() {
    let context = Context::default();
    for path in [
        "notes.MD",
        "notes.Md",
        "notes.markdown",
        "demo/guide.MARKDOWN",
        "readme.md",
    ] {
        assert_eq!(placement(path, &context), Placement::Refused, "{path}");
    }
}

#[test]
fn include_str_names_resolve_against_the_including_file() {
    let source = r#"
        #[doc = include_str!("draw_ctx/docs/arc_to.md")]
        let readme = include_str!( "../../README.md" );
        let text = include_str!(concat!(env!("OUT_DIR"), "/x.md"));
    "#;
    assert_eq!(
        included_literals(source),
        ["draw_ctx/docs/arc_to.md", "../../README.md"]
    );
    assert_eq!(
        resolve("agg-gui/src", "draw_ctx/docs/arc_to.md").as_deref(),
        Some("agg-gui/src/draw_ctx/docs/arc_to.md")
    );
    assert_eq!(
        resolve("demo-ui/src", "../../README.md").as_deref(),
        Some("README.md")
    );
    assert_eq!(resolve("a", "../../x.md"), None);
}

#[test]
fn markdown_gate_reads_untracked_files_and_skips_build_and_third_party_trees() {
    let tree = TempTree::new("walk");
    tree.write("Cargo.toml", "[workspace]\n");
    tree.write("README.md", "readme\n");
    tree.write("lib-a/Cargo.toml", "[package]\nname = \"lib-a\"\n");
    tree.write("lib-a/README.md", "crate readme\n");
    tree.write(
        "lib-a/src/lib.rs",
        "#[doc = include_str!(\"docs/kept.md\")]\nfn f() {}\n",
    );
    tree.write("lib-a/src/docs/kept.md", "a method's docs\n");
    tree.write("lib-a/src/docs/orphan.md", "no .rs names me\n");
    tree.write("docs/design/stray.md", "a design doc\n");
    tree.write("not-a-crate/README.md", "a folder's notes\n");
    // Build output, git and third-party trees are never walked.
    for skipped in [
        "target/doc/README.md",
        "demo/node_modules/pkg/README.md",
        "demo/dist/README.md",
        "demo/test-results/x/error-context.md",
        "demo/playwright-report/data/notes.md",
        "lib-a/target/notes.md",
        ".git/info/notes.md",
        ".claude/worktrees/slot1/notes.md",
        "tests/resvg-test-suite/README.md",
        "egui-reference/README.md",
        "build-out/CACHEDIR.TAG",
        "build-out/README.md",
    ] {
        tree.write(skipped, "skipped\n");
    }
    assert_eq!(
        misplaced(&tree.0),
        [
            "docs/design/stray.md",
            "lib-a/src/docs/orphan.md",
            "not-a-crate/README.md"
        ]
    );
}

#[test]
fn markdown_gate_failure_message_says_what_to_do() {
    let message = failure_message(&["docs/notes.md".to_string()]);
    assert!(message.starts_with("1 Markdown file(s) outside the places Markdown may live:\n"));
    assert!(message.contains("  docs/notes.md\n"));
    assert!(message.contains("into the header or doc comments of the code it is about"));
    assert!(failure_message(&[]).is_empty());
}
