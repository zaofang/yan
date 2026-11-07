//! `cost` bounds (R610). A `steps` bound is refuted (E618) by running the function on concrete
//! inputs of growing size with its run-time counter on: a counter panic is a witness. Probing only
//! refutes, so every bound it does not refute is W851 and stays a run-time counter. `alloc` bounds
//! are W851 until allocations are counted.

use yan_core::ast as y;
use yan_diag::{Severity, Span};
use yan_hir as j;
use yan_interp::{Limits, Stop};

use crate::{Cx, Globals, Ty, bound_text, lower};

/// The sizes each probed parameter takes: its value, length or element count.
const SIZES: [i128; 10] = [0, 1, 2, 3, 5, 8, 13, 21, 34, 55];
/// More parameter combinations than this probe only the diagonal, where every size is equal.
const MAX_INPUTS: usize = 128;
/// Steps per probe run; past it the function is left unrefuted.
const RUN_STEPS: u64 = 2_000_000;

pub(crate) fn check(g: &Globals, m: &j::Module, items: &[y::Item], cx: &mut Cx) {
    if cx.diags.iter().any(|d| d.severity == Severity::Error) {
        return;
    }
    for it in &m.items {
        let fs: Vec<&j::FuncDef> = match &it.kind {
            j::ItemKind::Func(f) => vec![f],
            j::ItemKind::Impl(i) => i.fns.iter().collect(),
            _ => continue,
        };
        for f in fs {
            let Some((steps, alloc)) =
                f.body
                    .iter()
                    .flat_map(|b| &b.contracts)
                    .find_map(|c| match c {
                        j::Contract::Cost { steps, alloc } => Some((steps, alloc)),
                        _ => None,
                    })
            else {
                continue;
            };
            let top = matches!(it.kind, j::ItemKind::Func(_));
            if let Some(e) = steps {
                let e = &spanned(e, cx.src);
                // Probing runs code at compile time, which an untrusted repository may not (10.4).
                let refuted = top && !cx.untrusted && refute(g, f, e, items, cx);
                if !refuted {
                    let text = bound_text(cx.src, e.span).unwrap_or_default();
                    cx.warn(
                        "W851",
                        e.span,
                        &f.name,
                        format!(
                            "`cost steps <= {text}` is neither proven nor refuted; it compiles to a step counter that panics with kind `contract` when exceeded"
                        ),
                    );
                }
            }
            if let Some(e) = alloc {
                let e = &spanned(e, cx.src);
                let text = bound_text(cx.src, e.span).unwrap_or_default();
                cx.warn(
                    "W851",
                    e.span,
                    &f.name,
                    format!(
                        "`cost alloc <= {text}` is neither proven nor refuted; this toolchain does not count allocations yet"
                    ),
                );
            }
        }
    }
}

/// One probe input: the argument expressions and the witness text and JSON.
struct Input {
    args: Vec<j::Expr>,
    shown: Vec<String>,
    json: serde_json::Map<String, serde_json::Value>,
}

