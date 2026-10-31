//! Diagnostic records (spec 18.2) and their line rendering (R294).

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;
pub const DEFAULT_MAX_ERRORS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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

    /// `[#n ] code path:line:col item: `quote` message { | note }` (R294).
    pub fn render_line(&self, id: Option<usize>) -> String {
        let mut out = String::new();
        if let Some(n) = id {
            out.push_str(&format!("#{n} "));
        }
        out.push_str(&format!(
            "{} {}:{}:{} {}: `{}` {}",
            self.code,
            self.file,
            self.span.line,
            self.span.col,
            self.item,
            self.quote,
            self.message
        ));
        for note in &self.notes {
            out.push_str(" | ");
            out.push_str(note);
        }
        out.replace('\n', "\\n")
    }
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
            "src/p.jian",
            at(14, 9),
            "xs[i]",
            "index not proven in bounds",
        )
        .with_item("value")
        .with_note("a\nb");
        assert_eq!(
            d.render_line(Some(1)),
            "#1 E501 src/p.jian:14:9 value: `xs[i]` index not proven in bounds | a\\nb"
        );
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
