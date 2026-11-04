//! HIR to Yan Core elaboration (14.1, 22.3), for Yan Agent and later Yan: resolve names, check
//! signatures, infer bodies bidirectionally, pass capabilities and lower to Yan IR in
//! A-normal form.
//!
//! The Yan Core tree keeps Yan Agent spans, so the Yan Core checker reports at Yan Agent positions.

mod bounds;
mod cli;
mod decorators;
mod desugar;
mod ffi;
mod lower;
mod stage0;
mod stdlib;
mod ty;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use yan_core::ast as y;
use yan_diag::{Applicability, Diagnostic, Span};
use yan_hir as j;

pub use desugar::{Manifest, manifest};
pub use stdlib::{StdInfo, StdModule, install_std};
pub use ty::Ty;

/// How the module is checked.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// `--strict`: the lenient reader is off (14.4).
    pub strict: bool,
    /// `--level release` (8.5).
    pub release: bool,
    /// The module came from Yan, whose lints differ from Yan Agent's (15.3).
    pub yn: bool,
    /// The module is an ffi module, under the package's `ffi/` directory (12).
    pub ffi: bool,
    /// The repository is not trusted, so stage 0 does not run (R676).
    pub untrusted: bool,
}

/// A construct the elaborator does not lower yet, at the innermost statement or expression
/// that was being lowered when it was found (E017, R649).
#[derive(Debug)]
pub struct Unsupported {
    pub what: String,
    pub at: Option<Span>,
}

impl Unsupported {
    /// Keeps the innermost span: the first one set wins as the error propagates outward.
    pub(crate) fn at(mut self, span: Span) -> Self {
        self.at.get_or_insert(span);
        self
    }
}

pub(crate) type R<T> = Result<T, Unsupported>;

pub(crate) fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(Unsupported {
        what: what.into(),
        at: None,
    })
}

/// E017 for a construct this toolchain does not support yet (R649).
pub fn unsupported_message(what: &str) -> String {
    format!("{what}: not supported by this toolchain yet")
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
    /// The capability trait of each binding in `caps` that is not named after it (`uses Kv as cache`).
    pub cap_tys: Vec<(String, String)>,
    /// The parameters a returned view borrows from (`-> View[T] from xs`, 4.5).
    pub from: Vec<String>,
    /// Erased ghost parameters, by position among all written parameters, and name.
    pub ghosts: Vec<(usize, String)>,
}

impl Sig {
    /// The capability trait that binding `c` of `caps` holds: `io` is `Io` unless renamed.
    pub fn cap_trait(&self, c: &str) -> String {
        self.cap_tys
            .iter()
            .find(|(b, _)| b == c)
            .map_or_else(|| cap_type(c), |(_, t)| t.clone())
    }
}