/// Whether a probe input makes `f` exceed `cost steps <= e`; reports E618 if one does.
fn refute(g: &Globals, f: &j::FuncDef, e: &j::Expr, items: &[y::Item], cx: &mut Cx) -> bool {
    let Some(sig) = g.fns.get(&f.name) else {
        return false;
    };
    if !sig.tparams.is_empty() || !sig.caps.is_empty() || !sig.ghosts.is_empty() {
        return false;
    }
    let Some(kinds) = sig
        .params
        .iter()
        .map(|p| Kind::of(&p.ty))
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let names: Vec<&str> = sig.params.iter().map(|p| p.name.as_str()).collect();
    // Each function's own counter only, so a counter panic is this function's.
    let base: Vec<y::Item> = items
        .iter()
        .cloned()
        .map(|mut it| {
            if let y::ItemKind::Fn(d) = &mut it.kind
                && d.path != [f.name.as_str()]
            {
                d.contracts
                    .retain(|c| !matches!(c, y::Contract::Cost { .. }));
            }
            it
        })
        .collect();
    for sizes in combinations(kinds.len()) {
        let input = input(&kinds, &names, &sizes, cx.yn, e.span);
        let Some(module) = probe(g, f, sig, &input, &base, e.span, cx) else {
            return false;
        };
        let limits = Limits {
            steps: RUN_STEPS,
            ..Limits::default()
        };
        match yan_interp::stage0(&module, PROBE, limits) {
            Err(Stop::Panic(p))
                if p.kind == "contract" && p.fields.iter().any(|(n, _)| n == "steps") =>
            {
                let bound = p.fields.iter().find_map(|(n, o)| match (n.as_str(), o) {
                    ("steps", yan_interp::Operand::Int(b)) => Some(*b),
                    _ => None,
                });
                let text = bound_text(cx.src, e.span).unwrap_or_default();
                let with = input.shown.join(", ");
                let over = bound.map_or("more".into(), |b| format!("more than {b}"));
                let mut message = format!(
                    "`cost steps <= {text}` is refuted: with {with} the body takes {over} steps"
                );
                if kinds.iter().any(|k| matches!(k, Kind::List(_))) {
                    message += "; each list holds len - 1 down to 0";
                }
                cx.err("E618", e.span, &f.name, message);
                if let Some(d) = cx.diags.last_mut() {
                    d.counterexample = Some(serde_json::Value::Object(input.json));
                }
                return true;
            }
            // Out of budget at this size: larger sizes cost more.
            Err(Stop::Limit(_)) => return false,
            // A `requires` the input breaks, another panic or a failure: not a cost witness.
            _ => {}
        }
    }
    false
}

const PROBE: &str = "__cost_probe";

/// The module with `fn __cost_probe` that calls `f` on `input`, or none when the probe does not
/// lower.
fn probe(
    g: &Globals,
    f: &j::FuncDef,
    sig: &crate::Sig,
    input: &Input,
    base: &[y::Item],
    at: Span,
    cx: &mut Cx,
) -> Option<y::Module> {
    let call = j::Expr {
        kind: j::ExprKind::Call(
            Box::new(expr(j::ExprKind::Name(f.name.clone()), at)),
            input
                .args
                .iter()
                .map(|a| j::Arg {
                    name: None,
                    kind: j::ArgKind::Expr(a.clone()),
                    span: at,
                })
                .collect(),
        ),
        span: at,
        paren: false,
    };
    let call = if sig.fail.is_some() {
        expr(j::ExprKind::Try(Box::new(call)), at)
    } else {
        call
    };
    let ret = f.ret.clone().map(|r| j::RetType {
        from: Vec::new(),
        ..r
    });
    let stmt = if matches!(sig.ret, Ty::Unit) {
        j::StmtKind::Expr(call)
    } else {
        j::StmtKind::Return(Some(call))
    };
    let p = j::FuncDef {
        name: PROBE.into(),
        name_span: at,
        tparams: Vec::new(),
        params: Vec::new(),
        ret,
        uses: None,
        body: Some(j::Body {
            doc: None,
            contracts: Vec::new(),
            stmts: vec![j::Stmt {
                kind: stmt,
                span: at,
            }],
        }),
    };
    let start = cx.diags.len();
    let kept = cx.spelling();
    let lowered = crate::sig(g, &p, cx).and_then(|s| lower::function(g, &p, &s, false, cx));
    let clean = cx.diags[start..]
        .iter()
        .all(|d| d.severity != Severity::Error);
    cx.diags.truncate(start);
    cx.restore_spelling(kept);
    let fd = lowered.ok().filter(|_| clean)?;
    let mut items = base.to_vec();
    items.extend(g.generated.take());
    items.push(y::Item {
        attrs: Vec::new(),
        kind: y::ItemKind::Fn(fd),
        span: at,
    });
    Some(y::Module {
        header: Vec::new(),
        path: vec!["cost".into()],
        items,
    })
}

/// The parameter types a probe can build values of.
#[derive(Clone, Copy)]
enum Kind {
    Int,
    Bool,
    Str,
    List(Elem),
}

#[derive(Clone, Copy)]
enum Elem {
    Int,
    Bool,
    Str,
}

