//! Monomorphization (R772): each generic `fn` instance a non-generic item reaches becomes an
//! `inst fn` item with the type parameters replaced. Calls keep their text: `id[i64](x)` names
//! `inst fn id[i64]`. Generic `fn` definitions do not appear in the output.

use std::collections::HashMap;

use yan_core::ast::*;
use yan_core::hash::{self, ItemHash};
use yan_diag::{Diagnostic, Span};

use crate::Error;

/// Nested instances allowed before polymorphic recursion is E275.
pub const DEPTH: usize = 64;

/// The generic item's identity hash and its arguments' canonical text.
type Key = (ItemHash, Vec<String>);

/// A let-bound closure's declared type, body and captured locals.
type Bound = (Type, Closure, Vec<(Name, Type)>);

/// A let-bound closure passed as a `comptime` argument. Its captured locals become extra value
/// parameters of the instance and the closure is re-bound inside it, so the instance is keyed by
/// the closure's identity rather than by a caller variable.
#[derive(Clone)]
struct Spec {
    /// The `comptime` parameter the closure is bound to.
    name: Name,
    ty: Type,
    closure: Closure,
    /// The captured locals and their types, in capture order.
    caps: Vec<(Name, Type)>,
}

struct Found {
    def: usize,
    args: Vec<TArg>,
    at: Span,
    specs: Vec<Spec>,
}

pub fn monomorphize(file: &str, m: &Module) -> Result<Module, Error> {
    let hashes = hash::hash_module(m);
    let generic: HashMap<String, usize> = m
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| match &it.kind {
            ItemKind::Fn(f) if is_generic(f) => Some((f.path.join("."), i)),
            _ => None,
        })
        .collect();
    let comptime_at: HashMap<usize, Vec<usize>> = generic
        .values()
        .map(|&i| {
            let ItemKind::Fn(f) = &m.items[i].kind else {
                unreachable!("`generic` holds fns")
            };
            (i, comptime_params(f))
        })
        .collect();
    let mut mono = Mono {
        file,
        m,
        keys: HashMap::new(),
        insts: Vec::new(),
    };
    let mut items = Vec::new();
    let none = HashMap::new();
    for it in &m.items {
        if matches!(&it.kind, ItemKind::Fn(f) if is_generic(f)) {
            continue;
        }
        let mut it = it.clone();
        let mut w = Walk::new(m, &none, &generic, &comptime_at);
        w.item(&mut it.kind);
        w.finish()?;
        mono.add(w.found, 1, &hashes)?;
        items.push(it);
    }
    // Instances are built in the order they are first reached; building one can add more.
    let mut done = 0;
    while done < mono.insts.len() {
        let (def, args, depth, specs) = mono.insts[done].clone();
        let it = &m.items[def];
        let ItemKind::Fn(f) = &it.kind else {
            unreachable!("only generic fns are instantiated")
        };
        let env: HashMap<String, TArg> = f
            .tparams
            .iter()
            .zip(&args)
            .map(|(p, a)| (p.name.clone(), a.clone()))
            .collect();
        let mut def = FnDef {
            tparams: Vec::new(),
            ..f.clone()
        };
        bind_comptime_values(&mut def, &args[f.tparams.len()..], specs, it.span);
        let mut kind = ItemKind::Inst(Inst { args, def });
        let mut w = Walk::new(m, &env, &generic, &comptime_at);
        w.item(&mut kind);
        w.finish()?;
        mono.add(w.found, depth + 1, &hashes)?;
        items.push(Item {
            attrs: it.attrs.clone(),
            kind,
            span: it.span,
        });
        done += 1;
    }
    Ok(Module {
        header: m.header.clone(),
        path: m.path.clone(),
        items,
    })
}

fn is_generic(f: &FnDef) -> bool {
    (!f.tparams.is_empty() || !comptime_params(f).is_empty()) && f.body.is_some()
}

/// Indexes of the `comptime` value parameters.
fn comptime_params(f: &FnDef) -> Vec<usize> {
    f.params
        .iter()
        .enumerate()
        .filter(|(_, p)| matches!(p, Param::Value { comptime: true, .. }))
        .map(|(i, _)| i)
        .collect()
}

