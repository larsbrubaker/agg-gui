//! Guardrail for keeping AI-touchable source files small enough to reason about.
//!
//! This deliberately checks more than Rust files. Any text source or checked-in
//! build artifact that an AI assistant may need to inspect or edit should stay
//! under the same limit, including JavaScript/HTML glue such as wasm-bindgen
//! package output. If a file fails this test, split or refactor it instead of
//! hiding that source type from the scan.

use std::fs;
use std::path::{Path, PathBuf};

const MAX_LINES: usize = 800;

// Keep exclusions limited to third-party mirrors, caches, generated output dirs,
// and external test suites. Do not exclude first-party source trees just because
// a file is generated-looking; if it is checked into a source/package directory
// and may be touched by an agent, it should be counted.
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    // Claude Code harness state; .claude/worktrees/* holds full agent checkouts
    // (each with its own target/ of generated code), so scanning it both
    // double-counts the project and trips on third-party build output.
    ".claude",
    ".cursor",
    "target",
    "egui-reference",
    "reference-egui-main",
    "agg-gui/reference-egui-main",
    "cpp-reference",
    "demo/dist",
    "demo/node_modules",
    "demo/public/pkg",
    "tests/resvg-test-suite",
];

// Text formats that are part of the project surface area for humans and AI
// agents. This intentionally includes JS/HTML/CSS/TS, so wasm package glue and
// web demo sources are held to the same size limit as Rust modules.
const CHECKED_EXTENSIONS: &[&str] = &[
    "css", "html", "js", "json", "md", "rs", "toml", "ts", "tsx", "yaml", "yml",
];

#[test]
fn first_party_project_files_stay_under_line_limit() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("agg-gui crate should live under the workspace root");

    let mut offenders = Vec::new();
    visit_files(workspace_root, workspace_root, &mut offenders);

    if !offenders.is_empty() {
        offenders.sort();
        panic!(
            "project files must stay at or below {MAX_LINES} lines; offenders:\n{}",
            offenders
                .into_iter()
                .map(|(lines, path)| format!("{lines:>5}  {}", path.display()))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

fn visit_files(root: &Path, dir: &Path, offenders: &mut Vec<(usize, PathBuf)>) {
    if is_excluded(root, dir) {
        return;
    }

    let entries = fs::read_dir(dir).unwrap_or_else(|err| {
        panic!("failed to read directory {}: {err}", dir.display());
    });

    for entry in entries {
        let entry = entry.unwrap_or_else(|err| {
            panic!("failed to read directory entry in {}: {err}", dir.display());
        });
        let path = entry.path();
        let file_type = entry.file_type().unwrap_or_else(|err| {
            panic!("failed to read file type for {}: {err}", path.display());
        });

        if file_type.is_dir() {
            visit_files(root, &path, offenders);
        } else if file_type.is_file() && should_check_file(&path) {
            let lines = count_lines(&path);
            if lines > MAX_LINES {
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                offenders.push((lines, rel));
            }
        }
    }
}

fn is_excluded(root: &Path, path: &Path) -> bool {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let rel = rel.to_string_lossy().replace('\\', "/");
    EXCLUDED_DIRS
        .iter()
        .any(|excluded| rel == *excluded || rel.starts_with(&format!("{excluded}/")))
}

fn should_check_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            CHECKED_EXTENSIONS
                .iter()
                .any(|checked| ext.eq_ignore_ascii_case(checked))
        })
        .unwrap_or(false)
}

fn count_lines(path: &Path) -> usize {
    let text = fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("failed to read {} as UTF-8 text: {err}", path.display());
    });
    text.lines().count()
}
