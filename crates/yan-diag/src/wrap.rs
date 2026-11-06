//! Wrapping of printed lines past the width (spec 15.4, 18.5): inside brackets one element per
//! line with a trailing comma, and method chains with one leading `.` call per line.

/// A bracket group at depth 0 of a line.
struct Group {
    open: usize,
    close: usize,
    /// Byte offsets of the commas directly inside.
    commas: Vec<usize>,
    /// A `for` directly inside: a comprehension or generator, which takes no trailing comma.
    comprehension: bool,
    /// The opener touches the text before it: a call or subscript, not a grouping.
    attached: bool,
}

struct Scan {
    groups: Vec<Group>,
    /// Byte offsets of the depth-0 dots that start a method call.
    calls: Vec<usize>,
}

fn cols(s: &str) -> usize {
    s.chars().count()
}

/// The end of the string literal opening at `i`, past its closing quote.
fn skip_string(b: &[u8], mut i: usize) -> Option<usize> {
    let q = b[i];
    let triple = b.len() >= i + 3 && b[i + 1] == q && b[i + 2] == q;
    i += if triple { 3 } else { 1 };
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'{' if q == b'"' && b.get(i + 1) == Some(&b'{') => i += 2,
            b'{' if q == b'"' => i = skip_braces(b, i)?,
            c if c == q && !triple => return Some(i + 1),
            c if c == q && b.get(i + 1) == Some(&q) && b.get(i + 2) == Some(&q) => {
                return Some(i + 3);
            }
            _ => i += 1,
        }
    }
    None
}

