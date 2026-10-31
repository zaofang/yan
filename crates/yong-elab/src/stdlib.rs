//! The std modules as the elaborator sees them (R474, R475): method signatures and the
//! non-generic Jian bodies a module calls, read from yong-std's sources.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use yong_core::ast as y;
use yong_hir as j;

use crate::{Cx, Globals, R, Ty};

pub(crate) struct Decl {
    pub module: &'static str,
    file: String,
    pub src: &'static str,
    pub intrinsic: bool,
    pub def: j::FuncDef,
}

/// Every std function by its path, `std.<module>.<name>`.
static DECLS: LazyLock<HashMap<String, Decl>> = LazyLock::new(|| {
    let mut out = HashMap::new();
    for &(module, src) in yong_std::SOURCES {
        let (m, diags) = yong_jian::parse(&file(module), src, true);
        assert!(diags.is_empty(), "std.{module}: {diags:?}");
        for it in m.expect("std parses").items {
            if let j::ItemKind::Func(def) = it.kind {
                let intrinsic = it.decorators.iter().any(|d| d.path == ["intrinsic"]);
                out.insert(
                    format!("std.{module}.{}", def.name),
                    Decl {
                        module,
                        file: file(module),
                        src,
                        intrinsic,
                        def,
                    },
                );
            }
        }
    }
    out
});

fn file(module: &str) -> String {
    format!("std/{module}.jian")
}

pub(crate) fn decl(path: &str) -> Option<&'static Decl> {
    DECLS.get(path)
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

/// The Jian name of `t`'s type constructor and its arguments.
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
            let [n] = path.as_slice() else { return None };
            match (n.as_str(), a.as_slice()) {
                (n, []) if b.contains_key(n) => b[n].clone(),
                ("int", []) => Ty::Int,
                ("float", []) => Ty::Float,
                ("bigint", []) => Ty::BigInt,
                ("bool", []) => Ty::Bool,
                ("str", []) => Ty::Str,
                ("char", []) => Ty::Char,
                ("list", [t]) => Ty::List(Box::new(t.clone())),
                ("set", [t]) => Ty::Set(Box::new(t.clone())),
                ("dict", [k, v]) => Ty::Dict(Box::new(k.clone()), Box::new(v.clone())),
                ("tuple", ts) => Ty::Tuple(ts.to_vec()),
                ("Result", [t, e]) => Ty::Result(Box::new(t.clone()), Box::new(e.clone())),
                _ => return None,
            }
        }
    })
}

/// The prelude functions with non-generic Jian bodies that `m` calls, directly or through
/// each other, and does not define itself.
fn prelude_bodies(m: &j::Module) -> Vec<&'static Decl> {
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
        let Some(d) = decl(&format!("std.prelude.{n}")) else {
            continue;
        };
        if d.intrinsic || !d.def.tparams.is_empty() {
            continue;
        }
        if let Some(b) = &d.def.body {
            crate::lower::callees(&b.stmts, &mut names);
        }
        out.push(d);
    }
    out
}

/// Add the signatures of the std bodies `m` calls, so calls to them resolve like the module's
/// own functions.
pub(crate) fn collect(g: &mut Globals, m: &j::Module) -> R<Vec<&'static Decl>> {
    let ds = prelude_bodies(m);
    for d in &ds {
        let mut cx = std_cx(d);
        let s = crate::sig(g, &d.def, &mut cx)?;
        g.fns.insert(d.def.name.clone(), s);
    }
    Ok(ds)
}

pub(crate) fn lower(g: &Globals, ds: &[&'static Decl]) -> R<Vec<y::Item>> {
    ds.iter()
        .map(|d| {
            let mut cx = std_cx(d);
            let f = crate::lower::function(g, &d.def, false, &mut cx)?;
            debug_assert!(cx.diags.is_empty(), "std.{}: {:?}", d.module, cx.diags);
            Ok(y::Item {
                attrs: Vec::new(),
                kind: y::ItemKind::Fn(f),
                span: d.def.name_span,
            })
        })
        .collect()
}

fn std_cx(d: &'static Decl) -> Cx<'static> {
    Cx {
        file: &d.file,
        src: d.src,
        strict: true,
        release: false,
        diags: Vec::new(),
    }
}
