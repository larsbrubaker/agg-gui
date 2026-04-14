# Claude Code Guidelines — agg-gui

## Philosophy

**Quality through iterations** - Start with correct implementations, then improve. In a porting project, every function matters.

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

### General style
- Prefer `Result`/`Option` over `unwrap` in library code; `expect` is acceptable in
  `main` for startup failures with a clear message.
- Keep handler functions focused — if a handler grows complex, extract helpers.
- Avoid unsafe code unless there is no alternative; document every `unsafe` block.

### Names
Follow Rust conventions (`snake_case` for functions/variables, `PascalCase` for types,
`SCREAMING_SNAKE_CASE` for constants).

### Refactoring
Improve code when it serves a purpose, not for aesthetics.

## Shell

This project uses **PowerShell** on Windows for build scripts. Claude Code's shell tool
runs bash/Linux, so adapt commands accordingly.