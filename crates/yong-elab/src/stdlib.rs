//! The std modules as the elaborator sees them (R474, R475): method signatures and the
//! non-generic Xin bodies a module calls, read from yong-std's sources.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, OnceLock};

use yong_core::ast as y;
use yong_diag::Span;
use yong_hir as j;

use crate::{Cx, Globals, R, Ty};

pub(crate) struct Decl {
    pub module: &'static str,
    pub intrinsic: bool,
    /// An operation of a capability trait.
    pub cap: bool,
    pub def: j::FuncDef,
}

pub(crate) struct Class {
    module: &'static str,
    class: j::Class,
    span: Span,
}

#[derive(Default)]
struct Std {
    /// By path, `std.<module>.<name>`; capability operations by `std.<module>.<op>`.
    fns: HashMap<String, Decl>,
    /// By bare name; std class names do not repeat across modules.
    classes: HashMap<String, Class>,
    caps: HashSet<String>,
    /// Sum types by bare name, as classes.
    sums: HashMap<String, (&'static str, j::TypeDef)>,
    /// Constants by path, `std.<module>.<name>`.
    consts: HashMap<String, j::Expr>,
    /// Each module's file name and source, for diagnostics in std bodies.
    files: HashMap<&'static str, (String, &'static str)>,
}

/// A std module as the driver parsed it: its path below `std.`, its file name and source (for
/// diagnostics in its bodies), and its HIR.
pub type StdModule = (&'static str, String, &'static str, j::Module);

static LOADER: OnceLock<fn() -> Vec<StdModule>> = OnceLock::new();

/// Tell the elaborator how to load the std modules, which it does on first use. The driver
/// parses them (std sources are loaded by `yong-db`, so this crate depends on no surface,
/// R560); the first call wins.
pub fn install_std(load: fn() -> Vec<StdModule>) {
    let _ = LOADER.set(load);
}

static STD: LazyLock<Std> = LazyLock::new(|| {
    let mut out = Std::default();
    // This crate's tests load std as the driver does.
    #[cfg(test)]
    install_std(yong_db::std_modules);
    let load = LOADER
        .get()
        .expect("yong_elab::install_std runs before elaboration");
    for (module, file, src, m) in load() {
        out.files.insert(module, (file, src));
        for it in m.items {
            let has = |d: &str| it.decorators.iter().any(|x| x.path == [d]);
            let decl = |def: j::FuncDef, intrinsic, cap| Decl {
                module,
                intrinsic,
                cap,
                def,
            };
            match it.kind {
                j::ItemKind::Func(def) => {
                    let path = format!("std.{module}.{}", def.name);
                    out.fns.insert(path, decl(def, has("intrinsic"), false));
                }
                j::ItemKind::Trait(t) if has("capability") => {
                    out.caps.insert(t.name);
                    for (_, def) in t.sigs {
                        let path = format!("std.{module}.{}", def.name);
                        out.fns.insert(path, decl(def, false, true));
                    }
                }
                j::ItemKind::Type(t) if matches!(t.rhs, j::TypeRhs::Sum(_)) => {
                    out.sums.insert(t.name.clone(), (module, t));
                }
                j::ItemKind::Const { name, value, .. } => {
                    out.consts.insert(format!("std.{module}.{name}"), value);
                }
                j::ItemKind::Class(class) => {
                    let span = it.span;
                    out.classes.insert(
                        class.name.clone(),
                        Class {
                            module,
                            class,
                            span,
                        },
                    );
                }
                _ => {}
            }
        }
    }
    out
});

/// What raise needs from std (R559): which items elaboration copies in, which module declares a
/// type, and each function's parameters with their defaults.
pub struct StdInfo;

impl yong_hir::raise::Std for StdInfo {
    fn inserted(&self, path: &str) -> bool {
        path.starts_with("std.")
            || STD.classes.contains_key(path)
            || STD.sums.contains_key(path)
            || STD.fns.contains_key(&format!("std.prelude.{path}"))
    }

