//! Word-wrapping for [`TextArea`](super::TextArea): splits the source text into
//! visual lines with byte-offset ranges back into the source, so the widget's
//! cursor, selection and hit-testing (`geometry.rs`, `edit_ops.rs`) can map
//! between visual lines and text offsets. Split out of `text_area.rs` to keep
//! that file under the project's 800-line cap.

use super::*;

/// A single visual line produced by [`wrap_text_indexed`].
#[derive(Clone, Debug)]
pub(super) struct WrappedLine {
    /// Inclusive byte offset into the source `text` where this visual
    /// line's content begins.
    pub(super) start: usize,
    /// Exclusive byte offset where this visual line's content ends
    /// (not including a trailing newline).
    pub(super) end: usize,
    /// Rendered text for this visual line (a substring of the source).
    pub(super) text: String,
    /// Whether this visual line ended because of an explicit `\n` in
    /// the source (vs. a soft wrap at word boundary).  Used to choose
    /// whether moving the cursor past the end of the line lands on
    /// the next visual line or just past the newline character.
    pub(super) hard_break: bool,
}

/// Wrap `text` at `max_width` and return the visual lines along with
/// byte-offset ranges back into the source.  Explicit `\n` always
/// produces a line break; between newlines, word-boundary soft wraps
/// keep each visual line ≤ `max_width`.  An empty source still returns
/// one empty line (so the cursor has somewhere to sit).
pub(super) fn wrap_text_indexed(
    font: &Arc<Font>,
    text: &str,
    font_size: f64,
    max_width: f64,
) -> Vec<WrappedLine> {
    let mut out: Vec<WrappedLine> = Vec::new();
    let mut para_start = 0usize;
    for (rel_end, chunk) in split_keep_newlines(text).enumerate() {
        let _ = rel_end;
        let para = chunk;
        let para_abs_start = para_start;
        let para_abs_end = para_abs_start + para.len();
        // Each paragraph soft-wraps independently.  Walk its char
        // byte indices and fill lines up to `max_width`.
        let mut cursor = 0usize; // byte offset within `para`
        let last_boundary = 0usize;
        while cursor < para.len() {
            // Find the longest prefix of `para[line_start..]` that
            // fits in `max_width`.  Use word boundaries — fall back
            // to the full prefix when no boundary is available (long
            // unbroken token).
            let line_start = cursor;
            let mut fit_end = line_start;
            let mut last_word_end: Option<usize> = None;
            let mut idx = line_start;
            while idx < para.len() {
                let next = next_char_boundary(para, idx);
                let candidate = &para[line_start..next];
                let w = measure_text_metrics(font, candidate, font_size).width;
                if w > max_width && fit_end > line_start {
                    break;
                }
                fit_end = next;
                // Record word boundaries as we pass them.
                if next < para.len() {
                    let next_ch = para[next..].chars().next().unwrap_or(' ');
                    if next_ch.is_whitespace() {
                        last_word_end = Some(next);
                    }
                }
                idx = next;
            }
            // Decide where to break: the last word boundary if we have
            // one AND we're not at the end of the paragraph; else just
            // at `fit_end`.
            let break_at = if fit_end < para.len() && last_word_end.is_some() {
                last_word_end.unwrap()
            } else {
                fit_end.max(next_char_boundary(para, line_start))
            };
            let _ = last_boundary; // reserved for future hyphenation
            let line_text = para[line_start..break_at].trim_end().to_string();
            let abs_start = para_abs_start + line_start;
            let abs_end = para_abs_start + break_at;
            out.push(WrappedLine {
                start: abs_start,
                end: abs_end,
                text: line_text,
                hard_break: false,
            });
            // Skip over the whitespace we just consumed as a separator.
            let mut next_line_start = break_at;
            while next_line_start < para.len() {
                let ch = para[next_line_start..].chars().next().unwrap_or('x');
                if !ch.is_whitespace() || ch == '\n' {
                    break;
                }
                next_line_start = next_char_boundary(para, next_line_start);
            }
            cursor = next_line_start;
            if cursor >= para.len() {
                break;
            }
        }
        // Emit at least one line for an empty paragraph (blank line
        // between \n\n, or a fresh doc with no content).
        if out.is_empty() || out.last().map(|l| l.end).unwrap_or(0) != para_abs_end {
            if para.is_empty() {
                out.push(WrappedLine {
                    start: para_abs_start,
                    end: para_abs_end,
                    text: String::new(),
                    hard_break: false,
                });
            }
        }
        // Mark the paragraph's last visual line as ending with a hard
        // break if the source had a trailing newline (see
        // `split_keep_newlines` contract below).
        let source_end = para_abs_end + 1; // +1 for the consumed '\n', if any
        let had_newline =
            source_end <= text.len() && text.as_bytes().get(para_abs_end) == Some(&b'\n');
        if had_newline {
            if let Some(last) = out.last_mut() {
                last.hard_break = true;
            }
        }
        para_start = if had_newline {
            source_end
        } else {
            para_abs_end
        };
    }
    if out.is_empty() {
        out.push(WrappedLine {
            start: 0,
            end: 0,
            text: String::new(),
            hard_break: false,
        });
    }
    out
}

/// Iterator over paragraph chunks — everything between `\n` boundaries
/// (newline is NOT included in the yielded chunk, but the caller can
/// detect its presence by comparing chunk byte-ranges to the source).
fn split_keep_newlines(text: &str) -> impl Iterator<Item = &str> + '_ {
    // `split('\n')` already gives the right semantics: consecutive \n's
    // yield empty strings so cursor can sit on blank lines, and a
    // trailing \n produces a final empty string (a blank final line).
    text.split('\n')
}
