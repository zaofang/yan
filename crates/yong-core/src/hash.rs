//! Item identity (3.6): each item's hash is BLAKE3 over its canonical Yong text, with locals,
//! closure parameters, labels and join points named by binding order (R519), references to
//! other items of the module replaced by their hashes, and recursive groups hashed as one with
//! in-group references as indices (SQ5). The salted hash keys caches; the identity hash, the same
//! without the toolchain salt, keys provenance, patch bases and spec freeze (R348).
//!
//! SPEC-Q: 3.6 does not say how references outside the module are hashed. Assumed until the
//! query database hashes std: a path that names no item of the module is kept as written.

use std::collections::HashMap;
use std::fmt;

use crate::ast::*;
use crate::printer;

/// The toolchain salt of the salted hash.
pub const SALT: &str = concat!("yong ", env!("CARGO_PKG_VERSION"));

/// `H(group)` for an item alone; `H(group).index` for a member of a recursive group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemHash {
    pub group: blake3::Hash,
    pub index: Option<usize>,
}

impl fmt::Display for ItemHash {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.group.to_hex())?;
        match self.index {
            Some(i) => write!(f, ".{i}"),
            None => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hashed {
    /// The item's name, or a description for an `impl` or `extern` block.
    pub key: String,
    pub salted: ItemHash,
    pub identity: ItemHash,
    /// The name table (R519): `names[k]` is the written name of `%k`.
    pub names: Vec<Name>,
    /// Indices of the module's items this item references directly.
    pub refs: Vec<usize>,
}

/// The hashes of `m`'s items, in item order.
pub fn hash_module(m: &Module) -> Vec<Hashed> {
    hash_module_salted(m, SALT)
}

pub fn hash_module_salted(m: &Module, salt: &str) -> Vec<Hashed> {
    let mut keys = HashMap::new();
    for (i, it) in m.items.iter().enumerate() {
        for k in defines(it) {
            keys.insert(k, i);
        }
    }
    let canon: Vec<Canon> = m
        .items
        .iter()
        .map(|it| canonical(it, &keys, &m.path))
        .collect();
    let mut salted: Vec<Option<ItemHash>> = vec![None; canon.len()];
    let mut identity: Vec<Option<ItemHash>> = vec![None; canon.len()];
    for scc in sccs(&canon) {
        let recursive = scc.len() > 1 || canon[scc[0]].refs.contains(&scc[0]);
        if !recursive {
            let i = scc[0];
            salted[i] = Some(single(&canon[i].text, Some(salt), &salted));
            identity[i] = Some(single(&canon[i].text, None, &identity));
            continue;
        }
        // Members are ordered by their text with in-group references blanked, so the group's
        // hash does not depend on item order (law 4).
        let mut order = scc.clone();
        let blank = |i: usize| subst(&canon[i].text, |j| (!scc.contains(&j)).then(|| "?".into()));
        order.sort_by_cached_key(|&i| (blank(i), defines(&m.items[i])));
        for (table, salt) in [(&mut salted, Some(salt)), (&mut identity, None)] {
            let pos = |j: usize| order.iter().position(|&k| k == j);
            let texts: Vec<String> = order
                .iter()
                .map(|&i| {
                    subst(&canon[i].text, |j| {
                        Some(match pos(j) {
                            Some(p) => format!("@{p}"),
                            None => table[j].expect("dependencies first").to_string(),
                        })
                    })
                })
                .collect();
            let group = digest(&texts.join("\n\0\n"), salt);
            for (p, &i) in order.iter().enumerate() {
                table[i] = Some(ItemHash {
                    group,
                    index: Some(p),
                });
            }
        }
    }
    m.items
        .iter()
        .zip(canon)
        .enumerate()
        .map(|(i, (it, c))| Hashed {
            key: key(it),
            salted: salted[i].expect("every item hashed"),
            identity: identity[i].expect("every item hashed"),
            names: c.names,
            refs: c.refs,
        })
        .collect()
}

fn single(text: &str, salt: Option<&str>, table: &[Option<ItemHash>]) -> ItemHash {
    let text = subst(text, |j| {
        Some(table[j].expect("dependencies first").to_string())
    });
    ItemHash {
        group: digest(&text, salt),
        index: None,
    }
}

fn digest(text: &str, salt: Option<&str>) -> blake3::Hash {
    let mut h = blake3::Hasher::new();
    if let Some(s) = salt {
        h.update(s.as_bytes());
        h.update(b"\0");
    }
    h.update(text.as_bytes());
    h.finalize()
}

const OPEN: char = '\u{1}';
const CLOSE: char = '\u{2}';

fn marker(i: usize) -> String {
    format!("{OPEN}{i}{CLOSE}")
}

/// `text` with each reference marker replaced by `f(item)`, or kept when `f` declines.
fn subst(text: &str, f: impl Fn(usize) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(a) = rest.find(OPEN) {
        out.push_str(&rest[..a]);
        let b = rest[a..].find(CLOSE).expect("markers close") + a;
        let i: usize = rest[a + 1..b].parse().expect("marker index");
        match f(i) {
            Some(s) => out.push_str(&s),
            None => out.push_str(&rest[a..=b]),
        }
        rest = &rest[b + 1..];
    }
    out.push_str(rest);
    out
}

/// The names an item makes visible to the rest of its module.
fn defines(it: &Item) -> Vec<String> {
    match &it.kind {
        ItemKind::Fn(f) | ItemKind::Gen(f) => vec![f.path.join(".")],
        ItemKind::Record(r) => vec![r.path.join(".")],
        ItemKind::Sum(s) => vec![s.path.join(".")],
        ItemKind::Newtype(n) => vec![n.path.join(".")],
        ItemKind::Trait(t) => vec![t.path.join(".")],
        ItemKind::Const(c) => vec![c.path.join(".")],
        ItemKind::Test(t) => vec![t.path.join(".")],
        ItemKind::Impl(_) => vec![],
        ItemKind::Extern(e) => e
            .items
            .iter()
            .map(|x| match x {
                XItem::Fn { name, .. } | XItem::Type { name, .. } | XItem::Static { name, .. } => {
                    name.clone()
                }
            })
            .collect(),
    }
}

fn key(it: &Item) -> String {
    match &it.kind {
        ItemKind::Impl(i) => format!(
            "impl {} for {}",
            printer::ty(&Type::Named(i.trait_.clone())),
            printer::ty(&i.for_ty)
        ),
        ItemKind::Extern(e) => format!("extern {}", e.lib),
        _ => defines(it).join(","),
    }
}

/// Tarjan's algorithm; components come out dependencies first.
fn sccs(canon: &[Canon]) -> Vec<Vec<usize>> {
    struct T<'a> {
        canon: &'a [Canon],
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        out: Vec<Vec<usize>>,
    }
    fn visit(t: &mut T, v: usize) {
        t.index[v] = Some(t.next);
        t.low[v] = t.next;
        t.next += 1;
        t.stack.push(v);
        t.on[v] = true;
        for &w in &t.canon[v].refs {
            match t.index[w] {
                None => {
                    visit(t, w);
                    t.low[v] = t.low[v].min(t.low[w]);
                }
                Some(iw) if t.on[w] => t.low[v] = t.low[v].min(iw),
                Some(_) => {}
            }
        }
        if Some(t.low[v]) == t.index[v] {
            let mut c = Vec::new();
            loop {
                let w = t.stack.pop().expect("v is on the stack");
                t.on[w] = false;
                c.push(w);
                if w == v {
                    break;
                }
            }
            t.out.push(c);
        }
    }
    let n = canon.len();
    let mut t = T {
        canon,
        index: vec![None; n],
        low: vec![0; n],
        on: vec![false; n],
        stack: Vec::new(),
        next: 0,
        out: Vec::new(),
    };
    for v in 0..n {
        if t.index[v].is_none() {
            visit(&mut t, v);
        }
    }
    t.out
}