impl Kind {
    fn of(t: &Ty) -> Option<Kind> {
        Some(match t {
            Ty::Int => Kind::Int,
            Ty::Bool => Kind::Bool,
            Ty::Str => Kind::Str,
            Ty::List(e) => Kind::List(match **e {
                Ty::Int => Elem::Int,
                Ty::Bool => Elem::Bool,
                Ty::Str => Elem::Str,
                _ => return None,
            }),
            _ => return None,
        })
    }
}

/// The size of each parameter for every probe input, smallest first: every combination when
/// there are few enough, else the diagonal.
fn combinations(n: usize) -> Vec<Vec<i128>> {
    if SIZES
        .len()
        .checked_pow(n as u32)
        .is_none_or(|k| k > MAX_INPUTS)
    {
        return SIZES.iter().map(|s| vec![*s; n]).collect();
    }
    let mut out = vec![Vec::new()];
    for _ in 0..n {
        out = out
            .into_iter()
            .flat_map(|v| {
                SIZES.iter().map(move |s| {
                    let mut v = v.clone();
                    v.push(*s);
                    v
                })
            })
            .collect();
    }
    out.sort_by_key(|v| (v.iter().copied().max(), v.clone()));
    out
}

fn input(kinds: &[Kind], names: &[&str], sizes: &[i128], yn: bool, at: Span) -> Input {
    use serde_json::Value as J;
    let mut input = Input {
        args: Vec::new(),
        shown: Vec::new(),
        json: serde_json::Map::new(),
    };
    for ((k, n), s) in kinds.iter().zip(names).zip(sizes) {
        let (arg, shown, key, value) = match k {
            Kind::Int => (
                int(*s, at),
                format!("{n} = {s}"),
                (*n).to_owned(),
                J::from(*s as i64),
            ),
            Kind::Bool => {
                let b = s % 2 == 1;
                let word = match (b, yn) {
                    (true, false) => "True",
                    (false, false) => "False",
                    (true, true) => "true",
                    (false, true) => "false",
                };
                let shown = format!("{n} = {word}");
                (
                    expr(j::ExprKind::Lit(j::Lit::Bool(b)), at),
                    shown,
                    (*n).to_owned(),
                    J::Bool(b),
                )
            }
            Kind::Str => (
                str_lit(&"a".repeat(*s as usize), at),
                format!("len({n}) = {s}"),
                format!("len({n})"),
                J::from(*s as i64),
            ),
            Kind::List(e) => {
                let xs = (0..*s)
                    .rev()
                    .map(|v| match e {
                        Elem::Int => int(v, at),
                        Elem::Bool => expr(j::ExprKind::Lit(j::Lit::Bool(v % 2 == 1)), at),
                        Elem::Str => str_lit(&"a".repeat(v as usize), at),
                    })
                    .collect();
                (
                    expr(j::ExprKind::List(xs), at),
                    format!("len({n}) = {s}"),
                    format!("len({n})"),
                    J::from(*s as i64),
                )
            }
        };
        input.args.push(arg);
        input.shown.push(shown);
        input.json.insert(key, value);
    }
    input
}

fn expr(kind: j::ExprKind, span: Span) -> j::Expr {
    j::Expr {
        kind,
        span,
        paren: false,
    }
}

fn int(v: i128, at: Span) -> j::Expr {
    expr(j::ExprKind::Lit(j::Lit::Int(v.to_string())), at)
}

fn str_lit(s: &str, at: Span) -> j::Expr {
    expr(
        j::ExprKind::Lit(j::Lit::Str {
            prefix: String::new(),
            value: s.to_owned(),
        }),
        at,
    )
}

/// `e` with its span widened to the bound as written, so diagnostics quote all of it.
fn spanned(e: &j::Expr, src: &str) -> j::Expr {
    let n = bound_text(src, e.span).map_or(0, |t| t.chars().count() as u32);
    j::Expr {
        span: Span {
            end_line: e.span.line,
            end_col: e.span.col + n.max(1),
            ..e.span
        },
        ..e.clone()
    }
}