/// Drops the `comptime` value parameters of an instance and binds each to its argument at the
/// top of the body; the instance is then keyed by the value, so the binding is a constant.
fn bind_comptime_values(def: &mut FnDef, values: &[TArg], specs: Vec<Spec>, span: Span) {
    let mut binds = Vec::new();
    let mut vals = values.iter();
    let mut specs = specs.into_iter();
    let mut cap_params = Vec::new();
    def.params.retain(|p| match p {
        Param::Value {
            comptime: true,
            name,
            ty,
            ..
        } => {
            let Some(TArg::Nat(n)) = vals.next() else {
                unreachable!("checked at the call")
            };
            if matches!(ty, Type::Fn(_)) {
                let spec = specs
                    .next()
                    .expect("one spec per function-valued parameter");
                cap_params.extend(spec.caps.iter().map(|(n, t)| Param::Value {
                    comptime: false,
                    conv: Conv::Let,
                    name: n.clone(),
                    ty: t.clone(),
                }));
                binds.push(closure_bind(&spec, span));
                return false;
            }
            binds.push(Stmt {
                kind: StmtKind::Let {
                    mutable: false,
                    pat: Pat::Bind(name.clone()),
                    pat_span: span,
                    ty: ty.clone(),
                    rhs: Rhs::Atom(Atom::Lit(Literal::Int(n.clone()), span)),
                    rhs_span: span,
                },
                span,
            });
            false
        }
        _ => true,
    });
    def.params.extend(cap_params);
    if let Some(b) = &mut def.body {
        b.stmts.splice(0..0, binds);
    }
}

fn closure_bind(spec: &Spec, span: Span) -> Stmt {
    Stmt {
        kind: StmtKind::Let {
            mutable: false,
            pat: Pat::Bind(spec.name.clone()),
            pat_span: span,
            ty: spec.ty.clone(),
            rhs: Rhs::Closure(Box::new(spec.closure.clone())),
            rhs_span: span,
        },
        span,
    }
}

/// The closure's identity: the content hash of a function that only binds it over its captures,
/// as a decimal so that it reads as an instance argument.
fn spec_id(m: &Module, callee: &FnDef, spec: &Spec, span: Span) -> String {
    let mut def = FnDef {
        tparams: Vec::new(),
        params: spec
            .caps
            .iter()
            .map(|(n, t)| Param::Value {
                comptime: false,
                conv: Conv::Let,
                name: n.clone(),
                ty: t.clone(),
            })
            .collect(),
        ..callee.clone()
    };
    def.path = vec!["spec".into()];
    def.body = Some(Block {
        stmts: vec![closure_bind(spec, span)],
    });
    let item = Item {
        attrs: Vec::new(),
        kind: ItemKind::Fn(def),
        span,
    };
    let probe = Module {
        header: m.header.clone(),
        path: m.path.clone(),
        items: vec![item],
    };
    let hex = hash::hash_module(&probe)[0].identity.group.to_hex();
    u64::from_str_radix(&hex[..16], 16)
        .expect("hex digits")
        .to_string()
}

struct Mono<'a> {
    file: &'a str,
    m: &'a Module,
    keys: HashMap<Key, usize>,
    /// Each instance: its generic item, its arguments and its nesting depth.
    insts: Vec<(usize, Vec<TArg>, usize, Vec<Spec>)>,
}

impl Mono<'_> {
    fn add(
        &mut self,
        found: Vec<Found>,
        depth: usize,
        hashes: &[hash::Hashed],
    ) -> Result<(), Error> {
        for Found {
            def,
            args,
            at,
            specs,
        } in found
        {
            let ItemKind::Fn(f) = &self.m.items[def].kind else {
                unreachable!("`generic` holds fns")
            };
            let name = f.path.join(".");
            if f.tparams.iter().any(|t| t.kind == Kind::Dim) {
                return Err(Error::Unsupported("a Dim type parameter".into()));
            }
            if f.tparams.len() + comptime_params(f).len() != args.len() {
                return Err(Error::Unsupported(format!(
                    "call of `{name}` without its type arguments"
                )));
            }
            let text = args
                .iter()
                .map(|a| match a {
                    TArg::Type(t) => Ok(yan_core::type_text(t)),
                    TArg::Nat(n) => Ok(n.clone()),
                })
                .collect::<Result<_, _>>()?;
            let key = (hashes[def].identity, text);
            if self.keys.contains_key(&key) {
                continue;
            }
            if depth > DEPTH {
                let d = Diagnostic::error(
                    "E275",
                    self.file,
                    at,
                    "",
                    format!("instantiation depth exceeded: `{name}` nests past {DEPTH} instances"),
                )
                .with_item(name);
                return Err(Error::Diag(Box::new(d)));
            }
            self.keys.insert(key, self.insts.len());
            self.insts.push((def, args, depth, specs));
        }
        Ok(())
    }
}

