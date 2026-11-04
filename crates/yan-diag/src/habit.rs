//! The habit table (R662): diagnostics that a Python habit triggers in Yan Agent, or a TypeScript
//! habit in Yan, gain a `habit` child that names the habit and the Yan form for it.

use crate::{Child, Diagnostic};

struct Habit {
    /// The file extension of the surface the habit shows up in.
    ext: &'static str,
    lang: &'static str,
    /// The codes it explains; empty for any.
    codes: &'static [&'static str],
    /// Whether the diagnostic's source line shows the habit.
    on: fn(&str) -> bool,
    text: &'static str,
}

const TABLE: &[Habit] = &[
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["E120"],
        on: |l| l.trim_start().starts_with("try") || l.trim_start().starts_with("except"),
        text: "Python `try`/`except`: Yan Agent has no exceptions; declare `fail E` and handle a \
               failure with `?`, `else` or a `match` on its `Result`",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &[],
        on: |l| l.contains("(Exception)"),
        text: "Python `class E(Exception)`: an error is a plain class, named after `fail` in the \
               signatures that can fail with it",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &[],
        on: |l| l.contains("with open("),
        text: "Python `with open(...) as f`: files go through the `fs` capability, as \
               `fs.read_text(path)`; `with name = e:` binds a handler",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["E100"],
        on: |l| l.starts_with(char::is_whitespace) && l.trim_start().starts_with("def "),
        text: "Python nested `def` that reads a local of the enclosing function: pass the value \
               as a parameter, or write a `lambda`",
    },
    Habit {
        ext: "yn",
        lang: "typescript",
        codes: &["E100"],
        on: |l| l.contains("=>"),
        text: "TypeScript arrow function `(x) => e`: a Yan lambda is `x -> e`, or `(x, y) -> e`",
    },
    Habit {
        ext: "yn",
        lang: "typescript",
        codes: &["W854"],
        on: |_| true,
        text: "TypeScript template literal `${x}`: Yan strings interpolate with `\\(x)`",
    },
    Habit {
        ext: "yn",
        lang: "typescript",
        codes: &["E403"],
        on: |_| true,
        text: "TypeScript `let` for a variable that changes: a Yan `let` is immutable; declare it \
               with `var`",
    },
];

/// Adds the habit child to each diagnostic whose code and source line match a habit. `src` is
/// the text of the diagnostics' file.
pub fn annotate(src: &str, diags: &mut [Diagnostic]) {
    let lines: Vec<&str> = src.lines().collect();
    for d in diags {
        let ext = d.file.rsplit('.').next().unwrap_or("");
        let Some(line) = lines.get((d.span.line as usize).wrapping_sub(1)) else {
            continue;
        };
        let found = TABLE.iter().find(|h| {
            h.ext == ext
                && (h.codes.is_empty() || h.codes.contains(&d.code.as_str()))
                && (h.on)(line)
        });
        if let Some(h) = found
            && !d.children.iter().any(|c| c.kind == "habit")
        {
            d.children.push(Child {
                kind: "habit".into(),
                lang: Some(h.lang.into()),
                text: h.text.into(),
            });
        }
    }
}

#[cfg(test)]
mod selftest {
    use super::*;
    use crate::Span;

    fn diag(code: &str, file: &str, line: u32) -> Diagnostic {
        let at = Span {
            line,
            col: 1,
            end_line: line,
            end_col: 2,
        };
        Diagnostic::error(code, file, at, "", "m")
    }

    #[test]
    fn habits_match_code_surface_and_line() {
        let src = "def f():\n    try:\n        pass\n";
        let mut ds = vec![
            diag("E120", "a.yag", 2),
            diag("E120", "a.yn", 2),
            diag("E201", "a.yag", 2),
        ];
        annotate(src, &mut ds);
        assert_eq!(ds[0].children[0].lang.as_deref(), Some("python"));
        assert!(ds[1].children.is_empty() && ds[2].children.is_empty());
        let v = serde_json::to_value(&ds[0]).unwrap();
        assert_eq!(v["children"][0]["kind"], "habit");
        assert!(ds[0].render_line(None).contains(" | note: Python `try`"));
        assert!(
            serde_json::to_value(&ds[2])
                .unwrap()
                .get("children")
                .is_none()
        );
    }
}
