//! HIR to Yong elaboration (14.1, 22.3), for Jian and later Wen: resolve names, check
//! signatures, infer bodies bidirectionally, pass capabilities and lower to Yong IR in
//! A-normal form.
//!
//! The Yong tree keeps Jian spans, so the Yong checker reports at Jian positions.

mod lower;
mod ty;

use std::collections::HashMap;

use yong_core::ast as y;
use yong_diag::{Diagnostic, Span};
use yong_hir as j;

pub use ty::Ty;

/// How the module is checked.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// `--strict`: the lenient reader is off (14.4).
    pub strict: bool,
    /// `--level release` (8.5).
    pub release: bool,
}

/// A construct the elaborator does not lower yet; the caller skips the semantic checks.
#[derive(Debug)]
pub struct Unsupported(pub String);

pub(crate) type R<T> = Result<T, Unsupported>;

pub(crate) fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(Unsupported(what.into()))
}

pub(crate) struct Field {
    pub name: String,
    pub ty: Ty,
    pub default: Option<j::Expr>,
}

pub(crate) struct P {
    pub name: String,
    pub conv: y::Conv,
    pub ty: Ty,
    pub default: Option<j::Expr>,
    pub positional: bool,
    pub span: Span,
}

pub(crate) struct Sig {
    pub params: Vec<P>,
    pub ret: Ty,
    pub fail: Option<Ty>,
    pub caps: Vec<String>,
}

#[derive(Default)]
pub(crate) struct Globals {
    pub records: HashMap<String, Vec<Field>>,
    pub sums: HashMap<String, Vec<(String, Vec<Field>)>>,
    pub variant_of: HashMap<String, String>,
    pub aliases: HashMap<String, j::Type>,
    pub fns: HashMap<String, Sig>,
    pub consts: HashMap<String, Ty>,
    /// Functions called from a `test_` function (E601).
    pub tested: std::collections::HashSet<String>,
}

pub(crate) struct Cx<'s> {
    pub file: &'s str,
    pub src: &'s str,
    pub strict: bool,
    pub release: bool,
    pub diags: Vec<Diagnostic>,
}

impl Cx<'_> {
    pub fn note(&mut self, code: &str, span: Span, item: &str, message: String) {
        self.err(code, span, item, message);
        if let Some(d) = self.diags.last_mut() {
            d.severity = yong_diag::Severity::Note;
        }
    }

    pub fn warn(&mut self, code: &str, span: Span, item: &str, message: String) {
        self.err(code, span, item, message);
        if let Some(d) = self.diags.last_mut() {
            d.severity = yong_diag::Severity::Warning;
        }
    }

    pub fn err(&mut self, code: &str, span: Span, item: &str, message: String) {
        let line = self
            .src
            .lines()
            .nth(span.line.saturating_sub(1) as usize)
            .unwrap_or("");
        let from = (span.col.saturating_sub(1) as usize).min(line.len());
        let to = if span.end_line == span.line {
            (span.end_col.saturating_sub(1) as usize).clamp(from, line.len())
        } else {
            line.len()
        };
        let quote = line.get(from..to).unwrap_or("");
        self.diags
            .push(Diagnostic::error(code, self.file, span, quote, message).with_item(item));
    }
}

/// The capability trait a `uses` name stands for: `io` is `Io`, `user_store` is `UserStore`.
pub(crate) fn cap_type(name: &str) -> String {
    name.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect()
}

impl Globals {
    /// Resolve a written type; reports E200 for an unknown name.
    pub fn ty(&self, t: &j::Type, cx: &mut Cx, span: Span) -> R<Ty> {
        Ok(match t {
            j::Type::None => Ty::Unit,
            j::Type::Optional(t) => Ty::Option(Box::new(self.ty(t, cx, span)?)),
            j::Type::Callable { .. } => return unsupported("callable types"),
            j::Type::Named { path, args } => {
                let mut a = Vec::new();
                for arg in args {
                    match arg {
                        j::TArg::Type(t) => a.push(self.ty(t, cx, span)?),
                        _ => return unsupported("value or union type arguments"),
                    }
                }
                let name = path.join(".");
                match (name.as_str(), a.as_slice()) {
                    ("int", []) => Ty::Int,
                    ("float", []) => Ty::Float,
                    ("bigint", []) => Ty::BigInt,
                    ("bool", []) => Ty::Bool,
                    ("str", []) => Ty::Str,
                    ("char", []) => Ty::Char,
                    ("int64", []) => Ty::Int,
                    ("float64", []) => Ty::Float,
                    (n, []) if ty::fixed(n).is_some() => Ty::Fixed(ty::fixed(n).expect("matched")),
                    ("list", [t]) => Ty::List(Box::new(t.clone())),
                    ("set", [t]) => Ty::Set(Box::new(t.clone())),
                    ("dict", [k, v]) => Ty::Dict(Box::new(k.clone()), Box::new(v.clone())),
                    ("tuple", ts) => Ty::Tuple(ts.to_vec()),
                    ("Result", [t, e]) => Ty::Result(Box::new(t.clone()), Box::new(e.clone())),
                    (n, []) if self.records.contains_key(n) || self.sums.contains_key(n) => {
                        Ty::Data(n.to_owned())
                    }
                    (n, []) if self.aliases.contains_key(n) => {
                        let t = self.aliases[n].clone();
                        self.ty(&t, cx, span)?
                    }
                    (n, _)
                        if n.contains('.') || n.chars().next().is_some_and(char::is_uppercase) =>
                    {
                        return unsupported(format!("type `{n}`"));
                    }
                    (n, _) => {
                        cx.err("E200", span, "", format!("cannot find type `{n}`"));
                        Ty::Err
                    }
                }
            }
        })
    }
}