/// Replaces type parameters by `env` and collects the calls that name generic fns.
struct Walk<'a> {
    m: &'a Module,
    env: &'a HashMap<String, TArg>,
    generic: &'a HashMap<String, usize>,
    /// The `comptime` value parameter indexes of each generic item.
    comptime_at: &'a HashMap<usize, Vec<usize>>,
    found: Vec<Found>,
    err: Option<Error>,
    /// Declared types of the locals seen so far.
    locals: HashMap<Name, Type>,
    /// Let-bound closures in scope, with their declared type and capture types.
    closures: HashMap<Name, Bound>,
}

impl<'a> Walk<'a> {
    fn new(
        m: &'a Module,
        env: &'a HashMap<String, TArg>,
        generic: &'a HashMap<String, usize>,
        comptime_at: &'a HashMap<usize, Vec<usize>>,
    ) -> Self {
        Walk {
            m,
            env,
            generic,
            comptime_at,
            found: Vec::new(),
            err: None,
            locals: HashMap::new(),
            closures: HashMap::new(),
        }
    }

    fn finish(&mut self) -> Result<(), Error> {
        self.err.take().map_or(Ok(()), Err)
    }

    fn item(&mut self, k: &mut ItemKind) {
        match k {
            ItemKind::Fn(f) | ItemKind::Gen(f) => self.func(f),
            ItemKind::Inst(i) => self.func(&mut i.def),
            ItemKind::Impl(i) => i.fns.iter_mut().for_each(|f| self.func(f)),
            ItemKind::Test(t) => self.block(&mut t.body),
            _ => {}
        }
    }

    fn func(&mut self, f: &mut FnDef) {
        f.params.iter_mut().for_each(|p| self.param(p));
        self.ty(&mut f.ret);
        if let Some(t) = &mut f.fail {
            self.ty(t);
        }
        for c in &mut f.contracts {
            match c {
                Contract::Requires(e) | Contract::Ensures(e) | Contract::Example(e) => self.expr(e),
                Contract::Decreases(es) => es.iter_mut().for_each(|e| self.expr(e)),
                Contract::Cost { steps, alloc } => {
                    steps.iter_mut().chain(alloc).for_each(|e| self.expr(e))
                }
            }
        }
        if let Some(b) = &mut f.body {
            self.block(b);
        }
    }

    fn param(&mut self, p: &mut Param) {
        match p {
            Param::Value { name, ty, .. } => {
                self.ty(ty);
                self.locals.insert(name.clone(), ty.clone());
            }
            Param::Cap { ty, .. } => self.targs(&mut ty.args),
        }
    }

    fn ty(&mut self, t: &mut Type) {
        match t {
            Type::Named(n) => {
                if let ([p], []) = (n.path.as_slice(), n.args.as_slice())
                    && let Some(TArg::Type(c)) = self.env.get(p)
                {
                    *t = c.clone();
                    return;
                }
                self.targs(&mut n.args);
            }
            Type::Dyn(n) => self.targs(&mut n.args),
            Type::Tuple(ts) | Type::Union(ts) => ts.iter_mut().for_each(|t| self.ty(t)),
            Type::Fn(f) => {
                f.params.iter_mut().for_each(|(_, t)| self.ty(t));
                self.ty(&mut f.ret);
                if let Some(t) = &mut f.fail {
                    self.ty(t);
                }
            }
            Type::Refined { ty, pred, .. } => {
                self.ty(ty);
                self.expr(pred);
            }
        }
    }

