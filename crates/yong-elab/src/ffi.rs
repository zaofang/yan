//! Extern blocks, `unsafe`, inline assembly and `@export` (12).

use yong_core::ast as y;
use yong_diag::Span;
use yong_hir as j;

use crate::ty::Ty;
use crate::{Cx, Globals, P, R, Sig, unsupported};

/// The `core.ffi` types beyond the C scalars.
pub(crate) const FOREIGN: [&str; 6] = ["Ptr", "CStr", "CString", "Owned", "View", "MutView"];

/// Functions with returns-twice semantics (12.5, E906).
const RETURNS_TWICE: [&str; 6] = [
    "setjmp",
    "_setjmp",
    "sigsetjmp",
    "savectx",
    "vfork",
    "getcontext",
];

/// A C scalar of `core.ffi` (12.3), on an LP64 target.
pub(crate) fn c_scalar(name: &str) -> Option<Ty> {
    Some(match name {
        "c_char" | "c_schar" => Ty::Fixed("i8"),
        "c_uchar" => Ty::Fixed("u8"),
        "c_short" => Ty::Fixed("i16"),
        "c_ushort" => Ty::Fixed("u16"),
        "c_int" => Ty::Fixed("i32"),
        "c_uint" => Ty::Fixed("u32"),
        "c_long" | "c_longlong" | "c_ssize" | "c_ptrdiff" => Ty::Int,
        "c_ulong" | "c_ulonglong" | "c_size" => Ty::Fixed("u64"),
        "c_float" => Ty::Fixed("f32"),
        "c_double" => Ty::Float,
        "c_void" => Ty::Unit,
        _ => return None,
    })
}

/// A type that may cross an extern signature (12.3, E904).
fn is_c(g: &Globals, t: &Ty) -> bool {
    match t {
        Ty::Int | Ty::Float | Ty::Fixed(_) | Ty::Bool | Ty::Unit | Ty::Err => true,
        Ty::Foreign(..) => true,
        Ty::Fn(_, ps, r) => ps.iter().all(|p| is_c(g, p)) && is_c(g, r),
        Ty::Option(t) => matches!(**t, Ty::Fn(..) | Ty::Foreign("Ptr" | "Owned", _)),
        Ty::Data(n) => g.layout_c.contains(n),
        _ => false,
    }
}

/// A type of the C profile `yong_abi_v1` (12.7). An owned `List` or `str` crosses only when
/// owned: as a `take` parameter or a result (SPEC_QUESTIONS).
fn is_profile(g: &Globals, t: &Ty, owned: bool) -> bool {
    match t {
        Ty::Int | Ty::Float | Ty::Fixed(_) | Ty::Bool | Ty::Char | Ty::Unit | Ty::Err => true,
        Ty::Str | Ty::List(_) => owned,
        Ty::Foreign("View" | "MutView", ts) => ts.iter().all(|t| is_profile(g, t, true)),
        Ty::Option(t) => is_profile(g, t, owned),
        Ty::Tuple(ts) => ts.iter().all(|t| is_profile(g, t, owned)),
        Ty::Data(n) => {
            let fields = g.records.get(n).into_iter().flatten().map(|f| &f.ty);
            let variants = g.sums.get(n).into_iter().flatten();
            let mut all = fields.chain(variants.flat_map(|(_, fs)| fs.iter().map(|f| &f.ty)));
            all.all(|t| is_profile(g, t, true))
        }
        _ => false,
    }
}

/// Registers an extern block's functions as callable and checks its declarations.
pub(crate) fn collect(g: &mut Globals, x: &j::Extern, at: Span, cx: &mut Cx) -> R<()> {
    if !cx.ffi {
        cx.err(
            "E902",
            at,
            &x.lib,
            "extern blocks belong in an ffi module, under the package's `ffi/` directory".into(),
        );
    }
    for xi in &x.items {
        let j::XItem::Fn {
            safe,
            name,
            name_span,
            params,
            variadic,
            ret,
            ..
        } = xi
        else {
            continue;
        };
        if RETURNS_TWICE.contains(&name.as_str()) {
            cx.err(
                "E906",
                *name_span,
                name,
                format!("`{name}` returns twice and cannot be declared"),
            );
        }
        let mut ps = Vec::new();
        for p in params {
            let t = g.ty(&p.ty, cx, p.ty_span)?;
            if !is_c(g, &t) {
                cx.err(
                    "E904",
                    p.ty_span,
                    name,
                    format!("`{t}` is not a C type; use `@layout(c)` or a `core.ffi` type"),
                );
            }
            ps.push(P {
                name: p.name.clone(),
                conv: if matches!(t, Ty::Foreign("MutView", _)) {
                    y::Conv::Inout
                } else {
                    y::Conv::Let
                },
                ty: t,
                default: None,
                positional: true,
                span: p.ty_span,
            });
        }
        let ret = match ret {
            Some((t, sp)) => {
                let t = g.ty(t, cx, *sp)?;
                if !is_c(g, &t) {
                    cx.err("E904", *sp, name, format!("`{t}` is not a C type"));
                }
                t
            }
            None => Ty::Unit,
        };
        g.externs.insert(name.clone(), (*safe, *variadic));
        g.fns.insert(
            name.clone(),
            Sig {
                tparams: Vec::new(),
                params: ps,
                ret,
                fail: None,
                caps: Vec::new(),
            },
        );
    }
    Ok(())
}

