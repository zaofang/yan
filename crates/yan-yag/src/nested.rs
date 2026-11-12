//! N31 (R661): the lenient reader lifts a `def` nested in a module function's body to module
//! level when it reads no local of the enclosing function. It keeps its name when that name is
//! free at module level and becomes `outer_inner` otherwise, with its uses in the enclosing
//! function renamed. A nested `def` that captures a local is left in place, where the parser
//! rejects it.
//!
//! The rewrite works on tokens, before parsing: the nested definition's tokens, whose INDENT and
//! DEDENT are balanced, move after the enclosing function.

use std::collections::HashSet;
use std::ops::Range;

use yan_diag::Severity;

use crate::lexer::{FPart, Issue, Tok, Token};

const KEYWORDS: [&str; 34] = [
    "and", "as", "assert", "break", "case", "class", "continue", "def", "elif", "else", "fail",
    "False", "for", "from", "if", "impl", "in", "is", "lambda", "match", "None", "not", "or",
    "pass", "return", "self", "take", "trait", "True", "type", "uses", "var", "while", "with",
];

const ASSIGN: [&str; 13] = [
    "=", "+=", "-=", "*=", "/=", "//=", "%=", "**=", "&=", "|=", "^=", "<<=", ">>=",
];

pub(crate) fn lift(toks: &mut Vec<Token>) -> Vec<Issue> {
    let depth = depths(toks);
    let module: HashSet<String> = (0..toks.len())
        .filter(|&i| {
            depth[i] == 0 && (toks[i].line_start || (i > 0 && is_name(&toks[i - 1].tok, "pub")))
        })
        .filter_map(|i| match (&toks[i].tok, toks.get(i + 1).map(|t| &t.tok)) {
            (Tok::Name(k), Some(Tok::Name(n)))
                if matches!(k.as_str(), "def" | "class" | "type") =>
            {
                Some(n.clone())
            }
            (Tok::Name(n), Some(Tok::Op("=" | ":"))) => Some(n.clone()),
            _ => None,
        })
        .collect();
    let mut taken = module.clone();
    let mut issues = Vec::new();
    // (outer range, nested ranges to move), last outer first so earlier indices stay valid.
    let mut plans: Vec<(Range<usize>, Vec<Range<usize>>)> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if !(depth[i] == 0 && starts_def(toks, i)) {
            i += 1;
            continue;
        }
        let outer = i..def_end(toks, &depth, i);
        let Some(Tok::Name(outer_name)) = toks.get(i + 1).map(|t| t.tok.clone()) else {
            i = outer.end;
            continue;
        };
        let nested: Vec<Range<usize>> = {
            let mut out = Vec::new();
            let mut k = header_end(toks, i);
            while k < outer.end {
                if depth[k] > 0 && starts_def(toks, k) {
                    let r = k..def_end(toks, &depth, k);
                    k = r.end;
                    out.push(r);
                } else {
                    k += 1;
                }
            }
            out
        };
        let mut outside: Vec<usize> = outer.clone().collect();
        outside.retain(|k| !nested.iter().any(|r| r.contains(k)));
        let mut outer_bound = params(toks, i);
        outer_bound.extend(binds(toks, &outside));
        let mut moved = Vec::new();
        for r in nested {
            let Tok::Name(inner) = toks[r.start + 1].tok.clone() else {
                continue;
            };
            let all: Vec<usize> = r.clone().collect();
            let mut bound = params(toks, r.start);
            bound.extend(binds(toks, &all));
            bound.insert(inner.clone());
            let mut captured: Vec<String> = reads(toks, &all)
                .into_iter()
                .filter(|n| !bound.contains(n) && outer_bound.contains(n))
                .collect();
            captured.sort();
            captured.dedup();
            if !captured.is_empty() {
                continue;
            }
            let name = if taken.contains(&inner) {
                format!("{outer_name}_{inner}")
            } else {
                inner.clone()
            };
            taken.insert(name.clone());
            let span = toks[r.start].span;
            let message = if name == inner {
                format!("nested `def {inner}` lifted to module level")
            } else {
                for k in outer.clone() {
                    let after_dot = k > 0 && toks[k - 1].tok == Tok::Op(".");
                    match &mut toks[k].tok {
                        Tok::Name(n) if !after_dot && *n == inner => *n = name.clone(),
                        Tok::FStr { parts, .. } => {
                            for p in parts {
                                if let FPart::Expr { src, .. } = p {
                                    *src = rename_in(src, &inner, &name);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                format!("nested `def {inner}` lifted to module level as `{name}`")
            };
            issues.push(Issue {
                code: "N31",
                severity: Severity::Note,
                span,
                message,
                fix: None,
            });
            moved.push(r);
        }
        if !moved.is_empty() {
            plans.push((outer.clone(), moved));
        }
        i = outer.end;
    }
    for (outer, moved) in plans.into_iter().rev() {
        let mut lifted = Vec::new();
        for r in moved.iter().rev() {
            let mut ts: Vec<Token> = toks.drain(r.clone()).collect();
            ts.append(&mut lifted);
            lifted = ts;
        }
        let at = outer.end - moved.iter().map(|r| r.len()).sum::<usize>();
        toks.splice(at..at, lifted);
    }
    issues
}

/// `src` with each whole-word `from`, not after `.`, replaced by `to`.
fn rename_in(src: &str, from: &str, to: &str) -> String {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::new();
    let mut rest = src;
    while let Some(k) = rest.find(from) {
        let before = rest[..k].chars().next_back();
        let after = rest[k + from.len()..].chars().next();
        let whole = !before.is_some_and(|c| word(c) || c == '.') && !after.is_some_and(word);
        out.push_str(&rest[..k]);
        out.push_str(if whole { to } else { from });
        rest = &rest[k + from.len()..];
    }
    out.push_str(rest);
    out
}

/// A `def` that opens its line, or follows a `pub` that does.
fn starts_def(toks: &[Token], i: usize) -> bool {
    is_name(&toks[i].tok, "def")
        && (toks[i].line_start
            || (i > 0 && toks[i - 1].line_start && is_name(&toks[i - 1].tok, "pub")))
}

fn is_name(t: &Tok, s: &str) -> bool {
    matches!(t, Tok::Name(n) if n == s)
}

/// The indentation depth before each token.
fn depths(toks: &[Token]) -> Vec<usize> {
    let mut d = 0usize;
    toks.iter()
        .map(|t| {
            let here = d;
            match t.tok {
                Tok::Indent => d += 1,
                Tok::Dedent => d = d.saturating_sub(1),
                _ => {}
            }
            here
        })
        .collect()
}

/// The index after the NEWLINE that ends the header of the `def` at `i`.
fn header_end(toks: &[Token], i: usize) -> usize {
    (i..toks.len())
        .find(|&k| toks[k].tok == Tok::Newline)
        .map_or(toks.len(), |k| k + 1)
}

/// The end of the definition at `i`: past the DEDENT that closes its body, or past the header
/// line for a one-line body.
fn def_end(toks: &[Token], depth: &[usize], i: usize) -> usize {
    let h = header_end(toks, i);
    if toks.get(h).map(|t| &t.tok) != Some(&Tok::Indent) {
        return h;
    }
    (h + 1..toks.len())
        .find(|&k| toks[k].tok == Tok::Dedent && depth[k] == depth[i] + 1)
        .map_or(toks.len(), |k| k + 1)
}

/// The parameter names of the `def` at `i`: names right after `(` or `,` at bracket depth 1.
fn params(toks: &[Token], i: usize) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut nest = 0;
    for k in i..header_end(toks, i) {
        match &toks[k].tok {
            Tok::Op("(" | "[" | "{") => nest += 1,
            Tok::Op(")" | "]" | "}") => nest -= 1,
            Tok::Name(n) if nest == 1 && !KEYWORDS.contains(&n.as_str()) => {
                let mut j = k - 1;
                while matches!(&toks[j].tok, Tok::Name(w) if w == "var" || w == "take")
                    || toks[j].tok == Tok::Op("*")
                {
                    j -= 1;
                }
                if matches!(toks[j].tok, Tok::Op("(" | ",")) {
                    out.insert(n.clone());
                }
            }
            _ => {}
        }
    }
    out
}

/// Names bound among the tokens at `ix`: assignment targets, annotated names, `for` and `lambda`
/// targets, and names after `as`.
fn binds(toks: &[Token], ix: &[usize]) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut targets = false;
    for (p, &k) in ix.iter().enumerate() {
        let Tok::Name(n) = &toks[k].tok else {
            if matches!(toks[k].tok, Tok::Op(":")) {
                targets = false;
            }
            continue;
        };
        if n == "for" || n == "lambda" {
            targets = true;
            continue;
        }
        if n == "in" {
            targets = false;
            continue;
        }
        if KEYWORDS.contains(&n.as_str()) {
            continue;
        }
        let next = ix.get(p + 1).map(|&j| &toks[j].tok);
        let after_as = p > 0 && is_name(&toks[ix[p - 1]].tok, "as");
        let assigned = matches!(next, Some(Tok::Op(op)) if ASSIGN.contains(op));
        let annotated = toks[k].line_start && next == Some(&Tok::Op(":"));
        if targets || after_as || assigned || annotated {
            out.insert(n.clone());
        }
    }
    out
}

/// Names read among the tokens at `ix`, including inside interpolations.
fn reads(toks: &[Token], ix: &[usize]) -> Vec<String> {
    let mut out = Vec::new();
    for &k in ix {
        match &toks[k].tok {
            Tok::Name(n) if !KEYWORDS.contains(&n.as_str()) => {
                if k == 0 || toks[k - 1].tok != Tok::Op(".") {
                    out.push(n.clone());
                }
            }
            Tok::FStr { parts, .. } => {
                for p in parts {
                    if let FPart::Expr { src, .. } = p {
                        out.extend(
                            src.split(|c: char| !c.is_alphanumeric() && c != '_')
                                .filter(|w| w.starts_with(|c: char| c.is_alphabetic() || c == '_'))
                                .map(str::to_owned),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    out
}
