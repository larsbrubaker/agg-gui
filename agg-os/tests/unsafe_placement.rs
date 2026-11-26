//! Unsafe placement gate: the workspace's rule on `unsafe`, which agg-os owns
//! (its `src/lib.rs` states it). agg-os holds the workspace's unsafe code; a
//! member whose manifest sets `unsafe_code = "forbid"` under `[lints.rust]`
//! cannot contain any, because the compiler refuses it. This test reads the
//! root `Cargo.toml`'s `[workspace] members` and checks each member's manifest:
//! agg-os does not set the lint, every member in [`NOT_YET_FORBIDDEN`] does not
//! set it yet, and every other member sets it.
//!
//! The manifests are read with plain string handling (no TOML dependency):
//! table headers, `key = value` lines, and `#` comments.
//!
//! The repository root is the parent of this crate's directory. A copy of the
//! crate with no `.git` there (a crates.io download) is not a checkout of the
//! repository, and the gate passes without checking.

use std::fs;
use std::path::{Path, PathBuf};

/// The crate where unsafe code is allowed: it must not forbid it.
const UNSAFE_HOME: &str = "agg-os";

/// Members, by directory name, that still call the operating system directly
/// and so do not forbid unsafe code. Each leaves this list in the change that
/// moves its calls behind an agg-os function and adds the lint to its
/// manifest.
const NOT_YET_FORBIDDEN: &[&str] = &[
    "agg-gui-automation",
    "agg-gui-files",
    "agg-gui-shell",
    "agg-mic",
];

/// `line` without its `#` comment, trimmed. A `#` inside a quoted string is
/// kept (manifest strings here are simple: no escaped quotes are expected).
fn strip_comment(line: &str) -> &str {
    let mut quote: Option<char> = None;
    for (i, c) in line.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '#') => return line[..i].trim(),
            _ => {}
        }
    }
    line.trim()
}

/// The table name of a header line such as `[lints.rust]` (spaces inside the
/// brackets tolerated), or `None` for any other line. An array-of-tables
/// header (`[[bin]]`) counts as a table of its own name.
fn table_header(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.strip_suffix(']')?;
    let inner = inner
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(inner);
    Some(
        inner
            .split('.')
            .map(str::trim)
            .collect::<Vec<_>>()
            .join("."),
    )
}

/// The value of a `key = value` line, when its key is `key`.
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (k, v) = line.split_once('=')?;
    (k.trim() == key).then_some(v.trim())
}

/// True when a lint value means `forbid`: `"forbid"` or
/// `{ level = "forbid", ... }`.
fn is_forbid(value: &str) -> bool {
    let unquoted = |s: &str| s.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
    if unquoted(value) == "forbid" {
        return true;
    }
    let Some(inner) = value.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return false;
    };
    inner
        .split(',')
        .filter_map(|pair| value_of(pair, "level"))
        .any(|level| unquoted(level) == "forbid")
}

/// True when the manifest text's `[lints.rust]` table sets
/// `unsafe_code = "forbid"` on a line that is not commented out.
fn forbids_unsafe(manifest: &str) -> bool {
    let mut table = String::new();
    for raw in manifest.lines() {
        let line = strip_comment(raw);
        if let Some(name) = table_header(line) {
            table = name;
        } else if table == "lints.rust" && value_of(line, "unsafe_code").is_some_and(is_forbid) {
            return true;
        }
    }
    false
}

/// The `members` of the root manifest's `[workspace]` table, in order.
fn workspace_members(root_manifest: &str) -> Vec<String> {
    let mut members = Vec::new();
    let mut table = String::new();
    let mut in_list = false;
    for raw in root_manifest.lines() {
        let line = strip_comment(raw);
        if !in_list {
            if let Some(name) = table_header(line) {
                table = name;
                continue;
            }
            if table != "workspace" {
                continue;
            }
            let Some(value) = value_of(line, "members") else {
                continue;
            };
            let Some(rest) = value.strip_prefix('[') else {
                continue;
            };
            in_list = collect_quoted(rest, &mut members);
        } else {
            in_list = collect_quoted(line, &mut members);
        }
    }
    members
}

/// Pushes the quoted strings in `text` (part of an array) onto `out`; returns
/// false once the array's closing `]` is reached.
fn collect_quoted(text: &str, out: &mut Vec<String>) -> bool {
    let (body, open) = match text.find(']') {
        Some(end) => (&text[..end], false),
        None => (text, true),
    };
    for item in body.split(',') {
        let item = item.trim().trim_matches(|c| c == '"' || c == '\'');
        if !item.is_empty() {
            out.push(item.to_string());
        }
    }
    open
}

/// The repository root, or `None` for a copy of the crate outside a checkout.
fn repo_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    // `.git` is a directory in a clone and a file in a worktree or submodule.
    root.join(".git").exists().then_some(root)
}

/// The path of a member's manifest; the member name is `/`-separated.
fn member_manifest(root: &Path, member: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for part in member.split('/') {
        path.push(part);
    }
    path.join("Cargo.toml")
}

/// The members read from the checkout at `root`, each with whether it
/// forbids unsafe code.
fn member_lints(root: &Path) -> Vec<(String, bool)> {
    let root_manifest = fs::read_to_string(root.join("Cargo.toml"))
        .unwrap_or_else(|e| panic!("read the root Cargo.toml: {e}"));
    workspace_members(&root_manifest)
        .into_iter()
        .map(|member| {
            let path = member_manifest(root, &member);
            let text = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let forbids = forbids_unsafe(&text);
            (member, forbids)
        })
        .collect()
}