    fn type_module(&self, name: &str) -> Option<String> {
        class_module(name)
            .or_else(|| sum_module(name))
            .map(str::to_owned)
    }

    fn params(&self, path: &str) -> Option<Vec<yong_hir::raise::StdParam>> {
        let d = STD.fns.get(path)?;
        let slash = d
            .def
            .params
            .iter()
            .position(|p| matches!(p, j::Param::Slash));
        Some(
            d.def
                .params
                .iter()
                .enumerate()
                .filter_map(|(i, p)| match p {
                    j::Param::Value {
                        name, default, ty, ..
                    } => Some(yong_hir::raise::StdParam {
                        name: name.clone(),
                        default: default.clone(),
                        positional: slash.is_some_and(|s| i < s),
                        iter: matches!(ty, j::Type::Named { path, .. } if path == &["iter"]),
                    }),
                    _ => None,
                })
                .collect(),
        )
    }
}

pub(crate) fn decl(path: &str) -> Option<&'static Decl> {
    STD.fns.get(path)
}

/// Whether some std method named `m` takes its receiver `var` (R421).
pub(crate) fn mutates_receiver(m: &str) -> bool {
    static MUTATING: LazyLock<HashSet<String>> = LazyLock::new(|| {
        STD.fns
            .iter()
            .filter(|(_, d)| {
                matches!(
                    d.def.params.first(),
                    Some(j::Param::Value {
                        conv: Some(j::Conv::Var),
                        ..
                    })
                )
            })
            .filter_map(|(path, _)| path.rsplit('.').next().map(str::to_owned))
            .collect()
    });
    MUTATING.contains(m)
}

/// The value of std constant `path`, an expression with no free names.
pub(crate) fn konst(path: &str) -> Option<&'static j::Expr> {
    STD.consts.get(path)
}

/// The std module that declares sum type `name`.
pub(crate) fn sum_module(name: &str) -> Option<&'static str> {
    STD.sums.get(name).map(|(m, _)| *m)
}

/// The std module that declares class `name`.
pub(crate) fn class_module(name: &str) -> Option<&'static str> {
    STD.classes.get(name).map(|c| c.module)
}

/// A method instantiated at its receiver's type.
pub(crate) struct Method {
    /// The receiver is `var`.
    pub inout: bool,
    /// The other parameters: `take` and the type.
    pub params: Vec<(bool, Ty)>,
    pub ret: Ty,
    /// The type arguments, in the declaration's order.
    pub targs: Vec<Ty>,
}

impl Decl {
    fn tparams(&self) -> Vec<&str> {
        self.def
            .tparams
            .iter()
            .filter_map(|p| match p {
                j::TParam::Type { name, .. } => Some(name.as_str()),
                j::TParam::Value { .. } => None,
            })
            .collect()
    }

    /// `None` when the receiver does not match `self` or a type is beyond [`Ty`].
    pub fn method(&self, recv: &Ty) -> Option<Method> {
        let tps = self.tparams();
        let mut ps = self.def.params.iter().filter_map(|p| match p {
            j::Param::Value { conv, ty, .. } => Some((*conv, ty)),
            _ => None,
        });
        let (conv, self_ty) = ps.next()?;
        let mut b = HashMap::new();
        if !unify(self_ty, recv, &tps, &mut b) {
            return None;
        }
        let params = ps
            .map(|(c, t)| Some((c == Some(j::Conv::Take), subst(t, &b)?)))
            .collect::<Option<_>>()?;
        let ret = match self.def.ret.as_ref().and_then(|r| r.ty.as_ref()) {
            Some(t) => subst(t, &b)?,
            None => Ty::Unit,
        };
        let targs = tps
            .iter()
            .map(|n| b.get(*n).cloned())
            .collect::<Option<_>>()?;
        Some(Method {
            inout: conv == Some(j::Conv::Var),
            params,
            ret,
            targs,
        })
    }
}

/// A non-generic std function or capability operation at its only instance.
pub(crate) struct Plain {
    /// `take` and the type of each parameter, without an operation's `self`.
    pub params: Vec<(bool, Ty)>,
    /// Each parameter's name and default.
    pub names: Vec<(&'static str, Option<&'static j::Expr>)>,
    pub ret: Ty,
    pub fail: Option<Ty>,
}

