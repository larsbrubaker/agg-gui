# Claude Code Guidelines — agg-gui

## Philosophy

**Quality through iterations** - Start with correct implementations, then improve. In a porting project, every function matters.

**Be a collaborator, not a stenographer.** Don't treat the developer's
instructions as gospel. Apply judgment, push back when something looks wrong,
and propose the approach you believe is the best practice or the most
appropriate solution for the problem — even if it differs from what was asked.
Disagree respectfully, explain the trade-offs, then defer once a decision is made.

## Test-First Bug Fixing (Critical Practice)

When a bug is reported, always follow this workflow:

1. **Write a reproducing test first** — create a failing test
2. **Fix the bug** — minimal change to address the root cause
3. **Verify via passing test** — the test must now pass

Do not skip the reproducing test. Even if the fix seems obvious.
Never commit a bug fix that isn't covered by a test.

## Testing

- Tests MUST test actual production code, not copies
- All tests must pass before advancing to the next phase
- When test failures occur, treat all failures as real bugs, resolve through instrumentation and root cause analysis, never by weakening tests

## Coding Standards

### File length
- **Hard limit: 800 lines.** Files that reach this must be refactored by splitting into
  focused modules before adding more code. Use code (python) to split modules if helpful
  to ensure we never lose code or functionality.
- Never reduce a file's line count by removing comments or blank lines to meet the limit —
  that is not refactoring. Split real logic into separate files/modules.

### Documentation
- Every file must begin with a comment block describing its purpose and how it relates
  to other modules in the project.
- Add doc comments to functions when they clarify intent, non-obvious behavior, or
  relationships to other parts of the codebase. Skip them when the function name and
  signature already tell the full story.

### Where documentation lives
- Documentation lives in the code: the header comment of the file, module or crate it
  describes, and the doc comments of its items, where whoever works on that code reads it.
  Write it in the present tense, saying why; no history.
- A rule that spans several files goes in the header of the crate that owns it (its `lib.rs`),
  and the files it governs point there. There are no design docs.
- Release notes for crate users go in that crate's `CHANGELOG.md`, in the entry of the version
  that made the change, migration steps included.
- Markdown is allowed only in: the root `README.md` and `CLAUDE.md`; a crate's own `README.md`,
  `CHANGELOG.md` and `THIRD_PARTY_NOTICES.md`; `.claude/` (agent instructions and skills); a
  file a `.rs` pulls into rustdoc with `include_str!` (the `DrawCtx` method docs in
  `agg-gui/src/draw_ctx/docs/`), while some `.rs` names it; and the root's `docs/later/`.
- An idea for later, written down when Lars asks for it, lives in `docs/later/<idea>.md`: it is
  not scheduled work and needs no plan entry; delete it when it is built or dropped.
- The `markdown_placement` test (`agg-gui/tests/markdown_placement.rs`) refuses any other
  Markdown file.

### Icons
- Use **Font Awesome** icons throughout the UI. Render icons via their Unicode
  code points in the appropriate Font Awesome font face, not as image assets.

### Demos
- Reproduce the egui demos as closely as possible — match layout, wording,
  defaults, and interaction. Consult the egui source in `egui-reference/`
  (a sync of the egui repo) whenever you touch a demo that exists there.
- Project conventions still win on conflict (Font Awesome, 800-line limit).
  Note any intentional deviation with a brief comment.

### General style
- Prefer `Result`/`Option` over `unwrap` in library code; `expect` is acceptable in
  `main` for startup failures with a clear message.
- Keep handler functions focused — if a handler grows complex, extract helpers.
- Avoid unsafe code unless there is no alternative; document every `unsafe` block.

### Performance
- **Never guess at performance problems by reading code.** Always measure first.
- Before optimizing, instrument the real workload and identify the actual bottleneck
  through profiling data or timing measurements.
- Validate that each change produces a measurable improvement. If it doesn't show up
  in the numbers, revert it.
- **Target: average frame rendering time must stay under 10 ms in the demo.**

## Shell

This project uses **PowerShell** on Windows for build scripts. Use the PowerShell tool
for them; the Bash tool is available separately and takes POSIX syntax.

## Orchestration pattern

The main session acts as planner and orchestrator only — it should not
write or edit code directly. All implementation is delegated to the `implementer`
subagent, one scoped step at a time. All post-change review is delegated to the
`reviewer` subagent. The main session handles only planning, architecture decisions,
and synthesizing subagent results.
## Branches and worktrees

When a task is complete, commit it, land it on `main` and push, then delete its branch and
remove its worktree — including any `worktree-agent-*` worktrees its sub-agents made (Lars,
2026-10-09). Only work still in progress keeps a branch. A worktree's git config inherits the
submodule's `core.worktree`, so git commands inside `.claude/worktrees/*` or `../agg-gui-wt-*`
need `--work-tree=<that path>`.
