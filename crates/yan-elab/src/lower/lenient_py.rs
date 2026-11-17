//! N55 and N56: Python's `str.format`, `%` formatting and `input()` read as the Yan spelling, and
//! the hints for the Python names that have no Yan expression (`type`, `open`, `isinstance`,
//! `sys.getsizeof`).

use super::*;

const NO_TYPE: &str = "Yan has no runtime type values: match a sum with `case Variant():`, or \
                       use `info(T)` at compile time";
const NO_OPEN: &str = "Yan has no file handles: `fs.read(path)` returns the text and fails with \
                       `FsError`, in a function that declares `uses fs`";
const NO_ISINSTANCE: &str = "Yan has no type test expression: `match x:` with `case Variant():` \
                             on a sum, or `x is None` on an option";
const NO_SIZEOF: &str = "Yan has no deep size: `size_of(T)` is the layout size (RFC 0083 D3)";

fn hint(n: &str) -> Option<&'static str> {
    Some(match n {
        "type" => NO_TYPE,
        "open" => NO_OPEN,
        "isinstance" => NO_ISINSTANCE,
        "getsizeof" => NO_SIZEOF,
        _ => return None,
    })
}

fn ex(kind: j::ExprKind, at: Span) -> j::Expr {
    j::Expr {
        kind,
        span: at,
        paren: false,
    }
}

fn str_lit(e: &j::Expr) -> Option<&str> {
    match &e.kind {
        j::ExprKind::Lit(j::Lit::Str { prefix, value }) if prefix.is_empty() => Some(value),
        _ => None,
    }
}

/// Whether `e` is a `str` literal formatted by N55, which lowers to an f-string with specs.
pub(crate) fn is_formatted_template(e: &j::Expr) -> bool {
    match &e.kind {
        j::ExprKind::Call(f, _) => {
            matches!(&f.kind, j::ExprKind::Attr(r, m, _) if m == "format" && str_lit(r).is_some())
        }
        j::ExprKind::Binary("%", l, _) => str_lit(l).is_some(),
        _ => false,
    }
}

fn push_lit(pieces: &mut Vec<j::FPiece>, s: &str) {
    match pieces.last_mut() {
        Some(j::FPiece::Lit(l)) => l.push_str(s),
        _ => pieces.push(j::FPiece::Lit(s.to_owned())),
    }
}

/// `"a{}b{0:>5}{n}".format(x, n=y)` as the pieces of an f-string. `None` for what an f-string
/// cannot say: conversions, attribute or index fields, nested fields, a missing argument.
fn format_pieces(
    template: &str,
    pos: &[&j::Expr],
    named: &[(&str, &j::Expr)],
) -> Option<Vec<j::FPiece>> {
    let mut pieces = Vec::new();
    let mut next = 0;
    let mut cs = template.chars().peekable();
    while let Some(c) = cs.next() {
        match c {
            '{' if cs.peek() == Some(&'{') => {
                cs.next();
                push_lit(&mut pieces, "{");
            }
            '}' if cs.peek() == Some(&'}') => {
                cs.next();
                push_lit(&mut pieces, "}");
            }
            '{' => {
                let mut field = String::new();
                loop {
                    match cs.next()? {
                        '}' => break,
                        '{' => return None,
                        c => field.push(c),
                    }
                }
                let (name, spec) = match field.split_once(':') {
                    Some((n, s)) => (n, Some(s)),
                    None => (field.as_str(), None),
                };
                let value = if name.is_empty() {
                    next += 1;
                    pos.get(next - 1)?
                } else if let Ok(i) = name.parse::<usize>() {
                    pos.get(i)?
                } else if name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    named.iter().find(|(n, _)| *n == name).map(|(_, e)| e)?
                } else {
                    return None;
                };
                let spec = spec
                    .map(|s| s.strip_suffix('d').unwrap_or(s).to_owned())
                    .filter(|s| !s.is_empty());
                pieces.push(j::FPiece::Expr((*value).clone(), spec));
            }
            '}' => return None,
            c => push_lit(&mut pieces, &c.to_string()),
        }
    }
    Some(pieces)
}