impl Decl {
    /// A generic intrinsic whose type arguments come from all its arguments, not just the
    /// receiver: std.iter's functions and any that take a function. Calls to it are typed as
    /// calls to a module function.
    pub fn typed_at_call(&self) -> bool {
        self.intrinsic
            && !self.def.tparams.is_empty()
            && (self.module.starts_with("iter")
                || self.def.params.iter().any(|p| match p {
                    j::Param::Value { ty, .. } => match ty {
                        j::Type::Optional(t) => matches!(**t, j::Type::Callable { .. }),
                        j::Type::Named { path, .. } => path.len() == 1 && path[0] == "iter",
                        t => matches!(t, j::Type::Callable { .. }),
                    },
                    _ => false,
                })
                || self.tparam_past_receiver())
    }

    /// A type parameter that the receiver's type does not fix, as `P` in `join[P: AsPath]`.
    fn tparam_past_receiver(&self) -> bool {
        let mut ps = self.def.params.iter().filter_map(|p| match p {
            j::Param::Value { ty, .. } => Some(ty),
            _ => None,
        });
        let Some(recv) = ps.next() else {
            return false;
        };
        let rest: Vec<_> = ps.collect();
        self.tparams()
            .iter()
            .any(|n| !mentions(recv, n) && rest.iter().any(|t| mentions(t, n)))
    }

    /// `None` for a generic function or a type beyond [`Ty`].
    pub fn plain(&'static self) -> Option<Plain> {
        if !self.def.tparams.is_empty() {
            return None;
        }
        let b = HashMap::new();
        let names = self
            .def
            .params
            .iter()
            .skip(usize::from(self.cap))
            .filter_map(|p| match p {
                j::Param::Value { name, default, .. } => Some((name.as_str(), default.as_ref())),
                _ => None,
            })
            .collect();
        let params = self
            .def
            .params
            .iter()
            .skip(usize::from(self.cap))
            .map(|p| match p {
                j::Param::Value { conv, ty, .. } => {
                    Some((*conv == Some(j::Conv::Take), subst(ty, &b)?))
                }
                _ => None,
            })
            .collect::<Option<_>>()?;
        let r = self.def.ret.as_ref();
        let ret = match r.and_then(|r| r.ty.as_ref()) {
            Some(t) => subst(t, &b)?,
            None => Ty::Unit,
        };
        let fail = match r.map(|r| r.fail.as_slice()).unwrap_or_default() {
            [] => None,
            [p] => Some(subst(
                &j::Type::Named {
                    path: p.clone(),
                    args: Vec::new(),
                },
                &b,
            )?),
            _ => return None,
        };
        Some(Plain {
            params,
            names,
            ret,
            fail,
        })
    }
}

/// The words of every name, path and literal in `m`. Comments and docstrings are trivia (16.5),
/// so they never decide which std types a module sees (law 4).
fn words(m: &j::Module) -> HashSet<String> {
    let mut w = raw_words(m);
    // A built-in type names its declaring module (R475): `bytes` methods fail with
    // `std.encoding` types.
    if w.contains("bytes") || w.contains("bytearray") {
        w.insert("encoding".into());
    }
    // Civil values reach `ZonedDateTime`, `Disambiguation` and `TimeError` (R601).
    const TIME: [&str; 8] = [
        "Duration",
        "Instant",
        "MonoInstant",
        "PlainDate",
        "PlainTime",
        "PlainDateTime",
        "Period",
        "TimeZone",
    ];
    if TIME.iter().any(|t| w.contains(*t)) {
        w.insert("time".into());
    }
    w
}

