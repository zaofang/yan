//! HIR to Yong elaboration (14.1, 22.3), for Xin and later Wen: resolve names, check
//! signatures, infer bodies bidirectionally, pass capabilities and lower to Yong IR in
//! A-normal form.
//!
//! The Yong tree keeps Xin spans, so the Yong checker reports at Xin positions.

mod bounds;
mod decorators;
mod ffi;
mod lower;
mod stage0;
mod stdlib;
mod ty;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use yong_core::ast as y;
use yong_diag::{Diagnostic, Span};
use yong_hir as j;

pub use stdlib::{StdModule, install_std};
pub use ty::Ty;

/// How the module is checked.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// `--strict`: the lenient reader is off (14.4).
    pub strict: bool,
    /// `--level release` (8.5).
    pub release: bool,
    /// The module came from Wen, whose lints differ from Xin's (15.3).
    pub wen: bool,
    /// The module is an ffi module, under the package's `ffi/` directory (12).
    pub ffi: bool,
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

#[derive(Clone)]
pub(crate) struct P {
    pub name: String,
    pub conv: y::Conv,
    pub ty: Ty,
    pub default: Option<j::Expr>,
    pub positional: bool,
    pub span: Span,
}

#[derive(Clone)]
pub(crate) struct Sig {
    /// Type parameters and the traits that bound each.
    pub tparams: Vec<(String, Vec<String>)>,
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
    /// `@derive(From)` conversions (R432): (from type, sum) to the generated function.
    pub froms: HashMap<(String, String), String>,
    /// Functions with a stage-0 (`Nat`) parameter, and its name: lowered per instance (10.4).
    pub templates: HashMap<String, (j::FuncDef, String)>,
    pub instances: RefCell<HashMap<String, Sig>>,
    /// The instances lowered so far.
    pub generated: RefCell<Vec<y::Item>>,
    /// Extern functions: whether each is `safe` and variadic (12.1).
    pub externs: HashMap<String, (bool, bool)>,
    /// Records marked `@layout(c)` (12.3).
    pub layout_c: HashSet<String>,
    /// Functions marked `@deprecated`, for W815.
    pub deprecated: HashSet<String>,
    pub consts: HashMap<String, Ty>,
    /// The records that come from std rather than the module.
    pub std_records: std::collections::HashSet<String>,
    /// Functions called from a `test_` function (E601).
    pub tested: HashSet<String>,
    /// Each trait's methods, with `Self` as a type parameter.
    pub traits: HashMap<String, Vec<(String, Sig)>>,
    /// `(trait, type)` for each `impl`, the type as written in Xin.
    pub impls: HashSet<(String, String)>,
    /// `(type, method)` to the method's signature, from every `impl`.
    pub methods: HashMap<(String, String), Sig>,
    /// The type parameters in scope while a signature or body is resolved.
    pub scope: RefCell<Vec<String>>,
    /// What `Self` names inside an `impl`.
    pub self_ty: RefCell<Option<Ty>>,
}

