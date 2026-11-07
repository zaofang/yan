//! Integer match coverage (R606): arm patterns and guards of the form `name <cmp> literal` as interval
//! sets. A guard outside that fragment covers nothing, so it can only cost a missed warning.

use yan_hir as j;

type Set = Vec<(i128, i128)>;

const LO: i128 = i64::MIN as i128;
const HI: i128 = i64::MAX as i128;

fn full() -> Set {
    vec![(LO, HI)]
}

fn normalize(mut s: Set) -> Set {
    s.retain(|&(a, b)| a <= b);
    s.sort_unstable();
    let mut out: Set = Vec::new();
    for (a, b) in s {
        match out.last_mut() {
            Some(l) if a <= l.1 + 1 => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn intersect(a: &Set, b: &Set) -> Set {
    let mut out = Vec::new();
    for &(x, y) in a {
        for &(u, v) in b {
            out.push((x.max(u), y.min(v)));
        }
    }
    normalize(out)
}

fn subset(a: &Set, b: &Set) -> bool {
    a.iter()
        .all(|&(x, y)| b.iter().any(|&(u, v)| u <= x && y <= v))
}

fn int(e: &j::Expr) -> Option<i128> {
    match &e.kind {
        j::ExprKind::Lit(j::Lit::Int(s)) => s.replace('_', "").parse().ok(),
        j::ExprKind::Unary("-", x) => int(x).map(|n| -n),
        _ => None,
    }
}

fn cmp_set(op: &str, k: i128) -> Option<Set> {
    Some(match op {
        "<" => vec![(LO, k - 1)],
        "<=" => vec![(LO, k)],
        ">" => vec![(k + 1, HI)],
        ">=" => vec![(k, HI)],
        "==" => vec![(k, k)],
        "!=" => vec![(LO, k - 1), (k + 1, HI)],
        _ => return None,
    })
}

fn flip(op: &str) -> &str {
    match op {
        "<" => ">",
        "<=" => ">=",
        ">" => "<",
        ">=" => "<=",
        o => o,
    }
}

/// The values for which `g` holds, when `g` compares one of `names` with an integer literal.
fn guard_set(g: &j::Expr, names: &[&str]) -> Option<Set> {
    let j::ExprKind::Compare(op, a, b) = &g.kind else {
        return None;
    };
    let is_name =
        |e: &j::Expr| matches!(&e.kind, j::ExprKind::Name(n) if names.contains(&n.as_str()));
    let (op, k) = match (is_name(a), is_name(b)) {
        (true, false) => (*op, int(b)?),
        (false, true) => (flip(op), int(a)?),
        _ => return None,
    };
    cmp_set(op, k).map(normalize)
}

/// What an integer `match` covers: for each arm whether it can match no value left by the arms before it,
/// and whether the arms together cover every `int`. `None` when a pattern is outside literals and names.
pub fn int_match(
    scrutinee: &j::Expr,
    cases: &[j::Case],
    is_variant: impl Fn(&str) -> bool,
) -> Option<(Vec<bool>, bool)> {
    let mut names: Vec<&str> = Vec::new();
    if let j::ExprKind::Name(n) = &scrutinee.kind {
        names.push(n);
    }
    let mut covered: Set = Vec::new();
    let mut dead = Vec::new();
    for c in cases {
        let mut pat = &c.pat;
        let mut local = names.clone();
        while let j::Pat::As(inner, n) = pat {
            local.push(n);
            pat = inner;
        }
        let set = match pat {
            j::Pat::Wild => full(),
            j::Pat::Path { path, args: None } if path.len() == 1 && !is_variant(&path[0]) => {
                local.push(&path[0]);
                full()
            }
            j::Pat::Lit(e) => vec![(int(e)?, int(e)?)],
            _ => return None,
        };
        let (possible, exact) = match c.guard.as_ref() {
            None => (set, true),
            Some(g) => match guard_set(g, &local) {
                Some(gs) => (intersect(&set, &gs), true),
                None => (set, false),
            },
        };
        dead.push(subset(&possible, &covered));
        if exact {
            covered = normalize([covered, possible].concat());
        }
    }
    Some((dead, subset(&full(), &covered)))
}
