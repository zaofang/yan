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
               `fs.read(path)`; `with name = e:` binds a handler",
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
        ext: "yag",
        lang: "python",
        codes: &["E100"],
        on: |l| {
            let l = l.trim_start();
            l.starts_with("ex ")
                && (l.contains("raises") || l.contains("fails ") || l.contains("pytest."))
        },
        text: "Python `pytest.raises(E)` or `fails E` in an example: state a failure as \
               `ex f(x) == fail E`",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["E100"],
        on: |l| {
            let l = l.trim_start();
            l.starts_with("ex ") && l.contains(" fail ") && !l.contains("== fail ")
        },
        text: "`fail E` in an `ex` line is an operand: write `ex f(x) == fail E` to state a failure",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["E504"],
        on: |l| l.contains('[') && !l.contains(':'),
        text: "Python `s[i]` on a `str`: write `s.char_at(i)` (it walks the string, so it costs \
               O(n)), or bind `cs = s.chars().collect()` once and index `cs[i]`",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["E504"],
        on: |l| l.contains('[') && l.contains(':'),
        text: "Python slice `s[a:b]` on a `str`: bind `cs = s.chars().collect()` and slice the \
               list `cs[a:b]`",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["E403"],
        on: |l| {
            let l = l.trim_start();
            l.contains(" = ") || l.contains(" += ") || l.contains(" -= ") || l.contains(" *= ")
        },
        text: "Python assigns to a parameter or a `let` name: here it is read-only. Copy it to a \
               new local (`n = x`) and change that, or declare the parameter `var x: T` if the \
               caller should see the change",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["E315"],
        on: |_| true,
        text: "Python raises a callee's error implicitly; Yan Agent does not. Write `f(x)?` to \
               pass the failure on (this function must declare the same `fail E`), or `f(x) else \
               v` or `f(x) else err -> v` to handle it",
    },
    Habit {
        ext: "yag",
        lang: "python",
        codes: &["W823"],
        on: |_| true,
        text: "Python annotates locals; Yan Agent infers them. Keep an annotation only where the \
               value alone does not give the type, as in `xs: list[int] = []`",
    },
    Habit {
        ext: "yn",
        lang: "python",
        codes: &["E100"],
        on: |l| l.trim_end().ends_with(':'),
        text: "Python block colon: Yan opens a block by indentation alone, so `if c`, `for x in \
               xs` and `with fs = fs.sub(d)` take no trailing `:`",
    },
    Habit {
        ext: "yn",
        lang: "javascript",
        codes: &["E203"],
        on: |l| l.contains(".push("),
        text: "JavaScript `xs.push(v)`: the list method is `xs.append(v)` on a `var xs`",
    },
    Habit {
        ext: "yag",
        lang: "javascript",
        codes: &["E203"],
        on: |l| l.contains(".push("),
        text: "JavaScript `xs.push(v)`: the list method is `xs.append(v)`",
    },
    Habit {
        ext: "yn",
        lang: "rust",
        codes: &["E017"],
        on: |l| {
            let l = l.trim();
            l.ends_with('?') && !l.contains('(')
        },
        text: "`?` applies to a call: write it where the failure arises (`check(x)?`, inside the \
               `par for` body for the lowest-index failure), not on a bound value",
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
    fn ex_failure_forms_get_the_fix() {
        let src = "    ex f(0) fail E\n    ex f(0) fails E\n    ex f(0) == fail E\n";
        let mut ds = vec![
            diag("E100", "a.yag", 1),
            diag("E100", "a.yag", 2),
            diag("E100", "a.yag", 3),
        ];
        annotate(src, &mut ds);
        assert!(ds[0].children[0].text.contains("ex f(x) == fail E"));
        assert!(ds[1].children[0].text.contains("ex f(x) == fail E"));
        assert!(ds[2].children.is_empty());
    }

    #[test]
    fn remaining_yag_hints() {
        let src = "    c = s[0]\n    c = s[1:2]\n    x = x + 1\n    n = f(a)\n    y: int = 1\n";
        let mut ds = vec![
            diag("E504", "a.yag", 1),
            diag("E504", "a.yag", 2),
            diag("E403", "a.yag", 3),
            diag("E315", "a.yag", 4),
            diag("W823", "a.yag", 5),
        ];
        annotate(src, &mut ds);
        let t = |i: usize| ds[i].children[0].text.as_str();
        assert!(t(0).contains("char_at"));
        assert!(t(1).contains("cs[a:b]"));
        assert!(t(2).contains("var x: T"));
        assert!(t(3).contains("f(x)?"));
        assert!(t(4).contains("xs: list[int]"));
    }

    #[test]
    fn benchmark_gap_hints() {
        let src = "  with fs = fs.sub(\"n\"):\n  o.push(x)\n  ys?\n";
        let mut ds = vec![
            diag("E100", "a.yn", 1),
            diag("E203", "a.yn", 2),
            diag("E017", "a.yn", 3),
        ];
        annotate(src, &mut ds);
        let t = |i: usize| ds[i].children[0].text.as_str();
        assert!(t(0).contains("no trailing `:`"));
        assert!(t(1).contains("append"));
        assert!(t(2).contains("check(x)?"));
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
