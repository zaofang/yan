//! `yan fix` (18.2, R292): applies the `auto` fixes of a file's diagnostics until none is left.

use std::path::Path;

use yan_diag::{Diagnostic, Span};

use crate::patch::{self, Applied, Diagnose};

/// A fix pass over one file feeds the next, since a fix can expose another.
const MAX_PASSES: usize = 8;

pub struct Fixed {
    pub applied: usize,
}

/// The text of `src` after the first `auto` fix of each diagnostic, and how many were applied.
/// Overlapping edits and edits whose search text no longer sits at the span are skipped.
fn pass(src: &str, diags: &[Diagnostic]) -> (String, usize) {
    let mut edits: Vec<(Span, &str, &str)> = diags
        .iter()
        .filter_map(|d| d.fixes.iter().find(|f| f["applicability"] == "auto"))
        .filter_map(|f| {
            let e = &f["edits"][0];
            let span = serde_json::from_value(e["span"].clone()).ok()?;
            Some((span, e["search"].as_str()?, e["text"].as_str()?))
        })
        .collect();
    edits.sort_by_key(|(s, ..)| std::cmp::Reverse((s.line, s.col)));
    let mut text = src.to_owned();
    let starts: Vec<usize> = std::iter::once(0)
        .chain(src.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    let mut count = 0;
    let mut floor = usize::MAX;
    for (span, search, replacement) in edits {
        let Some(&start) = starts.get(span.line.saturating_sub(1) as usize) else {
            continue;
        };
        let from = start + span.col.saturating_sub(1) as usize;
        let to = from + search.len();
        if to > floor || src.get(from..to) != Some(search) {
            continue;
        }
        text.replace_range(from..to, replacement);
        floor = from;
        count += 1;
    }
    (text, count)
}

/// Fixes `file` until stable. Writes (and logs the patch) unless `dry_run`.
pub fn run(file: &Path, dry_run: bool, diagnose: Diagnose) -> Result<Fixed, String> {
    let before = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut after = before.clone();
    let mut applied = 0;
    for _ in 0..MAX_PASSES {
        let (next, n) = pass(&after, &diagnose(file, &after)?);
        if n == 0 {
            break;
        }
        after = next;
        applied += n;
    }
    if applied > 0 && !dry_run {
        let dir = file
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let a = Applied {
            target: file.to_owned(),
            before: before.clone(),
            after: after.clone(),
            hashes: Vec::new(),
        };
        patch::commit(dir, &a)?;
    }
    Ok(Fixed { applied })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(line: u32, col: u32, search: &str, text: &str, kind: &str) -> Diagnostic {
        let span = Span {
            line,
            col,
            end_line: line,
            end_col: col + search.len() as u32,
        };
        let mut d = Diagnostic::error("E000", "m", span, "", "m");
        d.fixes.push(serde_json::json!({
            "title": "t", "applicability": kind,
            "edits": [{"span": span, "search": search, "text": text}],
        }));
        d
    }

    #[test]
    fn applies_auto_fixes_right_to_left_and_skips_choices() {
        let src = "a b\nc d";
        let ds = [
            diag(1, 1, "a", "xx", "auto"),
            diag(1, 3, "b", "yy", "auto"),
            diag(2, 1, "c", "zz", "choice"),
        ];
        assert_eq!(pass(src, &ds), ("xx yy\nc d".to_owned(), 2));
    }

    #[test]
    fn skips_stale_and_overlapping_edits() {
        let src = "abc";
        let ds = [diag(1, 1, "ab", "X", "auto"), diag(1, 2, "bc", "Y", "auto")];
        assert_eq!(pass(src, &ds), ("aY".to_owned(), 1));
        assert_eq!(
            pass(src, &[diag(1, 1, "zz", "X", "auto")]),
            (src.to_owned(), 0)
        );
    }

    #[test]
    fn applies_an_edit_that_spans_lines() {
        let src = "a\nb\n  c\nd";
        let span = Span {
            line: 2,
            col: 1,
            end_line: 3,
            end_col: 4,
        };
        let mut d = Diagnostic::error("E000", "m", span, "", "m");
        d.fixes.push(serde_json::json!({
            "title": "t", "applicability": "auto",
            "edits": [{"span": span, "search": "b\n  c\n", "text": ""}],
        }));
        assert_eq!(pass(src, &[d]), ("a\nd".to_owned(), 1));
    }
}