pub(crate) struct Cx<'s> {
    pub file: &'s str,
    pub src: &'s str,
    pub strict: bool,
    pub release: bool,
    pub wen: bool,
    pub ffi: bool,
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
            j::Type::Callable {
                par,
                params,
                ret,
                fail,
                uses,
            } => {
                if !fail.is_empty() || !uses.is_empty() {
                    return unsupported("callable types with `fail` or `uses`");
                }
                let mut ps = Vec::new();
                for p in params {
                    ps.push(self.ty(p, cx, span)?);
                }
                Ty::Fn(*par, ps, Box::new(self.ty(ret, cx, span)?))
            }
            j::Type::Named { path, .. } if cx.ffi && ffi::c_scalar(&path.join(".")).is_some() => {
                ffi::c_scalar(&path[0]).expect("checked")
            }
            j::Type::Named { path, args }
                if cx.ffi && path.len() == 1 && ffi::FOREIGN.contains(&path[0].as_str()) =>
            {
                let n = path[0].as_str();
                let k = ffi::FOREIGN.iter().position(|f| *f == n).expect("checked");
                let mut a = Vec::new();
                for (k, arg) in args.iter().enumerate() {
                    match arg {
                        // `Owned[T, free_fn]` names its free function.
                        j::TArg::Type(j::Type::Named { path, args })
                            if n == "Owned" && k == 1 && args.is_empty() =>
                        {
                            a.push(Ty::Data(path.join(".")))
                        }
                        j::TArg::Type(t) => a.push(self.ty(t, cx, span)?),
                        _ => return unsupported("value type arguments"),
                    }
                }
                Ty::Foreign(ffi::FOREIGN[k], a)
            }
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
                    (n, ts) if ty::collection(n).is_some_and(|(_, k)| k == ts.len()) => {
                        Ty::Coll(ty::collection(n).expect("matched").0, ts.to_vec())
                    }
                    ("Result", [t, e]) => Ty::Result(Box::new(t.clone()), Box::new(e.clone())),
                    (n, []) if self.records.contains_key(n) || self.sums.contains_key(n) => {
                        Ty::Data(n.to_owned())
                    }
                    ("Self", []) if self.self_ty.borrow().is_some() => {
                        self.self_ty.borrow().clone().expect("checked")
                    }
                    (n, []) if self.scope.borrow().iter().any(|p| p == n) => {
                        Ty::Param(n.to_owned())
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

/// Elaborate a parsed Xin module. Returns the Yong module and the elaboration diagnostics;
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
        wen: opts.wen,
        ffi: opts.ffi,
        diags: Vec::new(),
    };
    let mut g = Globals::default();
    let std_classes = stdlib::classes(&mut g, m)?;
    let std_sums = stdlib::sums(&mut g, m);
    collect(&mut g, m, &mut cx)?;
    let std_fns = stdlib::collect(&mut g, m)?;
    let mut items = derive_froms(&mut g, m);
    for name in std_classes {
        items.push(y::Item {
            attrs: Vec::new(),
            kind: y::ItemKind::Record(y::Record {
                is_pub: true,
                path: vec![name.clone()],
                tparams: Vec::new(),
                fields: core_fields(&g.records[&name])?,
            }),
            span: Span {
                line: 1,
                col: 1,
                end_line: 1,
                end_col: 1,
            },
        });
    }
    for name in std_sums {
        let variants = g.sums[&name]
            .iter()
            .map(|(v, _)| y::Variant {
                name: v.clone(),
                fields: None,
            })
            .collect();
        items.push(y::Item {
            attrs: Vec::new(),
            kind: y::ItemKind::Sum(y::Sum {
                is_pub: true,
                path: vec![name],
                tparams: Vec::new(),
                variants,
            }),
            span: Span {
                line: 1,
                col: 1,
                end_line: 1,
                end_col: 1,
            },
        });
    }
    for it in &m.items {
        let start = cx.diags.len();
        let k = item(&g, it, &mut cx)?;
        decorators::check(&g, it, start, &mut cx)?;
        if let Some(k) = k {
            items.push(y::Item {
                attrs: Vec::new(),
                kind: k,
                span: it.span,
            });
        }
    }
    items.extend(stdlib::lower(&g, &std_fns)?);
    items.extend(g.generated.take());
    for it in &m.items {
        if let j::ItemKind::Const {
            name,
            value:
                j::Expr {
                    kind: j::ExprKind::Stage(e),
                    span,
                    ..
                },
            ..
        } = &it.kind
            && let Some(value) = stage0::constant(&g, it, name, e, *span, &items, &mut cx)?
        {
            items.push(y::Item {
                attrs: Vec::new(),
                kind: y::ItemKind::Const(y::Const {
                    is_pub: it.is_pub,
                    path: vec![name.clone()],
                    ty: g.consts[name].core(),
                    value,
                }),
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
                let c_layout = |d: &j::Decorator| {
                    d.path == ["layout"]
                        && d.args.as_ref().is_some_and(|a| {
                            matches!(&a[..], [j::Arg { kind: j::ArgKind::Expr(j::Expr { kind: j::ExprKind::Name(n), .. }), .. }] if n == "c")
                        })
                };
                if it.decorators.iter().any(c_layout) {
                    g.layout_c.insert(c.name.clone());
                }
            }
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
            j::ItemKind::Func(f)
                if f.tparams
                    .iter()
                    .any(|t| matches!(t, j::TParam::Value { .. })) =>
            {
                let names: Vec<&String> = f
                    .tparams
                    .iter()
                    .filter_map(|t| match t {
                        j::TParam::Value { name, .. } => Some(name),
                        j::TParam::Type { .. } => None,
                    })
                    .collect();
                if names.len() != 1 || f.tparams.len() != 1 {
                    return unsupported("more than one stage-0 parameter");
                }
                g.templates
                    .insert(f.name.clone(), (f.clone(), names[0].clone()));
            }
            j::ItemKind::Func(f) => {
                let s = sig(g, f, cx)?;
                g.fns.insert(f.name.clone(), s);
                if it.decorators.iter().any(|d| d.path == ["deprecated"]) {
                    g.deprecated.insert(f.name.clone());
                }
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
            j::ItemKind::Extern(x) => ffi::collect(g, x, it.span, cx)?,
            j::ItemKind::Trait(t) => {
                if !t.tparams.is_empty() || !t.assoc.is_empty() {
                    return unsupported("generic traits and associated types");
                }
                *g.scope.borrow_mut() = vec!["Self".into()];
                let mut ms = Vec::new();
                for (_, f) in &t.sigs {
                    ms.push((f.name.clone(), sig(g, f, cx)?));
                }
                g.scope.borrow_mut().clear();
                g.traits.insert(t.name.clone(), ms);
            }
            _ => {}
        }
    }
    for it in &m.items {
        if let j::ItemKind::Impl(i) = &it.kind {
            let (Some(tr), true, true) = (impl_trait(i), i.args.is_empty(), i.assoc.is_empty())
            else {
                return unsupported("generic impls and associated types");
            };
            let for_ty = g.ty(&i.for_ty, cx, it.span)?;
            let key = for_ty.to_string();
            *g.self_ty.borrow_mut() = Some(for_ty);
            for f in &i.fns {
                let s = sig(g, f, cx)?;
                g.methods.insert((key.clone(), f.name.clone()), s);
            }
            *g.self_ty.borrow_mut() = None;
            if !g.impls.insert((tr.clone(), key.clone())) {
                cx.err(
                    "E220",
                    it.span,
                    &tr,
                    format!("`{key}` already implements `{tr}`"),
                );
            }
        }
    }
    Ok(())
}

fn f_span(t: &j::Trait, n: &str) -> Span {
    t.sigs
        .iter()
        .find(|(_, f)| f.name == n)
        .map(|(_, f)| f.name_span)
        .expect("a method of the trait")
}

fn impl_trait(i: &j::Impl) -> Option<String> {
    match i.trait_.as_slice() {
        [t] => Some(t.clone()),
        _ => None,
    }
}

pub(crate) fn sig(g: &Globals, f: &j::FuncDef, cx: &mut Cx) -> R<Sig> {
    let mut tparams = Vec::new();
    for t in &f.tparams {
        let j::TParam::Type { name, bounds } = t else {
            return unsupported("value type parameters");
        };
        let mut bs = Vec::new();
        for b in bounds {
            match b {
                j::Bound::Path { path, args } if path.len() == 1 && args.is_empty() => {
                    bs.push(path[0].clone())
                }
                _ => return unsupported("generic or `Nat` bounds"),
            }
        }
        tparams.push((name.clone(), bs));
    }
    let outer = g.scope.borrow().len();
    g.scope
        .borrow_mut()
        .extend(tparams.iter().map(|(n, _)| n.clone()));
    let r = sig_body(g, f, tparams, cx);
    g.scope.borrow_mut().truncate(outer);
    r
}

fn sig_body(
    g: &Globals,
    f: &j::FuncDef,
    tparams: Vec<(String, Vec<String>)>,
    cx: &mut Cx,
) -> R<Sig> {
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
        tparams,
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

/// R432: `@derive(From)` on a sum gives a conversion from the field type of each variant with
/// exactly one field of a nominal type, unless two variants share that type.
fn derive_froms(g: &mut Globals, m: &j::Module) -> Vec<y::Item> {
    let mut items = Vec::new();
    for it in &m.items {
        let j::ItemKind::Type(t) = &it.kind else {
            continue;
        };
        let derives_from = it.decorators.iter().any(|d| {
            d.path == ["derive"]
                && d.args.iter().flatten().any(|a| {
                    matches!(&a.kind, j::ArgKind::Expr(j::Expr { kind: j::ExprKind::Name(n), .. })
                        if n == "From")
                })
        });
        let Some(variants) = g.sums.get(&t.name).filter(|_| derives_from) else {
            continue;
        };
        let singles: Vec<(&str, &Field, String)> = variants
            .iter()
            .filter_map(|(v, fs)| match fs.as_slice() {
                [
                    f @ Field {
                        ty: Ty::Data(d), ..
                    },
                ] => Some((v.as_str(), f, d.clone())),
                _ => None,
            })
            .collect();
        let mut made = Vec::new();
        for (v, f, d) in &singles {
            if singles.iter().filter(|(_, _, e)| e == d).count() > 1 {
                continue;
            }
            let name = format!("{}__from_{v}", t.name);
            let at = it.span;
            let sum = Ty::Data(t.name.clone());
            let ctor = y::Rhs::Ctor(y::Ctor::Named {
                path: vec![(*v).to_owned()],
                targs: Vec::new(),
                fields: vec![(f.name.clone(), y::Atom::Name("e".into(), at))],
            });
            let body = vec![
                y::Stmt {
                    kind: y::StmtKind::Let {
                        mutable: false,
                        pat: y::Pat::Bind("r".into()),
                        pat_span: at,
                        ty: sum.core(),
                        rhs: ctor,
                        rhs_span: at,
                    },
                    span: at,
                },
                y::Stmt {
                    kind: y::StmtKind::Ret(y::Atom::Name("r".into(), at)),
                    span: at,
                },
            ];
            items.push(y::Item {
                attrs: Vec::new(),
                kind: y::ItemKind::Fn(y::FnDef {
                    is_pub: false,
                    is_unsafe: false,
                    path: vec![name.clone()],
                    name_span: at,
                    tparams: Vec::new(),
                    params: vec![y::Param::Value {
                        comptime: false,
                        conv: y::Conv::Sink,
                        name: "e".into(),
                        ty: f.ty.core(),
                    }],
                    ret: sum.core(),
                    from: Vec::new(),
                    fail: None,
                    contracts: Vec::new(),
                    body: Some(y::Block { stmts: body }),
                }),
                span: at,
            });
            made.push(((d.clone(), t.name.clone()), name));
        }
        g.froms.extend(made);
    }
    items
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
        j::ItemKind::Const {
            value:
                j::Expr {
                    kind: j::ExprKind::Stage(_),
                    ..
                },
            ..
        } => return Ok(None),
        j::ItemKind::Const { name, value, .. } => y::ItemKind::Const(y::Const {
            is_pub: it.is_pub,
            path: vec![name.clone()],
            ty: g.consts[name].core(),
            value: const_val(value)?,
        }),
        j::ItemKind::Lemma(f) => {
            if matches!(&f.uses, Some(j::Uses::Caps(cs)) if !cs.is_empty()) {
                cx.err(
                    "E605",
                    f.name_span,
                    &f.name,
                    "a lemma uses no capabilities; drop the `uses` clause".into(),
                );
            }
            return Ok(None);
        }
        j::ItemKind::Func(f) if g.templates.contains_key(&f.name) => return Ok(None),
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
                } else if cx.wen && !has_ex {
                    cx.warn(
                        "W813",
                        f.name_span,
                        &f.name,
                        format!("`{}` is pub and has no `ex` line", f.name),
                    );
                }
            }
            y::ItemKind::Fn(lower::function(g, f, &g.fns[&f.name], it.is_pub, cx)?)
        }
        j::ItemKind::Extern(x) => y::ItemKind::Extern(ffi::lower(g, x, cx)?),
        j::ItemKind::Trait(t) => y::ItemKind::Trait(y::Trait {
            is_pub: it.is_pub,
            path: vec![t.name.clone()],
            tparams: Vec::new(),
            assoc: Vec::new(),
            sigs: g.traits[&t.name]
                .iter()
                .map(|(n, s)| lower::signature(n, s, false, f_span(t, n)))
                .collect(),
            derived: None,
        }),
        j::ItemKind::Impl(i) => {
            let tr = impl_trait(i).expect("checked in collect");
            let for_ty = g.ty(&i.for_ty, cx, it.span)?;
            let key = for_ty.to_string();
            *g.self_ty.borrow_mut() = Some(for_ty.clone());
            let mut fns = Vec::new();
            for f in &i.fns {
                let s = &g.methods[&(key.clone(), f.name.clone())];
                fns.push(lower::function(g, f, s, false, cx));
            }
            *g.self_ty.borrow_mut() = None;
            y::ItemKind::Impl(y::Impl {
                is_unsafe: false,
                trait_: y::Named {
                    path: vec![tr],
                    args: Vec::new(),
                },
                for_ty: for_ty.core(),
                assoc: Vec::new(),
                fns: fns.into_iter().collect::<R<_>>()?,
            })
        }
    }))
}

#[cfg(test)]
mod selftest;

#[cfg(test)]
mod stdlib_selftest;