pub(crate) fn lower(g: &Globals, x: &j::Extern, cx: &mut Cx) -> R<y::Extern> {
    let mut items = Vec::new();
    for xi in &x.items {
        items.push(match xi {
            j::XItem::Fn {
                safe,
                name,
                params,
                variadic,
                uses_none,
                nonblocking,
                ..
            } => {
                let sig = &g.fns[name];
                let params = params
                    .iter()
                    .zip(&sig.params)
                    .map(|(p, s)| match (&s.ty, p.callback) {
                        (Ty::Fn(_, ps, r), Some(c)) => y::XParam::Callback {
                            name: p.name.clone(),
                            escaping: c == j::Callback::Escaping,
                            ty: y::FnType {
                                params: ps.iter().map(|t| (y::Conv::Let, t.core())).collect(),
                                ret: Box::new(r.core()),
                                uses: None,
                                fail: None,
                            },
                        },
                        (t, _) => y::XParam::Value {
                            name: p.name.clone(),
                            ty: t.core(),
                        },
                    })
                    .collect();
                y::XItem::Fn {
                    safe: *safe,
                    name: name.clone(),
                    params,
                    variadic: *variadic,
                    ret: sig.ret.core(),
                    from: Vec::new(),
                    uses_none: *uses_none,
                    nonblocking: *nonblocking,
                }
            }
            j::XItem::Static {
                safe,
                name,
                ty,
                ty_span,
            } => y::XItem::Static {
                safe: *safe,
                name: name.clone(),
                ty: g.ty(ty, cx, *ty_span)?.core(),
            },
            j::XItem::Type { .. } => return unsupported("extern types"),
        });
    }
    Ok(y::Extern {
        abi: x.abi.clone(),
        lib: x.lib.clone(),
        link: x.link.clone(),
        items,
    })
}

/// E907: an `@export` item is `pub` and its signature uses only C profile types.
pub(crate) fn export(g: &Globals, it: &j::Item, cx: &mut Cx) -> R<()> {
    let j::ItemKind::Func(f) = &it.kind else {
        return unsupported("`@export` on an item that is not a function");
    };
    if !it.is_pub {
        cx.err(
            "E907",
            f.name_span,
            &f.name,
            format!("`{}` is exported and must be `pub`", f.name),
        );
    }
    let sig = &g.fns[&f.name];
    for p in &sig.params {
        if !is_profile(g, &p.ty, p.conv == y::Conv::Sink) {
            let at = type_after(cx.src, p.span);
            cx.err(
                "E907",
                at,
                &f.name,
                format!("`{}` is not a C profile type (12.7)", p.ty),
            );
        }
    }
    if !is_profile(g, &sig.ret, true) {
        cx.err(
            "E907",
            f.name_span,
            &f.name,
            format!("the result `{}` is not a C profile type (12.7)", sig.ret),
        );
    }
    Ok(())
}

/// The type written after a parameter's name: `xs: List[int]` from the span of `xs`.
fn type_after(src: &str, name: Span) -> Span {
    let line = src.lines().nth(name.line as usize - 1).unwrap_or("");
    let from = (name.col as usize - 1).min(line.len());
    let Some(colon) = line[from..].find(':') else {
        return name;
    };
    let rest = &line[from + colon + 1..];
    let col = (from + colon + 1 + rest.len() - rest.trim_start().len() + 1) as u32;
    let len = rest
        .trim_start()
        .find([',', ')', '='])
        .unwrap_or(rest.trim_start().len());
    Span {
        line: name.line,
        col,
        end_line: name.line,
        end_col: col + rest.trim_start()[..len].trim_end().chars().count() as u32,
    }
}

/// Register classes and explicit registers of the target (12.9, E911).
pub(crate) fn register_ok(class: bool, name: &str) -> bool {
    let classes: &[&str] = if cfg!(target_arch = "aarch64") {
        &["reg", "vreg", "vreg_low16", "preg"]
    } else {
        &[
            "reg", "reg_abcd", "reg_byte", "xmm_reg", "ymm_reg", "zmm_reg", "kreg",
        ]
    };
    if class {
        return classes.contains(&name);
    }
    if cfg!(target_arch = "aarch64") {
        let num = |p: &str, n: u32| {
            name.strip_prefix(p)
                .and_then(|d| d.parse::<u32>().ok())
                .is_some_and(|d| d < n)
        };
        return ["x", "w"].iter().any(|p| num(p, 31))
            || ["v", "q", "d", "s", "h", "b"].iter().any(|p| num(p, 32))
            || ["sp", "lr", "fp", "xzr", "wzr"].contains(&name);
    }
    let base = [
        "ax", "bx", "cx", "dx", "si", "di", "bp", "sp", "al", "bl", "cl", "dl", "ah", "bh", "ch",
        "dh",
    ];
    let wide = |p: &str| name.strip_prefix(p).is_some_and(|r| base.contains(&r));
    let numbered = |p: &str, ns: std::ops::Range<u32>, suffixes: &[&str]| {
        ns.into_iter()
            .any(|n| suffixes.iter().any(|s| name == format!("{p}{n}{s}")))
    };
    base.contains(&name)
        || wide("e")
        || wide("r")
        || ["sil", "dil", "bpl", "spl"].contains(&name)
        || numbered("r", 8..16, &["", "d", "w", "b"])
        || ["xmm", "ymm", "zmm"]
            .iter()
            .any(|p| numbered(p, 0..32, &[""]))
        || numbered("k", 0..8, &[""])
}
