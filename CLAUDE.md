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

**Running tests:**
```bash
cargo test
cargo test --lib vec_tests 
cargo test test_name -- --exact
cargo test -- --nocapture
```

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
- Explain *why*, not *what*. A comment that restates the code is noise.

### Coordinate system
- **Y-axis is inverted (bottom-up).** Origin is at the bottom-left, so +Y points upward.
  Code from external sources (web, AI tools, other libraries) almost always assumes
  top-down Y. Watch for this in default positioning, collapse direction, SVG orientation,
  scroll offsets, and hit-testing.

### Icons
- Use **Font Awesome** icons throughout the UI. Render icons via their Unicode
  code points in the appropriate Font Awesome font face, not as image assets.

### Demos
- Reproduce the egui demos as closely as possible — match layout, wording,
  defaults, and interaction. Consult the egui source in `cpp-reference/`
  (a sync of the egui repo) whenever you touch a demo that exists there.
- Project conventions still win on conflict (Y-up, Font Awesome, 800-line limit).
  Note any intentional deviation with a brief comment.

### General style
- Prefer `Result`/`Option` over `unwrap` in library code; `expect` is acceptable in
  `main` for startup failures with a clear message.
- Keep handler functions focused — if a handler grows complex, extract helpers.
- Avoid unsafe code unless there is no alternative; document every `unsafe` block.

### Names
Follow Rust conventions (`snake_case` for functions/variables, `PascalCase` for types,
`SCREAMING_SNAKE_CASE` for constants).

### Performance
- **Never guess at performance problems by reading code.** Always measure first.
- Before optimizing, instrument the real workload and identify the actual bottleneck
  through profiling data or timing measurements.
- Validate that each change produces a measurable improvement. If it doesn't show up
  in the numbers, revert it.
- **Target: average frame rendering time must stay under 10 ms in the demo.**

### Refactoring
Improve code when it serves a purpose, not for aesthetics.

## Shell

This project uses **PowerShell** on Windows for build scripts. Claude Code's shell tool
runs bash/Linux, so adapt commands accordingly.