fn raw_words(m: &j::Module) -> HashSet<String> {
    fn undoc(f: &mut j::FuncDef) {
        if let Some(b) = &mut f.body {
            b.doc = None;
        }
    }
    let mut m = m.clone();
    m.header.clear();
    m.doc = None;
    for it in &mut m.items {
        match &mut it.kind {
            j::ItemKind::Func(f) | j::ItemKind::Lemma(f) => undoc(f),
            j::ItemKind::Class(c) => c.doc = None,
            j::ItemKind::Trait(t) => {
                t.doc = None;
                t.sigs.iter_mut().for_each(|(_, f)| undoc(f));
            }
            j::ItemKind::Impl(i) => i.fns.iter_mut().for_each(undoc),
            j::ItemKind::Type(_) | j::ItemKind::Const { .. } | j::ItemKind::Extern(_) => {}
        }
    }
    // HIR's Debug text quotes every name, path segment and literal, and nothing else.
    format!("{m:?}")
        .split('"')
        .skip(1)
        .step_by(2)
        .flat_map(|s| s.split(|c: char| !c.is_alphanumeric() && c != '_'))
        .map(str::to_owned)
        .collect()
}

/// The std classes `m` may meet, registered in `g`: those whose name or module (a capability
/// such as `fs`) `src` names, unless `m` defines the name itself.
pub(crate) fn classes(g: &mut Globals, m: &j::Module) -> R<Vec<String>> {
    let own: HashSet<&str> = m.items.iter().map(|it| it.kind.name()).collect();
    let words = words(m);
    let mut out = Vec::new();
    for (name, c) in &STD.classes {
        if (words.contains(name.as_str()) || words.contains(c.module))
            && !own.contains(name.as_str())
        {
            g.records.insert(name.clone(), Vec::new());
            g.std_records.insert(name.clone());
            out.push((name, c));
        }
    }
    out.sort_by_key(|(n, _)| *n);
    for (name, c) in &out {
        let mut cx = std_cx(c.module);
        let mut fs = Vec::new();
        for f in &c.class.fields {
            fs.push(crate::Field {
                name: f.name.clone(),
                ty: g.ty(&f.ty, &mut cx, c.span)?,
                default: f.default.clone(),
            });
        }
        g.records.insert((*name).clone(), fs);
    }
    Ok(out.into_iter().map(|(n, _)| n.clone()).collect())
}

/// The std sum types `m` may meet, registered in `g` as [`classes`] registers classes. Their
/// variants have no fields.
pub(crate) fn sums(g: &mut Globals, m: &j::Module) -> Vec<String> {
    let own: HashSet<&str> = m.items.iter().map(|it| it.kind.name()).collect();
    let words = words(m);
    let mut out = Vec::new();
    for (name, (module, t)) in &STD.sums {
        let j::TypeRhs::Sum(vs) = &t.rhs else {
            continue;
        };
        // A std class may name a sum of its module in a field.
        let class_named = STD
            .classes
            .iter()
            .any(|(c, k)| k.module == *module && words.contains(c.as_str()));
        if (words.contains(name.as_str()) || words.contains(*module) || class_named)
            && !own.contains(name.as_str())
        {
            for v in vs {
                g.variant_of.insert(v.name.clone(), name.clone());
            }
            let vs = vs.iter().map(|v| (v.name.clone(), Vec::new())).collect();
            g.sums.insert(name.clone(), vs);
            out.push(name.clone());
        }
    }
    out.sort();
    out
}

fn mentions(t: &j::Type, n: &str) -> bool {
    match t {
        j::Type::Named { path, args } => {
            path.as_slice() == [n]
                || args
                    .iter()
                    .any(|a| matches!(a, j::TArg::Type(t) if mentions(t, n)))
        }
        j::Type::Callable { params, ret, .. } => {
            params.iter().any(|p| mentions(p, n)) || mentions(ret, n)
        }
        j::Type::Optional(t) => mentions(t, n),
        j::Type::None => false,
    }
}