/// The end of the interpolation opening at `i`, past its closing brace.
fn skip_braces(b: &[u8], mut i: usize) -> Option<usize> {
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => i = skip_string(b, i)?,
            b'{' => {
                depth += 1;
                i += 1;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => i += 1,
        }
    }
    None
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn scan(s: &str) -> Option<Scan> {
    let b = s.as_bytes();
    let mut out = Scan {
        groups: Vec::new(),
        calls: Vec::new(),
    };
    let mut stack: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'"' | b'\'' => {
                i = skip_string(b, i)?;
                continue;
            }
            b'(' | b'[' | b'{' => {
                if stack.is_empty() {
                    out.groups.push(Group {
                        open: i,
                        close: 0,
                        commas: Vec::new(),
                        comprehension: false,
                        attached: i > 0
                            && (word(b[i - 1]) || matches!(b[i - 1], b')' | b']' | b'}')),
                    });
                }
                stack.push(b[i]);
            }
            b')' | b']' | b'}' => {
                let open = stack.pop()?;
                if !matches!((open, b[i]), (b'(', b')') | (b'[', b']') | (b'{', b'}')) {
                    return None;
                }
                if stack.is_empty() {
                    out.groups.last_mut()?.close = i;
                }
            }
            b',' if stack.len() == 1 => out.groups.last_mut()?.commas.push(i),
            b'f' if stack.len() == 1
                && s[i..].starts_with("for ")
                && (i == 0 || !word(b[i - 1])) =>
            {
                out.groups.last_mut()?.comprehension = true;
            }
            b'.' if stack.is_empty()
                && i > 0
                && b[i - 1] != b'.'
                && b.get(i + 1)
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_') =>
            {
                let mut j = i + 1;
                while j < b.len() && word(b[j]) {
                    j += 1;
                }
                if b.get(j) == Some(&b'(') {
                    out.calls.push(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    stack.is_empty().then_some(out)
}

/// `s` as lines of (indent relative to its own, text), none over `room` columns where a bracket
/// or, when `chains`, a method chain allows breaking. `step` is the continuation indent.
pub fn wrap(s: &str, room: usize, step: usize, chains: bool) -> Vec<(usize, String)> {
    let plain = || vec![(0, s.to_owned())];
    if cols(s) <= room || s.contains('\n') {
        return plain();
    }
    let Some(scan) = scan(s) else {
        return plain();
    };
    if chains && scan.calls.len() >= 3 {
        let mut cuts = scan.calls.clone();
        cuts.push(s.len());
        let mut out = wrap(&s[..cuts[0]], room, step, chains);
        for w in cuts.windows(2) {
            out.extend(
                wrap(&s[w[0]..w[1]], room.saturating_sub(step), step, chains)
                    .into_iter()
                    .map(|(n, t)| (n + step, t)),
            );
        }
        return out;
    }
    let groups: Vec<&Group> = scan
        .groups
        .iter()
        .filter(|g| !s[g.open + 1..g.close].trim().is_empty())
        .collect();
    groups
        .iter()
        .map(|g| break_group(s, g, room, step, chains))
        .min_by_key(|out| {
            let over = out.iter().filter(|(n, t)| n + cols(t) > room).count();
            (over, out.len())
        })
        .unwrap_or_else(plain)
}

/// `s` with the group `g` broken, one element per line.
fn break_group(s: &str, g: &Group, room: usize, step: usize, chains: bool) -> Vec<(usize, String)> {
    let mut bounds = vec![g.open];
    bounds.extend(&g.commas);
    bounds.push(g.close);
    let trailing_comma = !g.comprehension
        && match s.as_bytes()[g.open] {
            b'(' => g.attached || !g.commas.is_empty(),
            b'[' => !g.attached,
            _ => true,
        };
    let last = bounds.len() - 2;
    let mut out = vec![(0, s[..=g.open].to_owned())];
    for (k, w) in bounds.windows(2).enumerate() {
        let elem = s[w[0] + 1..w[1]].trim();
        if elem.is_empty() {
            continue;
        }
        let comma = if k == last && !trailing_comma {
            ""
        } else {
            ","
        };
        let text = format!("{elem}{comma}");
        out.extend(
            wrap(&text, room.saturating_sub(step), step, chains)
                .into_iter()
                .map(|(n, t)| (n + step, t)),
        );
    }
    out.extend(wrap(&s[g.close..], room, step, chains));
    out
}

#[cfg(test)]
mod tests {
    use super::wrap;

    fn lines(s: &str, room: usize) -> Vec<String> {
        wrap(s, room, 4, true)
            .into_iter()
            .map(|(n, t)| format!("{}{t}", " ".repeat(n)))
            .collect()
    }

    #[test]
    fn short_lines_stay() {
        assert_eq!(lines("f(a, b)", 20), ["f(a, b)"]);
    }

    #[test]
    fn call_breaks_one_argument_per_line() {
        assert_eq!(
            lines("let x = f(alpha, beta, gamma)", 20),
            ["let x = f(", "    alpha,", "    beta,", "    gamma,", ")"]
        );
    }

    #[test]
    fn grouping_takes_no_comma_and_subscript_none() {
        assert_eq!(
            lines("let x = (aaaaaaaaaa + bbbbbbbbbbb)", 20),
            ["let x = (", "    aaaaaaaaaa + bbbbbbbbbbb", ")"]
        );
        assert_eq!(
            lines("let x = xs[aaaaaaaaaa, bbbbbbbbbbb]", 20),
            ["let x = xs[", "    aaaaaaaaaa,", "    bbbbbbbbbbb", "]"]
        );
    }

    #[test]
    fn comprehension_takes_no_comma() {
        assert_eq!(
            lines("let x = sum(aaaaaaaa for aaaaaaaa in bbbbbbbbb)", 20),
            [
                "let x = sum(",
                "    aaaaaaaa for aaaaaaaa in bbbbbbbbb",
                ")"
            ]
        );
    }

    #[test]
    fn chains_break_before_each_call() {
        assert_eq!(
            lines("xs.map(f).filter(g).collect()", 20),
            ["xs", "    .map(f)", "    .filter(g)", "    .collect()"]
        );
    }

    #[test]
    fn strings_are_atoms() {
        assert_eq!(lines("f(\"a, b(\")", 5), ["f(", "    \"a, b(\",", ")"]);
    }
}
