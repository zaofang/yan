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
        let mut w = Walk::new(&none, &generic);
        w.item(&mut it.kind);
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
        let env: HashMap<String, Type> = f
            .tparams
            .iter()
            .zip(&args)
            .map(|(p, a)| match a {
                TArg::Type(t) => (p.name.clone(), t.clone()),
                TArg::Nat(_) => unreachable!("rejected in `add`"),
            })
            .collect();
        let mut kind = ItemKind::Inst(Inst {
            args,
            def: FnDef {
                tparams: Vec::new(),
                ..f.clone()
            },
        });
        let mut w = Walk::new(&env, &generic);
        w.item(&mut kind);
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
    !f.tparams.is_empty() && f.body.is_some()
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
            if f.tparams.iter().any(|t| t.comptime || t.kind != Kind::Type) {
                return Err(Error::Unsupported("a comptime type parameter".into()));
            }
            if f.tparams.len() != args.len() {
                return Err(Error::Unsupported(format!(
                    "call of `{name}` without its type arguments"
                )));
            }
            let text = args
                .iter()
                .map(|a| match a {
                    TArg::Type(t) => Ok(yan_core::type_text(t)),
                    TArg::Nat(_) => Err(Error::Unsupported("a Nat type argument".into())),
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
    env: &'a HashMap<String, Type>,
    generic: &'a HashMap<String, usize>,
    found: Vec<(usize, Vec<TArg>, Span)>,
}

impl<'a> Walk<'a> {
    fn new(env: &'a HashMap<String, Type>, generic: &'a HashMap<String, usize>) -> Self {
        Walk {
            env,
            generic,
            found: Vec::new(),
        }
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
                    && let Some(c) = self.env.get(p)
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
            if let TArg::Type(t) = a {
                self.ty(t);
            }
        }
    }

    fn call(&mut self, c: &mut Call) {
        self.targs(&mut c.targs);
        if let Some(&def) = self.generic.get(&c.path.join(".")) {
            self.found.push((def, c.targs.clone(), c.span));
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
            StmtKind::With { body, .. } | StmtKind::Unsafe(body) => self.block(body),
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
}