fn unify(pat: &j::Type, t: &Ty, tps: &[&str], b: &mut HashMap<String, Ty>) -> bool {
    match pat {
        j::Type::Named { path, args }
            if args.is_empty() && tps.contains(&path.join(".").as_str()) =>
        {
            match b.get(&path[0]) {
                Some(prev) => prev == t,
                None => {
                    b.insert(path[0].clone(), t.clone());
                    true
                }
            }
        }
        j::Type::Optional(p) => matches!(t, Ty::Option(x) if unify(p, x, tps, b)),
        j::Type::None => *t == Ty::Unit,
        j::Type::Callable { .. } => false,
        j::Type::Named { path, args } if matches!(t, Ty::Data(_)) => {
            args.is_empty() && matches!(t, Ty::Data(n) if path.as_slice() == [n.as_str()])
        }
        j::Type::Named { path, args } => {
            let Some((name, parts)) = shape(t) else {
                return false;
            };
            path.as_slice() == [name]
                && args.len() == parts.len()
                && args.iter().zip(parts).all(|(p, x)| match p {
                    j::TArg::Type(p) => unify(p, x, tps, b),
                    _ => false,
                })
        }
    }
}

/// The Xin name of `t`'s type constructor and its arguments.
fn shape(t: &Ty) -> Option<(&'static str, Vec<&Ty>)> {
    Some(match t {
        Ty::Int => ("int", vec![]),
        Ty::Float => ("float", vec![]),
        Ty::BigInt => ("bigint", vec![]),
        Ty::Bool => ("bool", vec![]),
        Ty::Str => ("str", vec![]),
        Ty::Char => ("char", vec![]),
        Ty::List(x) => ("list", vec![x]),
        Ty::Set(x) => ("set", vec![x]),
        Ty::Dict(k, v) => ("dict", vec![k, v]),
        Ty::Result(x, e) => ("Result", vec![x, e]),
        Ty::Tuple(ts) => ("tuple", ts.iter().collect()),
        Ty::Coll(n, ts) => (*n, ts.iter().collect()),
        Ty::Fixed(n) => (
            yong_names::type_name(n, yong_names::Surface::Yong, yong_names::Surface::Xin)?,
            vec![],
        ),
        _ => return None,
    })
}