/// The binding a capability trait gets by default: `UserStore` binds as `user_store`.
fn cap_binding(t: &str) -> String {
    let mut out = String::new();
    for (i, ch) in t.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

#[derive(Default)]
pub(crate) struct Globals {
    pub records: HashMap<String, Vec<Field>>,
    pub sums: HashMap<String, Vec<(String, Vec<Field>)>>,
    pub variant_of: HashMap<String, String>,
    /// Each alias's alternatives: one for `type X = T`, two or more for a union alias.
    pub aliases: HashMap<String, Vec<j::Type>>,
    pub fns: HashMap<String, Sig>,
    /// The entries of `fns` that are std functions rather than the module's own.
    pub std_fns: HashSet<String>,
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
    /// Records marked `@derive(Cli)`, with their `@help` texts (RFC 0019 D14).
    pub clis: HashMap<String, HashMap<String, String>>,
    /// Functions marked `@deprecated`, for W815.
    pub deprecated: HashSet<String>,
    pub consts: HashMap<String, Ty>,
    /// The records that come from std rather than the module.
    pub std_records: std::collections::HashSet<String>,
    /// Functions called from a `test_` function (E601).
    pub tested: HashSet<String>,
    /// Each trait's methods, with `Self` as a type parameter.
    pub traits: HashMap<String, Vec<(String, Sig)>>,
    /// `(trait, type)` for each `impl`, the type as written in Yan Agent.
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
    pub yn: bool,
    pub ffi: bool,
    pub untrusted: bool,
    /// The previews the package opts into (20.6).
    pub previews: Vec<String>,
    pub diags: Vec<Diagnostic>,
}

impl<'s> Cx<'s> {
    pub fn note(&mut self, code: &str, span: Span, item: &str, message: String) {
        self.err(code, span, item, message);
        if let Some(d) = self.diags.last_mut() {
            d.severity = yan_diag::Severity::Note;
        }
    }

    pub fn warn(&mut self, code: &str, span: Span, item: &str, message: String) {
        self.err(code, span, item, message);
        if let Some(d) = self.diags.last_mut() {
            d.severity = yan_diag::Severity::Warning;
        }
    }

    pub fn err(&mut self, code: &str, span: Span, item: &str, message: String) {
        let quote = self.text(span);
        self.diags
            .push(Diagnostic::error(code, self.file, span, quote, message).with_item(item));
    }

    /// The source text of `span`, cut at the end of its first line.
    pub fn text(&self, span: Span) -> &'s str {
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
        line.get(from..to).unwrap_or("")
    }

    /// Adds to the last diagnostic a fix that replaces the text of `span` with `text`.
    pub fn fix(&mut self, applicability: Applicability, title: &str, span: Span, text: &str) {
        let search = self.text(span);
        if let Some(d) = self.diags.pop() {
            self.diags
                .push(d.with_fix(applicability, title, span, search, text));
        }
    }

    /// Adds a secondary span to the last diagnostic.
    pub fn label(&mut self, span: Span, text: &str) {
        if let Some(d) = self.diags.last_mut() {
            d.labels.push(yan_diag::Label {
                span,
                text: text.to_owned(),
            });
        }
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
            // A view is a borrowed list: a `let` parameter already borrows, and the escape
            // rules (E406, E414) are checked from the written types (4.5).
            j::Type::Named { path, args }
                if path.len() == 1 && path[0] == "View" && args.len() == 1 =>
            {
                let j::TArg::Type(t) = &args[0] else {
                    return unsupported("value type arguments");
                };
                Ty::List(Box::new(self.ty(t, cx, span)?))
            }
            j::Type::Named { path, args } => {
                let mut a = Vec::new();
                for arg in args {
                    match arg {
                        j::TArg::Type(t) => a.push(self.ty(t, cx, span)?),
                        _ => return unsupported("value or union type arguments"),
                    }
                }
                let mut name = path.join(".");
                // A std type named through its module, as `i18n.Locale`.
                if let [_, last] = path.as_slice()
                    && ty::collection(last).is_some()
                {
                    name = last.clone();
                }
                if let Some(t) = ty::builtin(&name, &a) {
                    return Ok(t);
                }
                match (name.as_str(), a.as_slice()) {
                    (n, []) if self.records.contains_key(n) || self.sums.contains_key(n) => {
                        Ty::Data(n.to_owned())
                    }
                    ("Self", []) if self.self_ty.borrow().is_some() => {
                        self.self_ty.borrow().clone().expect("checked")
                    }
                    (n, []) if self.scope.borrow().iter().any(|p| p == n) => {
                        Ty::Param(n.to_owned())
                    }
                    (n, []) if self.aliases.contains_key(n) => match self.aliases[n].as_slice() {
                        [t] => self.ty(t, cx, span)?,
                        ts => {
                            let mut ms = Vec::new();
                            for t in ts {
                                union_add(&mut ms, self.ty(t, cx, span)?);
                            }
                            Ty::Union(ms)
                        }
                    },
                    (n, []) if stdlib::STD.caps.contains(n) => Ty::Data(n.to_owned()),
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

/// Elaborate a parsed Yan Agent module. Returns the Yan Core module and the elaboration diagnostics;
/// when none is an error, the module is ready for `yan_check::check`.
pub fn elaborate(
    file: &str,
    src: &str,
    m: &j::Module,
    opts: Options,
) -> Result<(y::Module, Vec<Diagnostic>), Unsupported> {
    elaborate_in(file, src, m, opts, None)
}

/// [`elaborate`] for a module of the package whose manifest is `manifest`; a script-form module
/// declares its own (19.1).
pub fn elaborate_in(
    file: &str,
    src: &str,
    m: &j::Module,
    opts: Options,
    manifest: Option<Manifest>,
) -> Result<(y::Module, Vec<Diagnostic>), Unsupported> {
    let mut cx = Cx {
        file,
        src,
        strict: opts.strict,
        release: opts.release,
        yn: opts.yn,
        ffi: opts.ffi,
        untrusted: opts.untrusted,
        previews: Vec::new(),
        diags: Vec::new(),
    };
    let manifest = manifest.or_else(|| desugar::manifest(m));
    let mut m = desugar::module(m, &mut cx);
    desugar::without_manifest(&mut m);
    if let Some(caps) = manifest.as_ref().and_then(|p| p.capabilities.as_ref()) {
        desugar::ceiling(&m, caps, &mut cx);
    }
    cx.previews = manifest.map(|p| p.previews).unwrap_or_default();
    let m = &m;
    let mut g = Globals::default();
    let std_sums = stdlib::sums(&mut g, m);
    let std_classes = stdlib::classes(&mut g, m)?;
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
        let k = match item(&g, it, &mut cx)
            .and_then(|k| decorators::check(&g, it, start, &mut cx).map(|()| k))
        {
            Ok(k) => k,
            // R649: report the construct and go on with the next item.
            Err(u) => {
                let at = u.at.unwrap_or(it.span);
                cx.err("E017", at, it.kind.name(), unsupported_message(&u.what));
                continue;
            }
        };
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
            j::ItemKind::Type(t) if !t.tparams.is_empty() => {
                return unsupported("generic types").map_err(|u| u.at(it.span));
            }
            j::ItemKind::Type(t) => match &t.rhs {
                j::TypeRhs::Sum(vs) => {
                    for v in vs {
                        g.variant_of.insert(v.name.clone(), t.name.clone());
                    }
                    g.sums.insert(t.name.clone(), Vec::new());
                }
                j::TypeRhs::Alias(ts) => {
                    g.aliases.insert(t.name.clone(), ts.clone());
                }
                _ => {
                    return unsupported("newtypes and capability sets").map_err(|u| u.at(it.span));
                }
            },
            j::ItemKind::Record(c) if !c.tparams.is_empty() => {
                return unsupported("generic classes").map_err(|u| u.at(it.span));
            }
            j::ItemKind::Record(c) => {
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
                let cli = |d: &j::Decorator| {
                    d.path == ["derive"]
                        && d.args.as_deref().unwrap_or_default().iter().any(|a| {
                            matches!(&a.kind, j::ArgKind::Expr(j::Expr { kind: j::ExprKind::Name(n), .. }) if n == "Cli")
                        })
                };
                if it.decorators.iter().any(cli) {
                    let help = it
                        .decorators
                        .iter()
                        .find(|d| d.path == ["help"])
                        .map(cli::help)
                        .transpose();
                    match help {
                        Ok(h) => {
                            g.clis.insert(c.name.clone(), h.unwrap_or_default());
                        }
                        Err(msg) => cx.err("E269", it.span, &c.name, msg),
                    }
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
            j::ItemKind::Record(c) => {
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
    let mut first = HashMap::new();
    for it in &m.items {
        match &it.kind {
            j::ItemKind::Func(f) if g.fns.contains_key(&f.name) => {
                cx.err(
                    "E250",
                    f.name_span,
                    &f.name,
                    format!("`{}` is defined twice", f.name),
                );
                if let Some(&at) = first.get(&f.name) {
                    cx.label(at, "first defined");
                }
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
                first.insert(f.name.clone(), f.name_span);
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
    let mut impls = HashMap::new();
    for it in &m.items {
        if let j::ItemKind::Impl(i) = &it.kind
            && let Some((from, f)) = from_impl(i)
        {
            let a = g.ty(from, cx, it.span)?;
            let for_ty = g.ty(&i.for_ty, cx, it.span)?;
            let name = from_fn_name(i);
            let s = sig(g, f, cx)?;
            g.fns.insert(name.clone(), s);
            g.froms.insert((a.to_string(), for_ty.to_string()), name);
            continue;
        }
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
            if let Some(&at) = impls.get(&(tr.clone(), key.clone())) {
                cx.err(
                    "E220",
                    it.span,
                    &tr,
                    format!("`{key}` already implements `{tr}`"),
                );
                cx.label(at, "first implemented");
            } else {
                impls.insert((tr.clone(), key.clone()), it.span);
                g.impls.insert((tr.clone(), key.clone()));
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

/// `impl From[E] for T` with its one `convert` function; it lowers to a free function that `?`
/// calls to convert an `E` failure into `T` (8.1).
fn from_impl(i: &j::Impl) -> Option<(&j::Type, &j::FuncDef)> {
    match (i.trait_.as_slice(), i.args.as_slice(), i.fns.as_slice()) {
        ([t], [a], [f]) if t == "From" && i.assoc.is_empty() && f.name == "convert" => Some((a, f)),
        _ => None,
    }
}

fn from_fn_name(i: &j::Impl) -> String {
    let text = |t: &j::Type| match t {
        j::Type::Named { path, .. } => path.join("_"),
        _ => "t".into(),
    };
    format!("{}__from_{}", text(&i.for_ty), text(&i.args[0]))
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
    let mut ghosts = Vec::new();
    let mut positional = f.params.iter().any(|p| matches!(p, j::Param::Slash));
    for p in &f.params {
        match p {
            j::Param::Slash => positional = false,
            // A ghost parameter exists only in proofs: erased here and at every call (8.7).
            j::Param::Value {
                conv: Some(j::Conv::Ghost),
                name,
                ..
            } => ghosts.push((params.len() + ghosts.len(), name.clone())),
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
                    Some(j::Conv::Ghost) => unreachable!("ghost parameters are erased above"),
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
            let ret = match &r.ty {
                Some(t) => g.ty(t, cx, f.name_span)?,
                None => Ty::Unit,
            };
            let mut members = Vec::new();
            for p in &r.fail {
                if p.len() != 1 {
                    return unsupported("a qualified error type");
                }
                let t = g.ty(
                    &j::Type::Named {
                        path: p.clone(),
                        args: Vec::new(),
                    },
                    cx,
                    f.name_span,
                )?;
                union_add(&mut members, t);
            }
            if members.len() > 8 {
                cx.warn(
                    "W805",
                    fail_span(cx.src, f.name_span),
                    &f.name,
                    format!(
                        "this error union has {} members; name it as a sum type",
                        members.len()
                    ),
                );
            }
            let fail = match members.len() {
                0 => None,
                1 => members.pop(),
                _ => Some(Ty::Union(members)),
            };
            (ret, fail)
        }
    };
    let mut cap_tys = Vec::new();
    let caps = match &f.uses {
        None | Some(j::Uses::None) => Vec::new(),
        Some(j::Uses::Caps(cs)) => {
            let mut out = Vec::new();
            for c in cs {
                let [n] = c.path.as_slice() else {
                    return unsupported("qualified capabilities");
                };
                let upper = n.chars().next().is_some_and(char::is_uppercase);
                let binding = match &c.alias {
                    Some(a) => a.clone(),
                    None if upper => cap_binding(n),
                    None => n.clone(),
                };
                let t = if upper { n.clone() } else { cap_type(n) };
                if t != cap_type(&binding) {
                    cap_tys.push((binding.clone(), t));
                }
                out.push(binding);
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
        cap_tys,
        from: f.ret.as_ref().map(|r| r.from.clone()).unwrap_or_default(),
        ghosts,
    })
}

/// Adds `t` to an error union's members, flattening a union alias and skipping repeats.
fn union_add(ms: &mut Vec<Ty>, t: Ty) {
    let ts = match t {
        Ty::Union(ts) => ts,
        t => vec![t],
    };
    for t in ts {
        if !ms.contains(&t) {
            ms.push(t);
        }
    }
}

/// The error type after `fail` on the header line of the function named at `name`.
fn fail_span(src: &str, name: Span) -> Span {
    let line = src.lines().nth(name.line as usize - 1).unwrap_or("");
    let Some(k) = line.find(" fail ") else {
        return name;
    };
    let col = (k + " fail ".len() + 1) as u32;
    let text = line[k + " fail ".len()..].trim_end().trim_end_matches(':');
    Span {
        line: name.line,
        col,
        end_line: name.line,
        end_col: col + text.trim_end().chars().count() as u32,
    }
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
        j::ItemKind::Record(c) => y::ItemKind::Record(y::Record {
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
            let declared = matches!(&f.uses, Some(j::Uses::Caps(cs)) if !cs.is_empty());
            if declared {
                cx.err(
                    "E605",
                    f.name_span,
                    &f.name,
                    "a lemma uses no capabilities; drop the `uses` clause".into(),
                );
            }
            let mut uses = Vec::new();
            if let Some(b) = f.body.as_ref().filter(|_| !declared) {
                lower::cap_uses(&b.stmts, &mut uses);
            }
            let params: Vec<&str> = f
                .params
                .iter()
                .filter_map(|p| match p {
                    j::Param::Value { name, .. } => Some(name.as_str()),
                    _ => None,
                })
                .collect();
            for (c, at) in uses {
                if !params.contains(&c.as_str()) {
                    cx.err(
                        "E605",
                        at,
                        &f.name,
                        format!("a lemma uses no capabilities, but this uses `{c}`"),
                    );
                }
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
                } else if cx.yn && !has_ex {
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
        j::ItemKind::Impl(i) if let Some((_, f)) = from_impl(i) => {
            let name = from_fn_name(i);
            let f = j::FuncDef {
                name: name.clone(),
                ..f.clone()
            };
            y::ItemKind::Fn(lower::function(g, &f, &g.fns[&name], false, cx)?)
        }
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