    fn targs(&mut self, args: &mut [TArg]) {
        for a in args {
            let TArg::Type(t) = a else { continue };
            if let Type::Named(n) = t
                && let ([p], []) = (n.path.as_slice(), n.args.as_slice())
                && let Some(c @ TArg::Nat(_)) = self.env.get(p)
            {
                *a = c.clone();
            } else {
                self.ty(t);
            }
        }
    }

    fn call(&mut self, c: &mut Call) {
        self.targs(&mut c.targs);
        let Some(&def) = self.generic.get(&c.path.join(".")) else {
            return;
        };
        let ItemKind::Fn(callee) = &self.m.items[def].kind else {
            unreachable!("`generic` holds fns")
        };
        let at = &self.comptime_at[&def];
        // Already rewritten: a closure copied into an instance carries its calls along.
        if !at.is_empty() && c.targs.len() == callee.tparams.len() + at.len() {
            self.found.push(Found {
                def,
                args: c.targs.clone(),
                at: c.span,
                specs: Vec::new(),
            });
            return;
        }
        // A `comptime` argument moves from the call's arguments to its type arguments; a
        // closure's captures follow the remaining arguments.
        let mut values = Vec::new();
        let mut specs = Vec::new();
        let mut cap_args = Vec::new();
        for &i in at {
            let Param::Value { name, ty, .. } = &callee.params[i] else {
                unreachable!("`comptime` parameters are values")
            };
            match (ty, c.args.get(i)) {
                (Type::Fn(_), Some(Arg::Value(_, Atom::Name(n, span), _))) => {
                    let Some((cty, closure, caps)) = self.closures.get(n) else {
                        self.err.get_or_insert(Error::Unsupported(format!(
                            "a `comptime` function argument of `{}` that is not a let-bound closure",
                            c.path.join(".")
                        )));
                        return;
                    };
                    if let Some((conv, cap, _)) =
                        closure.captures.iter().find(|(v, ..)| *v != Conv::Let)
                    {
                        self.err.get_or_insert(Error::Unsupported(format!(
                            "a `comptime` closure argument of `{}` capturing `{cap}` by {conv:?}",
                            c.path.join(".")
                        )));
                        return;
                    }
                    let spec = Spec {
                        name: name.clone(),
                        ty: cty.clone(),
                        closure: closure.clone(),
                        caps: caps.clone(),
                    };
                    values.push(TArg::Nat(spec_id(self.m, callee, &spec, c.span)));
                    cap_args.extend(spec.caps.iter().map(|(cap, _)| {
                        Arg::Value(Conv::Let, Atom::Name(cap.clone(), *span), *span)
                    }));
                    specs.push(spec);
                }
                (_, Some(Arg::Value(_, Atom::Lit(Literal::Int(n), _), _))) => {
                    values.push(TArg::Nat(n.clone()))
                }
                _ => {
                    self.err.get_or_insert(Error::Unsupported(format!(
                        "a `comptime` argument of `{}` that is not an integer literal",
                        c.path.join(".")
                    )));
                    return;
                }
            }
        }
        for &i in at.iter().rev() {
            c.args.remove(i);
        }
        c.args.extend(cap_args);
        c.targs.extend(values);
        self.found.push(Found {
            def,
            args: c.targs.clone(),
            at: c.span,
            specs,
        });
    }