/// `"%5.2f items: %s" % (a, b)` as the pieces of an f-string. `None` for a mapping key, `*`
/// width, `%r`, `%x`, `%e`, `%g`, `%o`, `%c` or a count that does not match the operands.
fn percent_pieces(template: &str, operands: &[&j::Expr]) -> Option<Vec<j::FPiece>> {
    let mut pieces = Vec::new();
    let mut used = 0;
    let mut cs = template.chars().peekable();
    while let Some(c) = cs.next() {
        if c == '{' || c == '}' {
            return None;
        }
        if c != '%' {
            push_lit(&mut pieces, &c.to_string());
            continue;
        }
        if cs.peek() == Some(&'%') {
            cs.next();
            push_lit(&mut pieces, "%");
            continue;
        }
        let (mut left, mut zero, mut plus) = (false, false, false);
        while let Some(&f) = cs.peek() {
            match f {
                '-' => left = true,
                '0' => zero = true,
                '+' => plus = true,
                _ => break,
            }
            cs.next();
        }
        let mut width = String::new();
        while let Some(&d) = cs.peek().filter(|d| d.is_ascii_digit()) {
            width.push(d);
            cs.next();
        }
        let mut prec = None;
        if cs.peek() == Some(&'.') {
            cs.next();
            let mut p = String::new();
            while let Some(&d) = cs.peek().filter(|d| d.is_ascii_digit()) {
                p.push(d);
                cs.next();
            }
            prec = Some(p);
        }
        let conv = cs.next()?;
        let mut spec = String::new();
        let text = conv == 's';
        if left {
            spec.push('<');
        } else if text && !width.is_empty() {
            spec.push('>');
        }
        if plus {
            spec.push('+');
        }
        if zero && !left {
            spec.push('0');
        }
        spec.push_str(&width);
        match conv {
            'd' | 'i' if prec.is_none() => {}
            's' if prec.is_none() => {}
            'f' => {
                spec.push('.');
                spec.push_str(prec.as_deref().filter(|p| !p.is_empty()).unwrap_or("6"));
                spec.push('f');
            }
            _ => return None,
        }
        pieces.push(j::FPiece::Expr(
            (*operands.get(used)?).clone(),
            Some(spec).filter(|s| !s.is_empty()),
        ));
        used += 1;
    }
    (used == operands.len()).then_some(pieces)
}

impl Lower<'_, '_, '_> {
    /// N55: `"...".format(...)` on a literal as an f-string.
    pub(super) fn lenient_format(
        &mut self,
        recv: &j::Expr,
        m: &str,
        args: &[j::Arg],
        at: Span,
    ) -> Option<j::Expr> {
        if self.cx.strict || m != "format" {
            return None;
        }
        let template = str_lit(recv)?;
        let mut pos = Vec::new();
        let mut named = Vec::new();
        for a in args {
            let j::ArgKind::Expr(e) = &a.kind else {
                return None;
            };
            match &a.name {
                None => pos.push(e),
                Some(n) => named.push((n.as_str(), e)),
            }
        }
        let pieces = format_pieces(template, &pos, &named)?;
        self.lenient("N55", at, "`\"...\".format(...)` normalized to an f-string");
        Some(ex(
            j::ExprKind::FStr {
                template: false,
                pieces,
            },
            at,
        ))
    }

    /// N55: `"..." % x` and `"..." % (a, b)` on a literal as an f-string.
    pub(super) fn lenient_percent(
        &mut self,
        l: &j::Expr,
        r: &j::Expr,
        at: Span,
    ) -> Option<j::Expr> {
        if self.cx.strict {
            return None;
        }
        let template = str_lit(l)?;
        let operands: Vec<&j::Expr> = match &r.kind {
            j::ExprKind::Tuple(xs) => xs.iter().collect(),
            _ => vec![r],
        };
        let pieces = percent_pieces(template, &operands)?;
        self.lenient("N55", at, "`\"...\" % x` normalized to an f-string");
        Some(ex(
            j::ExprKind::FStr {
                template: false,
                pieces,
            },
            at,
        ))
    }

    /// N56: `input()` is `io.read_line()` with the empty string at end of input, which Python
    /// reports as `EOFError`. E200 with a hint for the names Yan has no expression for.
    pub(super) fn lenient_py(
        &mut self,
        n: &str,
        args: &[j::Arg],
        at: Span,
        want: Option<&Ty>,
    ) -> R<Option<(Rhs, Ty)>> {
        if n == "input" && args.is_empty() && !self.cx.strict {
            let read = ex(
                j::ExprKind::Call(
                    Box::new(ex(
                        j::ExprKind::Attr(
                            Box::new(ex(j::ExprKind::Name("io".into()), at)),
                            "read_line".into(),
                            at,
                        ),
                        at,
                    )),
                    Vec::new(),
                ),
                at,
            );
            let empty = ex(
                j::ExprKind::Lit(j::Lit::Str {
                    prefix: String::new(),
                    value: String::new(),
                }),
                at,
            );
            let e = ex(
                j::ExprKind::Fallback(Box::new(read), Box::new(j::Fallback::Value(empty))),
                at,
            );
            self.lenient(
                "N56",
                at,
                "`input()` normalized to `io.read_line() else \"\"`",
            );
            return self.rhs(&e, want).map(Some);
        }
        let msg = match (n, args.len()) {
            ("input", _) => "`input(prompt)` has no Yan form: `io.print(prompt)` then `input()`",
            _ => match hint(n) {
                Some(h) => h,
                None => return Ok(None),
            },
        };
        self.err("E200", at, format!("cannot find function `{n}`: {msg}"));
        Ok(Some((Rhs::Atom(Atom::Lit(Literal::Unit, at)), Ty::Err)))
    }

    /// `sys.getsizeof(x)`: E200 with the `size_of` hint, never a bare unknown module.
    pub(super) fn lenient_sys_call(&mut self, module: &str, m: &str, at: Span) -> bool {
        if module != "sys" || self.locals.contains_key(module) {
            return false;
        }
        let Some(h) = hint(m) else {
            return false;
        };
        self.err("E200", at, format!("`sys.{m}` cannot be found: {h}"));
        true
    }
}