/// The type `pat` names once its type parameters are bound; `None` beyond [`Ty`].
fn subst(pat: &j::Type, b: &HashMap<String, Ty>) -> Option<Ty> {
    Some(match pat {
        j::Type::None => Ty::Unit,
        j::Type::Optional(t) => Ty::Option(Box::new(subst(t, b)?)),
        j::Type::Callable { .. } => return None,
        j::Type::Named { path, args } => {
            let a = args
                .iter()
                .map(|x| match x {
                    j::TArg::Type(t) => subst(t, b),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            let n = match path.as_slice() {
                [n] => n,
                [std, _, n] if std == "std" => n,
                _ => return None,
            };
            match (n.as_str(), a.as_slice()) {
                (n, [])
                    if STD.classes.contains_key(n)
                        || STD.caps.contains(n)
                        || STD.sums.contains_key(n) =>
                {
                    Ty::Data(n.to_owned())
                }
                (n, []) if b.contains_key(n) => b[n].clone(),
                ("int", []) => Ty::Int,
                ("float", []) => Ty::Float,
                ("bigint", []) => Ty::BigInt,
                ("bool", []) => Ty::Bool,
                ("str", []) => Ty::Str,
                ("char", []) => Ty::Char,
                (n, []) if crate::ty::fixed(n).is_some() => {
                    Ty::Fixed(crate::ty::fixed(n).expect("matched"))
                }
                ("list", [t]) => Ty::List(Box::new(t.clone())),
                ("set", [t]) => Ty::Set(Box::new(t.clone())),
                ("dict", [k, v]) => Ty::Dict(Box::new(k.clone()), Box::new(v.clone())),
                ("tuple", ts) => Ty::Tuple(ts.to_vec()),
                (n, ts) if crate::ty::collection(n).is_some_and(|(_, k)| k == ts.len()) => {
                    Ty::Coll(crate::ty::collection(n).expect("matched").0, ts.to_vec())
                }
                ("Result", [t, e]) => Ty::Result(Box::new(t.clone()), Box::new(e.clone())),
                _ => return None,
            }
        }
    })
}

/// A std function with a Xin body, lowered into the module that calls it.
pub(crate) struct Body {
    module: &'static str,
    /// The definition, renamed to the call path: the bare name for the prelude,
    /// `std.<module>.<name>` for a method.
    def: j::FuncDef,
    /// Native: only its signature is registered, for calls typed by [`Decl::typed_at_call`].
    intrinsic: bool,
}

/// The std functions with Xin bodies that `m` calls, directly or through each other, and does
/// not define itself.
fn bodies(m: &j::Module) -> Vec<Body> {
    let mut seen = HashSet::new();
    let mut names = HashSet::new();
    for it in &m.items {
        if let j::ItemKind::Func(f) = &it.kind {
            seen.insert(f.name.clone());
            if let Some(b) = &f.body {
                crate::lower::callees(&b.stmts, &mut names);
            }
        }
    }
    let mut out = Vec::new();
    while let Some(n) = names.iter().find(|n| !seen.contains(*n)).cloned() {
        seen.insert(n.clone());
        let prelude = decl(&format!("std.prelude.{n}")).map(|d| (n.clone(), d));
        let methods = STD
            .fns
            .iter()
            .filter(|(p, d)| d.module != "prelude" && p.rsplit('.').next() == Some(n.as_str()))
            .map(|(p, d)| (p.clone(), d));
        for (key, d) in prelude.into_iter().chain(methods) {
            if d.cap {
                continue;
            }
            if d.intrinsic {
                if d.typed_at_call() {
                    let mut def = d.def.clone();
                    def.name = key;
                    out.push(Body {
                        module: d.module,
                        def,
                        intrinsic: true,
                    });
                }
                continue;
            }
            let Some(b) = &d.def.body else { continue };
            crate::lower::callees(&b.stmts, &mut names);
            let mut def = d.def.clone();
            def.name = key;
            out.push(Body {
                module: d.module,
                def,
                intrinsic: false,
            });
        }
    }
    out.sort_by(|a, b| a.def.name.cmp(&b.def.name));
    out
}

/// Add the signatures of the std bodies `m` calls, so calls to them resolve like the module's
/// own functions.
pub(crate) fn collect(g: &mut Globals, m: &j::Module) -> R<Vec<Body>> {
    // A method of a std class the program never names (regex's `Match.text` for a `Path.text`
    // call) cannot be called, and its types are not registered.
    let unnamed = |b: &Body| {
        b.def.params.first().is_some_and(|p| {
            matches!(p, j::Param::Value { ty: j::Type::Named { path, .. }, .. }
                if path.len() == 1 && STD.classes.contains_key(&path[0])
                    && !g.std_records.contains(&path[0]))
        })
    };
    let bs: Vec<Body> = bodies(m).into_iter().filter(|b| !unnamed(b)).collect();
    for b in &bs {
        let mut cx = std_cx(b.module);
        let s = crate::sig(g, &b.def, &mut cx)?;
        g.fns.insert(b.def.name.clone(), s);
    }
    Ok(bs)
}

pub(crate) fn lower(g: &Globals, bs: &[Body]) -> R<Vec<y::Item>> {
    bs.iter()
        .filter(|b| !b.intrinsic)
        .map(|b| {
            let mut cx = std_cx(b.module);
            let f = crate::lower::function(g, &b.def, &g.fns[&b.def.name], false, &mut cx)?;
            // Warnings (a kept bounds check) stay inside std; errors would be std bugs.
            debug_assert!(
                cx.diags
                    .iter()
                    .all(|d| d.severity != yong_diag::Severity::Error),
                "std.{}: {:?}",
                b.module,
                cx.diags
            );
            Ok(y::Item {
                attrs: Vec::new(),
                kind: y::ItemKind::Fn(f),
                span: b.def.name_span,
            })
        })
        .collect()
}

fn std_cx(module: &str) -> Cx<'static> {
    let (file, src) = &STD.files[module];
    Cx {
        file,
        src,
        strict: true,
        release: false,
        wen: false,
        ffi: false,
        diags: Vec::new(),
    }
}
