//! Prose output styling (Workshop Standard L5, L10). Colours are the standard ANSI indices, so
//! the user's terminal theme applies; `site/brand/tokens.json` gives Yan's own 16-colour palette.

use std::io::IsTerminal;

use yan_diag::Severity;

pub struct Term {
    colour: bool,
}

impl Term {
    /// Colour only on a terminal, and never when `NO_COLOR` is set and non-empty.
    pub fn stdout() -> Self {
        let no_colour = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        Term {
            colour: std::io::stdout().is_terminal() && !no_colour,
        }
    }

    fn paint(&self, sgr: &str, s: &str) -> String {
        if self.colour {
            format!("\x1b[{sgr}m{s}\x1b[0m")
        } else {
            s.to_owned()
        }
    }

    pub fn header(&self) -> String {
        format!(
            "{} yan {}",
            self.paint("31", "言"),
            env!("CARGO_PKG_VERSION")
        )
    }

    /// Colours the first line of a rendered diagnostic: glyph, severity and code.
    pub fn diagnostic(&self, severity: Severity, rendered: &str) -> String {
        let sgr = match severity {
            Severity::Error => "1;31",
            Severity::Warning => "33",
            Severity::Note => "34",
        };
        match rendered.split_once('\n') {
            Some((head, rest)) => format!("{}\n{rest}", self.paint(sgr, head)),
            None => self.paint(sgr, rendered),
        }
    }

    pub fn summary(&self, files: usize, errors: usize, warnings: usize) -> String {
        let n =
            |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
        let checked = n(files, "file", "files");
        match (errors, warnings) {
            (0, 0) => self.paint("32", &format!("✓ {checked} checked")),
            (0, w) => self.paint(
                "33",
                &format!("▲ {} in {checked}", n(w, "warning", "warnings")),
            ),
            (e, 0) => self.paint(
                "1;31",
                &format!("✗ {} in {checked}", n(e, "error", "errors")),
            ),
            (e, w) => self.paint(
                "1;31",
                &format!(
                    "✗ {}, {} in {checked}",
                    n(e, "error", "errors"),
                    n(w, "warning", "warnings")
                ),
            ),
        }
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn plain_without_colour() {
        let t = Term { colour: false };
        assert_eq!(t.summary(3, 0, 0), "✓ 3 files checked");
        assert_eq!(t.summary(1, 2, 1), "✗ 2 errors, 1 warning in 1 file");
        assert_eq!(t.summary(2, 0, 1), "▲ 1 warning in 2 files");
        assert_eq!(t.diagnostic(Severity::Error, "a\nb"), "a\nb");
        assert!(t.header().starts_with("言 yan "));
    }

    #[test]
    fn colours_only_the_first_line() {
        let t = Term { colour: true };
        assert_eq!(
            t.diagnostic(Severity::Warning, "a\nb"),
            "\x1b[33ma\x1b[0m\nb"
        );
    }
}