/// Elaborate a parsed Jian module. Returns the Yong module and the elaboration diagnostics;
/// when none is an error, the module is ready for `yong_check::check`.
pub fn elaborate(
    file: &str,
    src: &str,
    m: &j::Module,
    opts: Options,
) -> Result<(y::Module, Vec<Diagnostic>), Unsupported> {
    let mut cx = Cx {
        file,
        src,
        strict: opts.strict,
        release: opts.release,
        diags: Vec::new(),
    };
    let mut g = Globals::default();
    collect(&mut g, m, &mut cx)?;
    let mut items = Vec::new();
    for it in &m.items {
        if let Some(k) = item(&g, it, &mut cx)? {
            items.push(y::Item {
                attrs: Vec::new(),
                kind: k,
                span: it.span,
            });
        }
    }
    let stem = std::path::Path::new(file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main")
        .to_owned();
    Ok((
        y::Module {
            header: Vec::new(),
            path: vec![stem],
            items,
        },
        cx.diags,
    ))
}

fn collect(g: &mut Globals, m: &j::Module, cx: &mut Cx) -> R<()> {
    for it in &m.items {
        match &it.kind {
            j::ItemKind::Type(t) if !t.tparams.is_empty() => return unsupported("generic types"),
            j::ItemKind::Type(t) => match &t.rhs {
                j::TypeRhs::Sum(vs) => {
                    for v in vs {
                        g.variant_of.insert(v.name.clone(), t.name.clone());
                    }
                    g.sums.insert(t.name.clone(), Vec::new());
                }
                j::TypeRhs::Alias(ts) if ts.len() == 1 => {
                    g.aliases.insert(t.name.clone(), ts[0].clone());
                }
                _ => return unsupported("union aliases, newtypes and capability sets"),
            },
            j::ItemKind::Class(c) if !c.tparams.is_empty() => {
                return unsupported("generic classes");
            }
            j::ItemKind::Class(c) => {
                g.records.insert(c.name.clone(), Vec::new());
            }
            j::ItemKind::Trait(_) | j::ItemKind::Impl(_) => return unsupported("traits"),
            _ => {}
        }
    }
    let fields = |g: &Globals, fs: &[j::Field], cx: &mut Cx, at: Span| -> R<Vec<Field>> {
        fs.iter()
            .map(|f| {
                Ok(Field {
                    name: f.name.clone(),
                    ty: g.ty(&f.ty, cx, at)?,
                    default: f.default.clone(),
                })
            })
            .collect()
    };
    for it in &m.items {
        match &it.kind {
            j::ItemKind::Class(c) => {
                let fs = fields(g, &c.fields, cx, it.span)?;
                g.records.insert(c.name.clone(), fs);
            }
            j::ItemKind::Type(j::TypeDef {
                name,
                rhs: j::TypeRhs::Sum(vs),
                ..
            }) => {
                let mut out = Vec::new();
                for v in vs {
                    out.push((v.name.clone(), fields(g, &v.fields, cx, it.span)?));
                }
                g.sums.insert(name.clone(), out);
            }
            _ => {}
        }
    }
    for it in &m.items {
        match &it.kind {
            j::ItemKind::Func(f) if g.fns.contains_key(&f.name) => {
                cx.err(
                    "E250",
                    f.name_span,
                    &f.name,
                    format!("`{}` is defined twice", f.name),
                );
            }
            j::ItemKind::Func(f) => {
                let s = sig(g, f, cx)?;
                g.fns.insert(f.name.clone(), s);
                if f.name.starts_with("test_")
                    && let Some(b) = &f.body
                {
                    lower::callees(&b.stmts, &mut g.tested);
                }
            }
            j::ItemKind::Const { name, ty, .. } => {
                let t = g.ty(ty, cx, it.span)?;
                g.consts.insert(name.clone(), t);
            }
            _ => {}
        }
    }
    Ok(())
}

fn sig(g: &Globals, f: &j::FuncDef, cx: &mut Cx) -> R<Sig> {
    if !f.tparams.is_empty() {
        return unsupported("generic functions");
    }
    let mut params = Vec::new();
    let mut positional = f.params.iter().any(|p| matches!(p, j::Param::Slash));
    for p in &f.params {
        match p {
            j::Param::Slash => positional = false,
            j::Param::Pack { .. } => return unsupported("argument packs"),
            j::Param::Value {
                conv,
                name,
                span,
                ty,
                default,
            } => params.push(P {
                name: name.clone(),
                conv: match conv {
                    None => y::Conv::Let,
                    Some(j::Conv::Var) => y::Conv::Inout,
                    Some(j::Conv::Take) => y::Conv::Sink,
                    Some(j::Conv::Ghost) => return unsupported("ghost parameters"),
                },
                ty: g.ty(ty, cx, *span)?,
                default: default.clone(),
                positional,
                span: *span,
            }),
        }
    }
    let (ret, fail) = match &f.ret {
        None => (Ty::Unit, None),
        Some(r) => {
            if !r.from.is_empty() {
                return unsupported("views");
            }
            let ret = match &r.ty {
                Some(t) => g.ty(t, cx, f.name_span)?,
                None => Ty::Unit,
            };
            let fail = match r.fail.as_slice() {
                [] => None,
                [p] if p.len() == 1 => Some(g.ty(
                    &j::Type::Named {
                        path: p.clone(),
                        args: Vec::new(),
                    },
                    cx,
                    f.name_span,
                )?),
                _ => return unsupported("error unions"),
            };
            (ret, fail)
        }
    };
    let caps = match &f.uses {
        None | Some(j::Uses::None) => Vec::new(),
        Some(j::Uses::Caps(cs)) => {
            let mut out = Vec::new();
            for c in cs {
                if c.alias.is_some() || c.path.len() != 1 {
                    return unsupported("renamed or qualified capabilities");
                }
                out.push(c.path[0].clone());
            }
            out
        }
    };
    Ok(Sig {
        params,
        ret,
        fail,
        caps,
    })
}

fn const_val(e: &j::Expr) -> R<y::ConstVal> {
    Ok(y::ConstVal::Lit(match &e.kind {
        j::ExprKind::Lit(l) => lower::literal(l)?,
        j::ExprKind::Unary("-", x) => match &x.kind {
            j::ExprKind::Lit(j::Lit::Int(s)) => y::Literal::Int(format!("-{s}")),
            j::ExprKind::Lit(j::Lit::Float(s)) => y::Literal::Float(format!("-{s}")),
            _ => return unsupported("non-literal defaults and constants"),
        },
        _ => return unsupported("non-literal defaults and constants"),
    }))
}

fn core_fields(fs: &[Field]) -> R<Vec<y::Field>> {
    fs.iter()
        .map(|f| {
            Ok(y::Field {
                is_pub: true,
                name: f.name.clone(),
                ty: f.ty.core(),
                default: f.default.as_ref().map(const_val).transpose()?,
            })
        })
        .collect()
}

fn item(g: &Globals, it: &j::Item, cx: &mut Cx) -> R<Option<y::ItemKind>> {
    if !it.decorators.is_empty() {
        return unsupported("decorators");
    }
    Ok(Some(match &it.kind {
        j::ItemKind::Class(c) => y::ItemKind::Record(y::Record {
            is_pub: it.is_pub,
            path: vec![c.name.clone()],
            tparams: Vec::new(),
            fields: core_fields(&g.records[&c.name])?,
        }),
        j::ItemKind::Type(t) => match &t.rhs {
            j::TypeRhs::Sum(_) => {
                let mut variants = Vec::new();
                for (name, fs) in &g.sums[&t.name] {
                    variants.push(y::Variant {
                        name: name.clone(),
                        fields: if fs.is_empty() {
                            None
                        } else {
                            Some(core_fields(fs)?)
                        },
                    });
                }
                y::ItemKind::Sum(y::Sum {
                    is_pub: it.is_pub,
                    path: vec![t.name.clone()],
                    tparams: Vec::new(),
                    variants,
                })
            }
            _ => return Ok(None),
        },
        j::ItemKind::Const { name, value, .. } => y::ItemKind::Const(y::Const {
            is_pub: it.is_pub,
            path: vec![name.clone()],
            ty: g.consts[name].core(),
            value: const_val(value)?,
        }),
        j::ItemKind::Lemma(_) => return Ok(None),
        j::ItemKind::Func(f) => {
            if it.is_pub && f.name != "main" {
                let body = f.body.as_ref();
                if body.is_none_or(|b| b.doc.is_none()) {
                    cx.warn(
                        "W830",
                        f.name_span,
                        &f.name,
                        format!("`{}` is pub and has no docstring", f.name),
                    );
                }
                let has_ex = body.is_some_and(|b| {
                    b.contracts
                        .iter()
                        .any(|c| matches!(c, j::Contract::Ex { .. }))
                });
                if cx.release && !has_ex && !g.tested.contains(&f.name) {
                    cx.err(
                        "E601",
                        f.name_span,
                        &f.name,
                        format!("`{}` is pub and has no `ex` line or test", f.name),
                    );
                }
            }
            y::ItemKind::Fn(lower::function(g, f, it.is_pub, cx)?)
        }
        j::ItemKind::Trait(_) | j::ItemKind::Impl(_) => return unsupported("traits"),
    }))
}

#[cfg(test)]
mod selftest;
