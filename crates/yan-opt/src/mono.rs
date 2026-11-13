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
        let mut w = Walk::new(&none, &generic, &comptime_at);
        w.item(&mut it.kind);
        w.finish()?;
        mono.add(w.found, 1, &hashes)?;
        items.push(it);
    }
    // Instances are built in the order they are first reached; building one can add more.
    let mut done = 0;
    while done < mono.insts.len() {
        let (def, args, depth) = mono.insts[done].clone();
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
        bind_comptime_values(&mut def, &args[f.tparams.len()..], it.span);
        let mut kind = ItemKind::Inst(Inst { args, def });
        let mut w = Walk::new(&env, &generic, &comptime_at);
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
fn bind_comptime_values(def: &mut FnDef, values: &[TArg], span: Span) {
    let mut binds = Vec::new();
    let mut vals = values.iter();
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
    if let Some(b) = &mut def.body {
        b.stmts.splice(0..0, binds);
    }
}

struct Mono<'a> {
    file: &'a str,
    m: &'a Module,
    keys: HashMap<Key, usize>,
    /// Each instance: its generic item, its arguments and its nesting depth.
    insts: Vec<(usize, Vec<TArg>, usize)>,
}

impl Mono<'_> {
    fn add(
        &mut self,
        found: Vec<(usize, Vec<TArg>, Span)>,
        depth: usize,
        hashes: &[hash::Hashed],
    ) -> Result<(), Error> {
        for (def, args, at) in found {
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
            self.insts.push((def, args, depth));
        }
        Ok(())
    }
}

/// Replaces type parameters by `env` and collects the calls that name generic fns.
struct Walk<'a> {
    env: &'a HashMap<String, TArg>,
    generic: &'a HashMap<String, usize>,
    /// The `comptime` value parameter indexes of each generic item.
    comptime_at: &'a HashMap<usize, Vec<usize>>,
    found: Vec<(usize, Vec<TArg>, Span)>,
    err: Option<Error>,
}

impl<'a> Walk<'a> {
    fn new(
        env: &'a HashMap<String, TArg>,
        generic: &'a HashMap<String, usize>,
        comptime_at: &'a HashMap<usize, Vec<usize>>,
    ) -> Self {
        Walk {
            env,
            generic,
            comptime_at,
            found: Vec::new(),
            err: None,
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
            Param::Value { ty, .. } => self.ty(ty),
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
        // A `comptime` argument moves from the call's arguments to its type arguments.
        let at = &self.comptime_at[&def];
        let mut values = Vec::new();
        for &i in at {
            match c.args.get(i) {
                Some(Arg::Value(_, Atom::Lit(Literal::Int(n), _), _)) => {
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
        c.targs.extend(values);
        self.found.push((def, c.targs.clone(), c.span));
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
            StmtKind::Let { ty, rhs, .. } => {
                self.ty(ty);
                self.rhs(rhs);
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
}