struct Canon {
    text: String,
    refs: Vec<usize>,
    names: Vec<Name>,
}

fn canonical(it: &Item, keys: &HashMap<String, usize>, module: &[Name]) -> Canon {
    let mut it = it.clone();
    // Docstrings are trivia (16.5).
    it.attrs.retain(|a| a.name != "doc");
    let mut cx = Cx {
        keys,
        module,
        scopes: vec![HashMap::new()],
        labels: vec![HashMap::new()],
        names: Vec::new(),
        refs: Vec::new(),
    };
    cx.item(&mut it);
    let mut refs = cx.refs;
    refs.sort_unstable();
    refs.dedup();
    Canon {
        text: printer::item(&it),
        refs,
        names: cx.names,
    }
}

/// Renames locals by binding order and marks references to items of the module.
struct Cx<'a> {
    keys: &'a HashMap<String, usize>,
    module: &'a [Name],
    /// Written name to hashed name; parameters and type parameters map to themselves.
    scopes: Vec<HashMap<Name, Name>>,
    labels: Vec<HashMap<Name, Name>>,
    names: Vec<Name>,
    refs: Vec<usize>,
}

impl Cx<'_> {
    fn fresh(&mut self, written: &Name) -> Name {
        let n = format!("%{}", self.names.len());
        self.names.push(written.clone());
        n
    }

    fn bind(&mut self, n: &mut Name) {
        let new = self.fresh(n);
        self.scopes
            .last_mut()
            .expect("a scope")
            .insert(n.clone(), new.clone());
        *n = new;
    }

    fn keep(&mut self, n: &Name) {
        self.scopes
            .last_mut()
            .expect("a scope")
            .insert(n.clone(), n.clone());
    }

    fn local(&self, n: &str) -> Option<&Name> {
        self.scopes.iter().rev().find_map(|s| s.get(n))
    }

    fn scoped<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.scopes.push(HashMap::new());
        self.labels.push(HashMap::new());
        let r = f(self);
        self.scopes.pop();
        self.labels.pop();
        r
    }

    /// A name used as a value: a local, or a reference to an item.
    fn name(&mut self, n: &mut Name) {
        if let Some(l) = self.local(n) {
            *n = l.clone();
        } else if let Some(&i) = self.keys.get(n.as_str()) {
            self.refs.push(i);
            *n = marker(i);
        }
    }

    /// A path to an item, a variant or a member of one; the longest prefix naming an item of
    /// this module is replaced by its marker.
    fn path(&mut self, p: &mut Path) {
        if p.len() == 1 && self.local(&p[0]).is_some() {
            return self.name(&mut p[0]);
        }
        if p.len() > self.module.len() && p.starts_with(self.module) {
            p.drain(..self.module.len());
        }
        for k in (1..=p.len()).rev() {
            if let Some(&i) = self.keys.get(&p[..k].join(".")) {
                self.refs.push(i);
                p.splice(..k, [marker(i)]);
                return;
            }
        }
    }

    fn item(&mut self, it: &mut Item) {
        for a in &mut it.attrs {
            for arg in a.args.iter_mut().flatten() {
                self.const_val(&mut arg.value);
            }
        }
        match &mut it.kind {
            ItemKind::Fn(f) | ItemKind::Gen(f) => self.func(f),
            ItemKind::Record(r) => self.scoped(|cx| {
                cx.tparams(&mut r.tparams);
                cx.fields(&mut r.fields);
            }),
            ItemKind::Sum(s) => self.scoped(|cx| {
                cx.tparams(&mut s.tparams);
                for v in &mut s.variants {
                    cx.fields(v.fields.iter_mut().flatten());
                }
            }),
            ItemKind::Newtype(n) => self.scoped(|cx| {
                cx.tparams(&mut n.tparams);
                cx.ty(&mut n.ty);
                if let Some(e) = &mut n.pred {
                    cx.expr(e);
                }
            }),
            ItemKind::Trait(t) => self.scoped(|cx| {
                cx.tparams(&mut t.tparams);
                for a in &t.assoc {
                    cx.keep(a);
                }
                cx.keep(&"Self".to_owned());
                t.sigs.iter_mut().for_each(|f| cx.func(f));
                if let Some(b) = &mut t.derived {
                    cx.block(b);
                }
            }),
            ItemKind::Impl(i) => self.scoped(|cx| {
                cx.path(&mut i.trait_.path);
                cx.targs(&mut i.trait_.args);
                cx.ty(&mut i.for_ty);
                for (n, t) in &mut i.assoc {
                    cx.keep(n);
                    cx.ty(t);
                }
                i.fns.iter_mut().for_each(|f| cx.func(f));
            }),
            ItemKind::Const(c) => {
                self.ty(&mut c.ty);
                self.const_val(&mut c.value);
            }
            ItemKind::Test(t) => self.scoped(|cx| cx.block(&mut t.body)),
            ItemKind::Extern(e) => {
                for x in &mut e.items {
                    match x {
                        XItem::Fn { params, ret, .. } => {
                            for p in params {
                                match p {
                                    XParam::Value { ty, .. } => self.ty(ty),
                                    XParam::Callback { ty, .. } => self.fn_type(ty),
                                }
                            }
                            self.ty(ret);
                        }
                        XItem::Type { drop, .. } => self.path(drop),
                        XItem::Static { ty, .. } => self.ty(ty),
                    }
                }
            }
        }
    }

    fn func(&mut self, f: &mut FnDef) {
        self.scoped(|cx| {
            cx.tparams(&mut f.tparams);
            for p in &mut f.params {
                match p {
                    Param::Value { name, ty, .. } => {
                        cx.ty(ty);
                        cx.keep(name);
                    }
                    Param::Cap { name, ty } => {
                        cx.named(ty);
                        cx.keep(name);
                    }
                }
            }
            cx.ty(&mut f.ret);
            for n in &mut f.from {
                cx.name(n);
            }
            if let Some(t) = &mut f.fail {
                cx.ty(t);
            }
            for c in &mut f.contracts {
                match c {
                    Contract::Requires(e) | Contract::Ensures(e) | Contract::Example(e) => {
                        cx.expr(e)
                    }
                    Contract::Decreases(es) => es.iter_mut().for_each(|e| cx.expr(e)),
                }
            }
            if let Some(b) = &mut f.body {
                cx.block(b);
            }
        })
    }

    fn tparams(&mut self, ps: &mut [TParam]) {
        for p in ps.iter() {
            self.keep(&p.name);
        }
        for p in ps {
            p.bounds.iter_mut().for_each(|b| self.path(b));
        }
    }

    fn fields<'f>(&mut self, fs: impl IntoIterator<Item = &'f mut Field>) {
        for f in fs {
            self.ty(&mut f.ty);
            if let Some(v) = &mut f.default {
                self.const_val(v);
            }
        }
    }

    fn const_val(&mut self, c: &mut ConstVal) {
        match c {
            ConstVal::Lit(_) => {}
            ConstVal::Path { path, fields } => {
                self.path(path);
                for (_, v) in fields.iter_mut().flatten() {
                    self.const_val(v);
                }
            }
            ConstVal::List(vs) | ConstVal::Tuple(vs) => {
                vs.iter_mut().for_each(|v| self.const_val(v))
            }
        }
    }

    fn named(&mut self, n: &mut Named) {
        self.path(&mut n.path);
        self.targs(&mut n.args);
    }

    fn targs(&mut self, args: &mut [TArg]) {
        for a in args {
            if let TArg::Type(t) = a {
                self.ty(t);
            }
        }
    }

    fn fn_type(&mut self, f: &mut FnType) {
        f.params.iter_mut().for_each(|(_, t)| self.ty(t));
        self.ty(&mut f.ret);
        if let Some(t) = &mut f.fail {
            self.ty(t);
        }
    }

    fn ty(&mut self, t: &mut Type) {
        match t {
            Type::Named(n) | Type::Dyn(n) => self.named(n),
            Type::Tuple(ts) | Type::Union(ts) => ts.iter_mut().for_each(|t| self.ty(t)),
            Type::Fn(f) => self.fn_type(f),
            Type::Refined { name, ty, pred } => {
                self.ty(ty);
                self.scoped(|cx| {
                    cx.bind(name);
                    cx.expr(pred);
                });
            }
        }
    }

    fn atom(&mut self, a: &mut Atom) {
        if let Atom::Name(n, _) = a {
            self.name(n);
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match e {
            Expr::Atom(a) => self.atom(a),
            Expr::Call(c) => self.call(c),
        }
    }

    fn call(&mut self, c: &mut Call) {
        self.path(&mut c.path);
        self.targs(&mut c.targs);
        for a in &mut c.args {
            if let Arg::Value(_, a, _) = a {
                self.atom(a);
            }
        }
    }

    fn place(&mut self, p: &mut Place) {
        self.name(&mut p.root);
        for pr in &mut p.projs {
            if let Proj::Index(a) = pr {
                self.atom(a);
            }
        }
    }

    fn ctor(&mut self, c: &mut Ctor) {
        match c {
            Ctor::Named {
                path,
                targs,
                fields,
            } => {
                self.path(path);
                self.targs(targs);
                fields.iter_mut().for_each(|(_, a)| self.atom(a));
            }
            Ctor::Tuple(atoms) => atoms.iter_mut().for_each(|a| self.atom(a)),
        }
    }

    fn rhs(&mut self, r: &mut Rhs) {
        match r {
            Rhs::Atom(a) => self.atom(a),
            Rhs::Call(c) | Rhs::Try(c) => self.call(c),
            Rhs::Propagate { call, via } => {
                self.call(call);
                if let Some(p) = via {
                    self.path(p);
                }
            }
            Rhs::Ctor(c) => self.ctor(c),
            Rhs::Proj(p) | Rhs::Copy(p) | Rhs::Move(p) => self.place(p),
            Rhs::Closure(c) => {
                for (_, n, _) in &mut c.captures {
                    self.name(n);
                }
                self.scoped(|cx| {
                    for p in &mut c.params {
                        cx.param(p);
                    }
                    cx.ty(&mut c.ret);
                    if let Some(t) = &mut c.fail {
                        cx.ty(t);
                    }
                    cx.block(&mut c.body);
                });
            }
        }
    }

    /// A closure or join-point parameter: a local.
    fn param(&mut self, p: &mut Param) {
        match p {
            Param::Value { name, ty, .. } => {
                self.ty(ty);
                self.bind(name);
            }
            Param::Cap { name, ty } => {
                self.named(ty);
                self.keep(name);
            }
        }
    }

    fn pat(&mut self, p: &mut Pat) {
        match p {
            Pat::Wild | Pat::Lit(_) | Pat::Range(..) => {}
            Pat::Bind(n) => self.bind(n),
            Pat::Ctor { path, fields } => {
                self.path(path);
                for (_, p) in fields.iter_mut().flat_map(|(fs, _)| fs.iter_mut()) {
                    self.pat(p);
                }
            }
            Pat::Tuple(ps) => ps.iter_mut().for_each(|p| self.pat(p)),
            // Every alternative binds the same names; they get the first alternative's numbers.
            Pat::Or(ps) => {
                let Some((first, rest)) = ps.split_first_mut() else {
                    return;
                };
                self.pat(first);
                for p in rest {
                    self.rename_bound(p);
                }
            }
            Pat::Slice { elems, rest } => {
                elems.iter_mut().for_each(|p| self.pat(p));
                if let Some(Some(n)) = rest {
                    self.bind(n);
                }
            }
        }
    }

    /// Renames the bindings of a later or-pattern alternative to the numbers already given.
    fn rename_bound(&mut self, p: &mut Pat) {
        match p {
            Pat::Bind(n) => self.name(n),
            Pat::Ctor { path, fields } => {
                self.path(path);
                for (_, p) in fields.iter_mut().flat_map(|(fs, _)| fs.iter_mut()) {
                    self.rename_bound(p);
                }
            }
            Pat::Tuple(ps) | Pat::Or(ps) => ps.iter_mut().for_each(|p| self.rename_bound(p)),
            Pat::Slice { elems, rest } => {
                elems.iter_mut().for_each(|p| self.rename_bound(p));
                if let Some(Some(n)) = rest {
                    self.name(n);
                }
            }
            Pat::Wild | Pat::Lit(_) | Pat::Range(..) => {}
        }
    }

    fn label(&mut self, n: &mut Name) {
        if let Some(l) = self.labels.iter().rev().find_map(|s| s.get(n.as_str())) {
            *n = l.clone();
        }
    }

    fn block(&mut self, b: &mut Block) {
        self.scoped(|cx| b.stmts.iter_mut().for_each(|s| cx.stmt(s)));
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match &mut s.kind {
            StmtKind::Let { pat, ty, rhs, .. } => {
                self.ty(ty);
                self.rhs(rhs);
                self.pat(pat);
            }
            StmtKind::Assign { place, rhs, .. } => {
                self.rhs(rhs);
                self.place(place);
            }
            StmtKind::If { cond, then, els } | StmtKind::ComptimeIf { cond, then, els } => {
                self.atom(cond);
                self.block(then);
                if let Some(b) = els {
                    self.block(b);
                }
            }
            StmtKind::Match { scrutinee, arms } => {
                self.atom(scrutinee);
                for (p, b) in arms {
                    self.scoped(|cx| {
                        cx.pat(p);
                        cx.block(b);
                    });
                }
            }
            StmtKind::Loop {
                label,
                invariants,
                decreases,
                body,
            } => self.scoped(|cx| {
                let new = cx.fresh(label);
                cx.labels
                    .last_mut()
                    .expect("a scope")
                    .insert(label.clone(), new.clone());
                *label = new;
                invariants.iter_mut().for_each(|e| cx.expr(e));
                decreases.iter_mut().for_each(|e| cx.expr(e));
                cx.block(body);
            }),
            StmtKind::Break { label, value } => {
                self.label(label);
                if let Some(a) = value {
                    self.atom(a);
                }
            }
            StmtKind::Continue { label } => self.label(label),
            StmtKind::Jp { name, params, body } => {
                self.bind(name);
                self.scoped(|cx| {
                    params.iter_mut().for_each(|p| cx.param(p));
                    cx.block(body);
                });
            }
            StmtKind::Jump { name, args } => {
                self.name(name);
                args.iter_mut().for_each(|a| self.atom(a));
            }
            StmtKind::Ret(a) | StmtKind::Fail(a) | StmtKind::Yield(a) => self.atom(a),
            StmtKind::Become(c) => self.call(c),
            StmtKind::With {
                name,
                handler,
                body,
                ..
            } => {
                self.atom(handler);
                self.scoped(|cx| {
                    cx.keep(name);
                    cx.block(body);
                });
            }
            StmtKind::Par { blocks, .. } => blocks.iter_mut().for_each(|b| self.block(b)),
            StmtKind::ComptimeFor {
                name,
                ty,
                iter,
                body,
            } => {
                self.ty(ty);
                self.atom(iter);
                self.scoped(|cx| {
                    cx.bind(name);
                    cx.block(body);
                });
            }
            StmtKind::Assert(e) => self.expr(e),
            StmtKind::Unreachable => {}
            StmtKind::Unsafe(b) => self.block(b),
            StmtKind::Asm { operands, .. } => {
                for o in operands {
                    if let AsmOperand::Reg { value, .. } = o {
                        self.atom(value);
                    }
                }
            }
            StmtKind::Drop(n) => self.name(n),
            StmtKind::Reuse { name, ctor } => {
                self.name(name);
                self.ctor(ctor);
            }
            StmtKind::Rhs(r) => self.rhs(r),
        }
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    fn hashes(src: &str) -> Vec<(String, String, String)> {
        let (m, d) = crate::parse("t.yong", src);
        assert!(d.is_empty(), "{d:?}");
        hash_module(&m.unwrap())
            .into_iter()
            .map(|h| (h.key, h.salted.to_string(), h.identity.to_string()))
            .collect()
    }

    fn sorted(src: &str) -> Vec<(String, String, String)> {
        let mut h = hashes(src);
        h.sort();
        h
    }

    const BASE: &str = "module m

fn sq(let x: i64) -> i64 {
    let y: i64 = mul_i64(let x, let x)
    ret y
}

fn main(cap io: Io) -> Unit {
    let a: i64 = sq(let 3)
    ret ()
}
";

    #[test]
    fn local_names_do_not_change_hashes() {
        let renamed = BASE
            .replace("let y", "let total")
            .replace("ret y", "ret total");
        assert_eq!(hashes(BASE), hashes(&renamed));
    }

    #[test]
    fn parameter_names_change_hashes() {
        let renamed = BASE.replace("let x: i64) -> i64", "let z: i64) -> i64");
        let renamed = renamed.replace("let x, let x", "let z, let z");
        assert_ne!(hashes(BASE)[0], hashes(&renamed)[0]);
    }

    #[test]
    fn item_order_and_comments_do_not_change_hashes() {
        let (head, rest) = BASE.split_once("\n\n").unwrap();
        let (sq, main) = rest.split_once("\n\n").unwrap();
        let swapped = format!("{head}\n\n{main}\n-- a comment\n\n{sq}");
        assert_eq!(sorted(BASE), sorted(&swapped));
    }

    #[test]
    fn a_body_change_reaches_its_callers() {
        let changed = BASE.replace("mul_i64", "add_i64");
        let (a, b) = (hashes(BASE), hashes(&changed));
        assert_ne!(a[0], b[0]);
        assert_ne!(a[1], b[1], "main names sq by its hash");
    }

    #[test]
    fn salted_and_identity_hashes_differ() {
        let h = &hashes(BASE)[0];
        assert_ne!(h.1, h.2);
        let m = crate::parse("t.yong", BASE).0.unwrap();
        let other = hash_module_salted(&m, "yong 99.0.0");
        assert_eq!(other[0].identity.to_string(), h.2);
        assert_ne!(other[0].salted.to_string(), h.1);
    }

    const MUTUAL: &str = "module m

fn even(let n: i64) -> bool {
    let z: bool = eq_i64(let n, let 0)
    if z {
        ret true
    }
    let k: i64 = sub_i64(let n, let 1)
    let r: bool = odd(let k)
    ret r
}

fn odd(let n: i64) -> bool {
    let z: bool = eq_i64(let n, let 0)
    if z {
        ret false
    }
    let k: i64 = sub_i64(let n, let 1)
    let r: bool = even(let k)
    ret r
}
";

    #[test]
    fn recursive_items_hash_as_one_group() {
        let h = hashes(MUTUAL);
        let (g0, i0) = h[0].1.split_once('.').unwrap();
        let (g1, i1) = h[1].1.split_once('.').unwrap();
        assert_eq!(g0, g1);
        assert_ne!(i0, i1);
        let (head, rest) = MUTUAL.split_once("\n\n").unwrap();
        let (even, odd) = rest.split_once("\n\n").unwrap();
        assert_eq!(sorted(MUTUAL), sorted(&format!("{head}\n\n{odd}\n{even}")));
    }

    #[test]
    fn the_name_table_keeps_written_names() {
        let (m, _) = crate::parse("t.yong", BASE);
        let h = hash_module(&m.unwrap());
        assert_eq!(h[0].names, ["y"]);
        assert_eq!(h[1].names, ["a"]);
    }
}