/// The rule's violations among `lints`, one message each, with `not_yet`
/// the members excused for now (the test passes [`NOT_YET_FORBIDDEN`]).
fn violations(lints: &[(String, bool)], not_yet: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for (member, forbids) in lints {
        let listed = not_yet.contains(&member.as_str());
        if member == UNSAFE_HOME {
            if *forbids {
                found.push(format!(
                    "{member}: its Cargo.toml sets unsafe_code = \"forbid\", but it is where \
                     the workspace's unsafe code lives; remove the lint"
                ));
            }
        } else if listed && *forbids {
            found.push(format!(
                "{member}: its Cargo.toml now sets unsafe_code = \"forbid\"; take it off \
                 NOT_YET_FORBIDDEN in agg-os/tests/unsafe_placement.rs"
            ));
        } else if !listed && !*forbids {
            found.push(format!(
                "{member}: its Cargo.toml does not set unsafe_code = \"forbid\" under \
                 [lints.rust]; add the lint (with the comment the other members carry above \
                 it), and move any unsafe code it needs into an agg-os function"
            ));
        }
    }
    for listed in not_yet {
        if !lints.iter().any(|(member, _)| member == listed) {
            found.push(format!(
                "{listed}: listed in NOT_YET_FORBIDDEN but not a workspace member; take it \
                 off the list"
            ));
        }
    }
    found
}

#[test]
fn every_member_forbids_unsafe_except_agg_os_and_the_listed_ones() {
    let Some(root) = repo_root() else {
        eprintln!("unsafe_placement: not a checkout of the repository (no .git); skipped");
        return;
    };
    let lints = member_lints(&root);
    // Guards against a check of nothing (wrong root, a members list misread).
    assert!(
        !lints.is_empty(),
        "read no workspace members from the root Cargo.toml"
    );
    for expected in ["agg-gui", UNSAFE_HOME] {
        assert!(
            lints.iter().any(|(member, _)| member == expected),
            "the workspace members read from the root Cargo.toml miss {expected}"
        );
    }
    let found = violations(&lints, NOT_YET_FORBIDDEN);
    assert!(
        found.is_empty(),
        "the workspace's rule on unsafe (agg-os/src/lib.rs) is broken:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn forbids_unsafe_reads_the_lint_only_where_it_applies() {
    assert!(forbids_unsafe(
        "[package]\nname = \"a\"\n\n[lints.rust]\nunsafe_code = \"forbid\"\n"
    ));
    assert!(forbids_unsafe(
        "[ lints . rust ]\n  unsafe_code=\"forbid\"   # the rule\n[lints.clippy]\n"
    ));
    assert!(forbids_unsafe(
        "[lints.rust]\nunsafe_code = { level = \"forbid\", priority = 1 }\n"
    ));
    // Absent.
    assert!(!forbids_unsafe("[package]\nname = \"a\"\n"));
    assert!(!forbids_unsafe(
        "[lints.rust]\nunsafe_op_in_unsafe_fn = \"deny\"\n"
    ));
    // A weaker level.
    assert!(!forbids_unsafe("[lints.rust]\nunsafe_code = \"deny\"\n"));
    // Under another table.
    assert!(!forbids_unsafe(
        "[lints.clippy]\nunsafe_code = \"forbid\"\n"
    ));
    assert!(!forbids_unsafe(
        "[lints.rust]\n\n[lints.clippy]\nunsafe_code = \"forbid\"\n"
    ));
    // Commented out.
    assert!(!forbids_unsafe(
        "[lints.rust]\n# unsafe_code = \"forbid\"\n"
    ));
    assert!(!forbids_unsafe(
        "# [lints.rust]\n# unsafe_code = \"forbid\"\n"
    ));
}

#[test]
fn workspace_members_reads_the_members_array() {
    let manifest = "[package]\nmembers = [\"not-this\"]\n\n[workspace]\nresolver = \"2\"\n\
                    members = [\n    \"a\",\n    # \"commented\",\n    \"b\", # trailing\n\
                    \"c/d\" ]\nexclude = [\"e\"]\n";
    assert_eq!(workspace_members(manifest), ["a", "b", "c/d"]);
    assert_eq!(
        workspace_members("[workspace]\nmembers = [\"x\", \"y\"]\n"),
        ["x", "y"]
    );
    assert!(workspace_members("[package]\nname = \"a\"\n").is_empty());
}

#[test]
fn violations_name_the_member_and_what_to_do() {
    let lints = vec![
        (UNSAFE_HOME.to_string(), true),
        ("plain".to_string(), false),
        ("listed-now-forbids".to_string(), true),
        ("listed".to_string(), false),
        ("forbids".to_string(), true),
    ];
    let not_yet = ["listed-now-forbids", "listed", "gone"];
    let found = violations(&lints, &not_yet);
    assert_eq!(found.len(), 4, "{found:#?}");
    assert!(found[0].starts_with("agg-os:") && found[0].contains("remove the lint"));
    assert!(found[1].starts_with("plain:") && found[1].contains("add the lint"));
    assert!(found[2].starts_with("listed-now-forbids:") && found[2].contains("take it off"));
    assert!(found[3].starts_with("gone:") && found[3].contains("not a workspace"));
}
