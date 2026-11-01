//! std.regex patterns: one builder for the stage-0 check (E545) and the runtime, so both reject
//! the same patterns.

use regex::{Regex, RegexBuilder};

/// The most a compiled automaton may take (D5).
pub const SIZE_LIMIT: usize = 1 << 20;

/// Compile `pattern`; `Err` is the reason it is rejected.
pub fn build(pattern: &str) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .size_limit(SIZE_LIMIT)
        .build()
        .map_err(|e| match e {
            regex::Error::CompiledTooBig(_) => "too large".to_owned(),
            e => {
                let last = e
                    .to_string()
                    .lines()
                    .last()
                    .unwrap_or_default()
                    .trim()
                    .to_owned();
                last.strip_prefix("error: ").unwrap_or(&last).to_owned()
            }
        })
}

/// Python's replacement syntax (`\1`, `\g<name>`, `\\`) in the crate's (`${1}`, `${name}`, `$$`).
pub fn replacement(repl: &str) -> String {
    let mut out = String::new();
    let mut cs = repl.chars().peekable();
    while let Some(c) = cs.next() {
        match c {
            '$' => out.push_str("$$"),
            '\\' => match cs.peek().copied() {
                Some(d) if d.is_ascii_digit() => {
                    let mut n = String::new();
                    while let Some(d) = cs.peek().copied().filter(char::is_ascii_digit) {
                        n.push(d);
                        cs.next();
                    }
                    out.push_str(&format!("${{{n}}}"));
                }
                Some('g') => {
                    cs.next();
                    let name: String = cs.by_ref().skip(1).take_while(|&c| c != '>').collect();
                    out.push_str(&format!("${{{name}}}"));
                }
                Some('\\') => {
                    cs.next();
                    out.push('\\');
                }
                Some('n') => {
                    cs.next();
                    out.push('\n');
                }
                _ => out.push('\\'),
            },
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_what_linear_time_cannot_do() {
        assert!(build(r"(a)\1").is_err());
        assert!(build(r"(?=a)").is_err());
        assert!(build("(").is_err());
        assert!(build(r"(?P<y>\d+)-(\d+)").is_ok());
    }

    #[test]
    fn python_replacements() {
        assert_eq!(replacement(r"\2-\g<y> $\\"), "${2}-${y} $$\\");
    }
}
