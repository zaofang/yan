//! Diagnostic records (spec 18.2) and their line rendering (R294).

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;
pub const DEFAULT_MAX_ERRORS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub schema: u32,
    pub code: String,
    pub severity: Severity,
    pub item: String,
    pub file: String,
    pub span: Span,
    pub message: String,
    pub notes: Vec<String>,
    pub fixes: Vec<serde_json::Value>,
    pub counterexample: Option<serde_json::Value>,
    pub quote: String,
    /// Secondary spans, each with what happened there (R290).
    #[serde(default)]
    pub labels: Vec<Label>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Label {
    pub span: Span,
    pub text: String,
}

impl Diagnostic {
    pub fn error(
        code: &str,
        file: &str,
        span: Span,
        quote: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            schema: SCHEMA,
            code: code.to_owned(),
            severity: Severity::Error,
            item: String::new(),
            file: file.to_owned(),
            span,
            message: message.into(),
            notes: Vec::new(),
            fixes: Vec::new(),
            counterexample: None,
            quote: quote.to_owned(),
            labels: Vec::new(),
        }
    }

    pub fn with_item(mut self, item: impl Into<String>) -> Self {
        self.item = item.into();
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_label(mut self, span: Span, text: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            text: text.into(),
        });
        self
    }

    /// Adds a one-edit fix that replaces `search` at `span` with `text` (R292).
    pub fn with_fix(
        mut self,
        applicability: Applicability,
        title: impl Into<String>,
        span: Span,
        search: &str,
        text: &str,
    ) -> Self {
        self.fixes.push(serde_json::json!({
            "title": title.into(),
            "applicability": applicability,
            "edits": [{"span": span, "search": search, "text": text}],
        }));
        self
    }

    /// `[#n ] code path:line:col[ item:][ `quote`] message { | note } { | kind: old -> new }`
    /// (R294, R650). A label renders as the note `text at line:col`.
    pub fn render_line(&self, id: Option<usize>) -> String {
        let mut out = String::new();
        if let Some(n) = id {
            out.push_str(&format!("#{n} "));
        }
        out.push_str(&format!(
            "{} {}:{}:{}",
            self.code, self.file, self.span.line, self.span.col
        ));
        if !self.item.is_empty() {
            out.push_str(&format!(" {}:", self.item));
        }
        if !self.quote.is_empty() {
            out.push_str(&format!(" `{}`", self.quote));
        }
        out.push(' ');
        out.push_str(&self.message);
        for l in &self.labels {
            out.push_str(&format!(" | {} at {}:{}", l.text, l.span.line, l.span.col));
        }
        for note in &self.notes {
            out.push_str(" | ");
            out.push_str(note);
        }
        for fix in &self.fixes {
            let kind = match fix["applicability"].as_str() {
                Some("auto") => "fix",
                Some("choice") => "maybe",
                Some("template") => "fill",
                _ => continue,
            };
            let edit = &fix["edits"][0];
            if let (Some(old), Some(new)) = (edit["search"].as_str(), edit["text"].as_str()) {
                out.push_str(&format!(" | {kind}: {old} -> {new}"));
            }
        }
        out.replace('\n', "\\n")
    }
}

/// How a fix may be applied (R292): `auto` keeps meaning, `choice` changes it, `template`
/// has typed holes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Applicability {
    Auto,
    Choice,
    Template,
}

/// The candidates within edit distance 2 of `name`, nearest first, at most three (R651).
pub fn near<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut hits: Vec<(usize, &str)> = candidates
        .into_iter()
        .filter(|c| *c != name)
        .map(|c| (distance(name, c), c))
        .filter(|(d, _)| *d <= 2)
        .collect();
    hits.sort();
    hits.dedup();
    hits.into_iter().take(3).map(|(_, c)| c).collect()
}

/// Optimal string alignment distance: insertions, deletions, substitutions and adjacent
/// transpositions, by characters.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    d[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// Sort by file, line, column, then code, and keep at most `max_errors` errors (18.2).
pub fn finish(mut diags: Vec<Diagnostic>, max_errors: usize) -> Vec<Diagnostic> {
    diags.sort_by(|a, b| {
        (&a.file, a.span.line, a.span.col, &a.code).cmp(&(
            &b.file,
            b.span.line,
            b.span.col,
            &b.code,
        ))
    });
    let mut errors = 0;
    diags.retain(|d| {
        if d.severity != Severity::Error {
            return true;
        }
        errors += 1;
        errors <= max_errors
    });
    diags
}

#[cfg(test)]
mod selftest {
    use super::*;

    fn at(line: u32, col: u32) -> Span {
        Span {
            line,
            col,
            end_line: line,
            end_col: col + 1,
        }
    }

    #[test]
    fn json_uses_spec_field_names() {
        let d = Diagnostic::error("E100", "a.yong", at(1, 2), "}", "unexpected token");
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["severity"], "error");
        assert_eq!(v["span"]["end_col"], 3);
        assert!(v["counterexample"].is_null());
    }

    #[test]
    fn line_rendering() {
        let d = Diagnostic::error(
            "E501",
            "src/p.xin",
            at(14, 9),
            "xs[i]",
            "index not proven in bounds",
        )
        .with_item("value")
        .with_note("a\nb");
        assert_eq!(
            d.render_line(Some(1)),
            "#1 E501 src/p.xin:14:9 value: `xs[i]` index not proven in bounds | a\\nb"
        );
    }

    #[test]
    fn line_rendering_omits_empty_item_and_quote() {
        let d = Diagnostic::error("P002", "m.xin", at(5, 1), "", "`t` matches 9 times");
        assert_eq!(d.render_line(None), "P002 m.xin:5:1 `t` matches 9 times");
        let d = Diagnostic::error("E101", "f.xin", at(6, 12), "a", "needs parentheses");
        assert_eq!(d.render_line(None), "E101 f.xin:6:12 `a` needs parentheses");
    }

    #[test]
    fn near_names_nearest_first() {
        let names = ["len", "lens", "length", "sort", "x"];
        assert_eq!(near("lenght", names), ["length"]);
        assert_eq!(near("lne", names), ["len", "lens"]);
        assert!(near("zzzzzz", names).is_empty());
    }

    #[test]
    fn line_rendering_shows_fixes() {
        let d = Diagnostic::error("E200", "f.xin", at(2, 12), "lenght", "cannot find `lenght`")
            .with_item("f")
            .with_fix(
                Applicability::Choice,
                "use `length`",
                at(2, 12),
                "lenght",
                "length",
            );
        assert_eq!(
            d.render_line(None),
            "E200 f.xin:2:12 f: `lenght` cannot find `lenght` | maybe: lenght -> length"
        );
        let v = serde_json::to_value(&d).unwrap();
        assert_eq!(v["fixes"][0]["applicability"], "choice");
    }

    #[test]
    fn finish_sorts_and_caps_errors_only() {
        let mut w = Diagnostic::error("W800", "a", at(9, 1), "x", "w");
        w.severity = Severity::Warning;
        let diags = vec![
            Diagnostic::error("E2", "a", at(2, 1), "x", "e"),
            w,
            Diagnostic::error("E1", "a", at(2, 1), "x", "e"),
            Diagnostic::error("E3", "a", at(1, 5), "x", "e"),
        ];
        let codes: Vec<_> = finish(diags, 2).into_iter().map(|d| d.code).collect();
        assert_eq!(codes, ["E3", "E1", "W800"]);
    }
}
