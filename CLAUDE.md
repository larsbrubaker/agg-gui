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
- Tests must verify **exact behavioral match** with the C++ implementation — same floating-point results, same triangle counts, same vertex positions
- To confirm numerical exactness, instrument both the Rust and C++ implementations and compare output byte-for-byte where applicable
- All tests must pass before advancing to the next phase
- When test failures occur, use the fix-test-failures skill — treat all failures as real bugs, resolve through instrumentation and root cause analysis, never by weakening tests

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
  focused modules before adding more code. Be very careful when refactoring. Use code (python) to split modules if helpful to ensure we never loose code or functionality.
- Never reduce a file's line count by removing comments or blank lines to meet the limit —
  that is not refactoring. Split real logic into separate files/modules.
- **Exceptions:**


### General style
- Prefer `Result`/`Option` over `unwrap` in library code; `expect` is acceptable in
  `main` for startup failures with a clear message.
- Keep handler functions focused — if a handler grows complex, extract helpers.
- Avoid unsafe code unless there is no alternative; document every `unsafe` block.

### Names
Follow Rust conventions (`snake_case` for functions/variables, `PascalCase` for types, `SCREAMING_SNAKE_CASE` for constants). Mirror C++ names where they are clear; use better Rust names where they aren't.

### Comments
Explain *why*, not *what*. When porting, note where Rust differs from C++ and why.

### Refactoring
Improve code when it serves a purpose, not for aesthetics.

## Shell

This project uses **PowerShell** on Windows for build scripts. Use `bash` syntax in Bash tool calls.