    fn remember_closure(&mut self, name: &Name, ty: &Type, c: &Closure) {
        let caps: Option<Vec<_>> = c
            .captures
            .iter()
            .map(|(_, n, _)| self.locals.get(n).map(|t| (n.clone(), t.clone())))
            .collect();
        match caps {
            Some(caps) => {
                self.closures
                    .insert(name.clone(), (ty.clone(), c.clone(), caps));
            }
            None => {
                self.err.get_or_insert(Error::Unsupported(
                    "a closure capturing a local of unknown type".into(),
                ));
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        if let Expr::Call(c) = e {
            self.call(c);
        }
    }

    fn block(&mut self, b: &mut Block) {
        b.stmts.iter_mut().for_each(|s| self.stmt(&mut s.kind));
    }

    fn stmt(&mut self, s: &mut StmtKind) {
        match s {
            StmtKind::Let { pat, ty, rhs, .. } => {
                self.ty(ty);
                self.rhs(rhs);
                if let Pat::Bind(n) = pat {
                    self.locals.insert(n.clone(), ty.clone());
                    self.closures.remove(n);
                    if let Rhs::Closure(c) = rhs {
                        self.remember_closure(n, ty, c);
                    }
                }
            }
            StmtKind::Assign { rhs, .. } | StmtKind::Rhs(rhs) => self.rhs(rhs),
            StmtKind::If { then, els, .. } | StmtKind::ComptimeIf { then, els, .. } => {
                self.block(then);
                if let Some(b) = els {
                    self.block(b);
                }
            }
            StmtKind::Match { arms, .. } => arms.iter_mut().for_each(|(_, b)| self.block(b)),
            StmtKind::Loop {
                invariants,
                decreases,
                body,
                ..
            } => {
                invariants.iter_mut().for_each(|e| self.expr(e));
                decreases.iter_mut().for_each(|e| self.expr(e));
                self.block(body);
            }
            StmtKind::Jp { params, body, .. } => {
                params.iter_mut().for_each(|p| self.param(p));
                self.block(body);
            }
            StmtKind::Become(c) => self.call(c),
            StmtKind::With { body, .. }
            | StmtKind::Deadline { body, .. }
            | StmtKind::Unsafe(body) => self.block(body),
            StmtKind::Par { blocks, .. } => blocks.iter_mut().for_each(|b| self.block(b)),
            StmtKind::ComptimeFor { ty, body, .. } => {
                self.ty(ty);
                self.block(body);
            }
            StmtKind::Assert(e) => self.expr(e),
            StmtKind::Reuse { ctor, .. } => self.ctor(ctor),
            StmtKind::Break { .. }
            | StmtKind::Continue { .. }
            | StmtKind::Jump { .. }
            | StmtKind::Ret(_)
            | StmtKind::Fail(_)
            | StmtKind::Yield(_)
            | StmtKind::Unreachable
            | StmtKind::Asm { .. }
            | StmtKind::Drop(_) => {}
        }
    }

    fn rhs(&mut self, r: &mut Rhs) {
        match r {
            Rhs::Call(c) | Rhs::Try(c) | Rhs::Propagate { call: c, .. } => self.call(c),
            Rhs::Ctor(c) => self.ctor(c),
            Rhs::Closure(c) => {
                c.params.iter_mut().for_each(|p| self.param(p));
                self.ty(&mut c.ret);
                if let Some(t) = &mut c.fail {
                    self.ty(t);
                }
                self.block(&mut c.body);
            }
            Rhs::Atom(_) | Rhs::Proj(_) | Rhs::Copy(_) | Rhs::Move(_) => {}
        }
    }

    fn ctor(&mut self, c: &mut Ctor) {
        if let Ctor::Named { targs, .. } = c {
            self.targs(targs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(src: &str) -> Result<Module, Error> {
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let m = m.expect("parses");
        monomorphize("t.yir", &m)
    }

    #[test]
    fn replaces_generic_fns_by_their_instances() {
        let src = "module t\n\
            fn id[T: Type](let x: T) -> T {\n ret x\n}\n\
            fn twice[T: Type](let x: T) -> T {\n let y: T = id[T](let x)\n ret y\n}\n\
            fn main() -> Unit {\n let a: i64 = twice[i64](let 1)\n let b: Str = id[Str](let \"s\")\n\
             let c: i64 = id[i64](let a)\n ret ()\n}\n";
        let text = yan_core::print(&opt(src).expect("specializes"));
        assert!(!text.contains("fn id[T"), "{text}");
        let insts: Vec<&str> = text.lines().filter(|l| l.starts_with("inst fn")).collect();
        assert_eq!(
            insts,
            [
                "inst fn twice[i64](let x: i64) -> i64 {",
                "inst fn id[Str](let x: Str) -> Str {",
                "inst fn id[i64](let x: i64) -> i64 {",
            ]
        );
        assert!(text.contains("let y: i64 = id[i64](let x)"), "{text}");
        let again = yan_core::parse("t.yir", &text)
            .0
            .expect("optimized Yan Core parses");
        assert_eq!(yan_core::print(&again), text);
    }

    #[test]
    fn polymorphic_recursion_is_e275() {
        let src = "module t\n\
            fn f[T: Type](let x: T) -> Unit {\n let l: List[T] = std.list.new[T]()\n\
             let u: Unit = f[List[T]](let l)\n ret ()\n}\n\
            fn main() -> Unit {\n let u: Unit = f[i64](let 1)\n ret ()\n}\n";
        let Err(Error::Diag(d)) = opt(src) else {
            panic!("expected E275")
        };
        assert_eq!(d.code, "E275");
    }

    #[test]
    fn comptime_values_make_one_instance_each() {
        let src = "module t\n\
            fn scale(comptime let k: i64, let x: i64) -> i64 {\n ret x\n}\n\
            fn sized[comptime n: Nat](let xs: Array[n, f64]) -> Array[n, f64] {\n\
             let ys: Array[n, f64] = copy xs\n ret ys\n}\n\
            fn main() -> Unit {\n let a: i64 = scale(let 3, let 1)\n\
             let b: i64 = scale(let 3, let 2)\n let c: i64 = scale(let 4, let 1)\n\
             let xs: Array[2, f64] = std.array.new[2, f64]()\n\
             let ys: Array[2, f64] = sized[2](let xs)\n ret ()\n}\n";
        let text = yan_core::print(&opt(src).expect("specializes"));
        let insts: Vec<&str> = text.lines().filter(|l| l.starts_with("inst fn")).collect();
        assert_eq!(
            insts,
            [
                "inst fn scale[3](let x: i64) -> i64 {",
                "inst fn scale[4](let x: i64) -> i64 {",
                "inst fn sized[2](let xs: Array[2, f64]) -> Array[2, f64] {",
            ]
        );
        assert!(text.contains("let k: i64 = 3"), "{text}");
        assert!(text.contains("scale[3](let 2)"), "{text}");
    }

    #[test]
    fn comptime_argument_must_be_a_literal() {
        let src = "module t\n\
            fn scale(comptime let k: i64, let x: i64) -> i64 {\n ret x\n}\n\
            fn main() -> Unit {\n let n: i64 = 3\n let a: i64 = scale(let n, let 1)\n ret ()\n}\n";
        assert!(matches!(opt(src), Err(Error::Unsupported(m)) if m.contains("comptime")));
    }

    const APPLY: &str = "module t\n\
        fn apply(comptime let f: Fn(let i64) -> i64, let x: i64) -> i64 {\n\
         let r: i64 = f(let x)\n ret r\n}\n";

    #[test]
    fn closures_specialize_with_captures_lifted() {
        let src = format!(
            "{APPLY}fn main() -> Unit {{\n let base: i64 = 10\n\
             let add: Fn(let i64) -> i64 = fn [let base] (let x: i64) -> i64 {{\n ret x\n}}\n\
             let dbl: Fn(let i64) -> i64 = fn [] (let x: i64) -> i64 {{\n ret x\n}}\n\
             let a: i64 = apply(let add, let 1)\n let b: i64 = apply(let add, let 2)\n\
             let c: i64 = apply(let dbl, let 3)\n ret ()\n}}\n"
        );
        let text = yan_core::print(&opt(&src).expect("specializes"));
        let insts: Vec<&str> = text.lines().filter(|l| l.starts_with("inst fn")).collect();
        assert_eq!(insts.len(), 2, "{text}");
        assert!(
            insts
                .iter()
                .any(|l| l.ends_with("(let x: i64, let base: i64) -> i64 {")),
            "{text}"
        );
        assert!(text.contains("(let 1, let base)"), "{text}");
        assert!(
            text.contains("let f: Fn(let i64) -> i64 = fn [let base]"),
            "{text}"
        );
    }

    #[test]
    fn comptime_function_must_be_a_let_bound_closure() {
        let src = format!(
            "{APPLY}fn pass(let g: Fn(let i64) -> i64) -> i64 {{\n\
             let r: i64 = apply(let g, let 1)\n ret r\n}}\n"
        );
        assert!(matches!(opt(&src), Err(Error::Unsupported(m)) if m.contains("let-bound closure")));
    }
}
