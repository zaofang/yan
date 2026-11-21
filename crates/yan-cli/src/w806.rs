//! W806 on a branch is kept only when the branch is the example's only source of its result
//! (RFC 0091 D1): the `ex` lines of the function are rerun with the condition replaced by
//! `False`, and a warning whose examples still hold is dropped. Anything that cannot be
//! rerun keeps the warning.

use yan_db::{Database, Loaded};
use yan_diag::Diagnostic;
use yan_interp::{Limits, Stop};

use crate::examples::{self, Kind};

const BRANCH: &str = "this branch special-cases an `ex` input";

/// `diags` without the W806 warnings on `if` conditions that the rest of the body makes redundant.
pub fn confirm(file: &str, src: &str, diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diags
        .into_iter()
        .filter(|d| d.code != "W806" || !d.message.contains(BRANCH) || !redundant(file, src, d))
        .collect()
}

fn offset(src: &str, line: u32, col: u32) -> Option<usize> {
    let start = src
        .split_inclusive('\n')
        .take(line.checked_sub(1)? as usize)
        .map(str::len)
        .sum::<usize>();
    let text = src[start..].split('\n').next()?;
    let at = text
        .char_indices()
        .nth(col.checked_sub(1)? as usize)
        .map_or(text.len(), |(i, _)| i);
    Some(start + at)
}

fn redundant(file: &str, src: &str, d: &Diagnostic) -> bool {
    // The warning's span is the start of the condition; it runs to the `:` that ends the line.
    let Some(a) = offset(src, d.span.line, d.span.col) else {
        return false;
    };
    let eol = src[a..].find('\n').map_or(src.len(), |n| a + n);
    let Some(colon) = src[a..eol].rfind(':') else {
        return false;
    };
    let variant = format!("{}False{}", &src[..a], &src[a + colon..]);
    let (text, exs) = examples::augment(file, &variant, false);
    let mine: Vec<_> = exs
        .iter()
        .filter(|x| matches!(x.kind, Kind::Holds) && x.func == d.item)
        .collect();
    if mine.is_empty() {
        return false;
    }
    let mut db = Database::new(false, false);
    let f = db.file(std::path::Path::new(file), &text);
    let Ok(Loaded::Ready(m, _, _)) = yan_db::load(&db, f) else {
        return false;
    };
    mine.iter().all(|x| {
        let (mut out, mut steps) = (Vec::new(), 0);
        let r = yan_interp::test_metered(m, &x.name, Limits::default(), &mut out, &mut steps);
        !matches!(r, Err(Stop::Panic(_)) | Err(Stop::Limit(_)) | Ok(false)) && r.is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w806(src: &str) -> Vec<String> {
        let mut db = Database::new(false, false);
        let f = db.file(
            std::path::Path::new("t.yag"),
            &examples::augment("t.yag", src, false).0,
        );
        let all = yan_ide::diagnostics(&db, f).expect("diagnostics");
        let all = examples::remap(all, &examples::augment("t.yag", src, false).1);
        confirm("t.yag", src, all)
            .into_iter()
            .filter(|d| d.code == "W806")
            .map(|d| d.message)
            .collect()
    }

    #[test]
    fn a_guard_the_body_already_covers_is_not_flagged() {
        let src = "def f(s: str) -> bool:\n    ex f(\"\") == False\n    ex f(\"a\") == True\n    if s == \"\":\n        return False\n    return s.char_count() > 0\n";
        assert!(w806(src).is_empty());
    }

    #[test]
    fn a_hard_coded_branch_is_flagged() {
        let src = "def f(n: int) -> int:\n    ex f(7) == 42\n    if n == 7:\n        return 42\n    return n\n";
        assert_eq!(w806(src).len(), 1);
    }
}
