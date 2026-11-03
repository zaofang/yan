//! std.regex kernels: one builder for the stage-0 check (E545) and the runtime, so both reject
//! the same patterns, and a cache of compiled automata by pattern text.

use std::cell::RefCell;
use std::collections::HashMap;

use regex::RegexBuilder;
pub use regex::{Captures, Regex, escape};

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

#[derive(Clone, Copy)]
pub enum Anchor {
    None,
    Start,
    Both,
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Regex>> = RefCell::new(HashMap::new());
}

/// `pattern`, which `build` accepted when its `Regex` was made, anchored as `a`.
pub fn compiled(pattern: &str, a: Anchor) -> Regex {
    let p = match a {
        Anchor::None => pattern.to_owned(),
        Anchor::Start => format!(r"\A(?:{pattern})"),
        Anchor::Both => format!(r"\A(?:{pattern})\z"),
    };
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(p.clone())
            .or_insert_with(|| build(&p).expect("checked by compile"))
            .clone()
    })
}

/// As Python: captured groups are kept between the pieces, "" for one that did not take part.
pub fn split<'s>(re: &Regex, s: &'s str) -> Vec<&'s str> {
    let mut out = Vec::new();
    let mut last = 0;
    for c in re.captures_iter(s) {
        let m = c.get(0).expect("group 0");
        out.push(&s[last..m.start()]);
        out.extend(c.iter().skip(1).map(|g| g.map_or("", |g| g.as_str())));
        last = m.end();
    }
    out.push(&s[last..]);
    out
}

/// Every match of `re` in `s` replaced by `repl` in Python's syntax.
pub fn sub(re: &Regex, repl: &str, s: &str) -> String {
    re.replace_all(s, replacement(repl).as_str()).into_owned()
}

/// The char offset of byte offset `byte` in `s`.
pub fn chars(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
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
    fn split_keeps_groups() {
        let re = compiled(r"(-)|,", Anchor::None);
        assert_eq!(split(&re, "a-b,c"), ["a", "-", "b", "", "c"]);
    }

    #[test]
    fn python_replacements() {
        assert_eq!(replacement(r"\2-\g<y> $\\"), "${2}-${y} $$\\");
    }
}
