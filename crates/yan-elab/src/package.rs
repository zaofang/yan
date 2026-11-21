//! Names across the modules of one package (RFC 0084 D4, slice 2a). The module environment is the
//! package's top-level modules; a path whose first segments name one resolves to that module's item.
//! The referenced items, and the items they reference, are copied into the module being elaborated
//! under their qualified names (`fees.bps`), as std items are pulled into their callers, so check,
//! MIR, the interpreter and native see one module. Variant names stay bare: they are wire tags.

use std::collections::{BTreeMap, HashMap, HashSet};

use yan_diag::Span;
use yan_hir as j;

/// Spans of pulled items are moved this many lines down, so a diagnostic about a copied item, which
/// its own module's check already reported, is told apart from the module's own and dropped.
pub const PULLED: u32 = 10_000_000;

/// The package as one module sees it: its sibling modules by dotted path, and their HIR on demand.
pub struct PackageEnv<'a> {
    /// The module being elaborated, so an item that names it resolves to the module's own names.
    pub this: String,
    /// The package's own modules and, for each dependency, its modules under the local name:
    /// `http` for the dependency's `lib`, `http.client` for its `client` (RFC 0084 D4).
    pub modules: Vec<String>,
    /// The dependencies, so a name written inside one of their modules resolves against its own.
    pub deps: Vec<DepScope>,
    pub load: &'a dyn Fn(&str) -> Option<j::Module>,
}

/// A dependency as the package sees it: its local root name and its module paths relative to its
/// own source root (`lib` is its root facade).
pub struct DepScope {
    pub local: String,
    pub modules: Vec<String>,
}

impl DepScope {
    /// The name under which the package reaches the dependency module `rel`.
    fn global(&self, rel: &str) -> String {
        if rel == "lib" {
            self.local.clone()
        } else {
            format!("{}.{rel}", self.local)
        }
    }
}

pub(crate) struct LinkError {
    pub what: String,
    pub code: Option<&'static str>,
    pub at: Option<Span>,
}

/// The sibling modules `m` names in a path, which the driver checks before elaborating `m`.
pub fn referenced(m: &j::Module, modules: &[String]) -> Vec<String> {
    let mut m = m.clone();
    let env = PackageEnv {
        this: String::new(),
        modules: modules.to_vec(),
        deps: Vec::new(),
        load: &|_| None,
    };
    let mut l = Linker::new(&env);
    l.items(&mut m.items);
    l.used.into_iter().collect()
}

/// Copy into `m` the items of the sibling modules it references.
pub(crate) fn link(m: &mut j::Module, env: &PackageEnv) -> Result<(), LinkError> {
    let mut l = Linker::new(env);
    let own: HashSet<&str> = m
        .items
        .iter()
        .filter_map(|it| match &it.kind {
            j::ItemKind::Type(t) => Some(t),
            _ => None,
        })
        .flat_map(|t| match &t.rhs {
            j::TypeRhs::Sum(vs) => vs.iter().map(|v| v.name.as_str()).collect(),
            _ => Vec::new(),
        })
        .collect();
    l.variants = own
        .into_iter()
        .map(|v| (v.to_owned(), String::new()))
        .collect();
    l.own_items = m.items.clone();
    l.items(&mut m.items);
    let mut pulled = Vec::new();
    while let Some((module, name)) = l.queue.pop() {
        if let Some(it) = l.pull(&module, &name)? {
            pulled.push(it);
        }
    }
    if let Some(e) = l.error {
        return Err(e);
    }
    pulled.append(&mut m.items);
    m.items = pulled;
    Ok(())
}

/// The visibility of the type, record, function or constant `item` (or sum variant) of `items`.
fn item_vis(items: &[j::Item], item: &str) -> Option<j::Vis> {
    items.iter().find_map(|it| match &it.kind {
        j::ItemKind::Type(t) => match &t.rhs {
            j::TypeRhs::Sum(vs) if t.name == item || vs.iter().any(|v| v.name == item) => {
                Some(it.vis)
            }
            _ => (t.name == item).then_some(it.vis),
        },
        j::ItemKind::Func(_) | j::ItemKind::Record(_) | j::ItemKind::Const { .. }
            if it.kind.name() == item =>
        {
            Some(it.vis)
        }
        _ => None,
    })
}

struct Linker<'a> {
    env: &'a PackageEnv<'a>,
    /// The module paths a name may start with, as written, to the module they stand for.
    names: HashMap<String, String>,
    heads: HashSet<String>,
    mods: HashMap<String, Option<j::Module>>,
    /// Modules named by a path.
    used: std::collections::BTreeSet<String>,
    queue: Vec<(String, String)>,
    done: HashSet<(String, String)>,
    /// The module whose item is being rewritten; its bare names are its own.
    at: Option<String>,
    own: HashSet<String>,
    own_types: HashSet<String>,
    /// The module being elaborated, for the visibility of its own items.
    own_items: Vec<j::Item>,
    /// Set while the signature of a `pub` item is walked, with the item's name (E336).
    exported: Option<String>,
    locals: HashSet<String>,
    tparams: HashSet<String>,
    /// Variant name to the module that declares it ("" for the module being elaborated).
    variants: BTreeMap<String, String>,
    shift: bool,
    error: Option<LinkError>,
    /// The statement or item being walked, where a type or pattern has no span of its own.
    site: Span,
}

impl<'a> Linker<'a> {
    fn new(env: &'a PackageEnv<'a>) -> Self {
        Linker {
            env,
            names: env.modules.iter().map(|m| (m.clone(), m.clone())).collect(),
            heads: env
                .modules
                .iter()
                .filter_map(|m| m.split('.').next().map(str::to_owned))
                .collect(),
            mods: HashMap::new(),
            used: Default::default(),
            queue: Vec::new(),
            done: HashSet::new(),
            at: None,
            own: HashSet::new(),
            own_types: HashSet::new(),
            own_items: Vec::new(),
            exported: None,
            locals: HashSet::new(),
            tparams: HashSet::new(),
            variants: BTreeMap::new(),
            shift: false,
            error: None,
            site: Span::default(),
        }
    }

    fn module(&mut self, name: &str) -> Option<&j::Module> {
        if !self.mods.contains_key(name) {
            let m = (self.env.load)(name);
            self.mods.insert(name.to_owned(), m);
        }
        self.mods[name].as_ref()
    }

    fn fail(&mut self, what: String) {
        self.error.get_or_insert(LinkError {
            what,
            code: None,
            at: None,
        });
    }

    /// E287 when the module being elaborated names a private item of another module (RFC 0084 D3).
    /// A copied item's own module reports its own references.
    fn check_visible(&mut self, module: &str, item: &str, at: Span) {
        if self.at.is_some() || module == self.env.this {
            return;
        }
        let vis = self.module(module).and_then(|m| item_vis(&m.items, item));
        let foreign = self.dep_of(module).is_some();
        if foreign && vis.is_some_and(|v| v != j::Vis::Pub) {
            self.error.get_or_insert(LinkError {
                what: format!(
                    "`{module}.{item}` is not `pub` in its package; only `pub` items are reachable from a dependent"
                ),
                code: Some("E287"),
                at: Some(at),
            });
        } else if vis == Some(j::Vis::Private) {
            self.error.get_or_insert(LinkError {
                what: format!(
                    "`{module}.{item}` is private to module `{module}`; declare it `pkg` to share it within the package or `pub` to export it"
                ),
                code: Some("E287"),
                at: Some(at),
            });
        }
    }

    /// E336 when a `pub` item's signature names an item that is not `pub` (RFC 0084 D3): a
    /// dependent could not name the type.
    fn check_exported(&mut self, module: &str, item: &str) {
        let Some(owner) = self.exported.clone() else {
            return;
        };
        let vis = if module == self.env.this {
            item_vis(&self.own_items, item)
        } else {
            self.module(module).and_then(|m| item_vis(&m.items, item))
        };
        let how = match vis {
            Some(j::Vis::Pkg) => "`pkg`",
            Some(j::Vis::Private) => "private",
            _ => return,
        };
        self.error.get_or_insert(LinkError {
            what: format!(
                "`{item}` is {how} but the signature of `pub` `{owner}` names it; a public signature may only name `pub` items, so declare `{item}` `pub`"
            ),
            code: Some("E336"),
            at: Some(self.site),
        });
    }

    /// The module and item a path of 2 or more names stands for, when its head is not a local.
    /// A `pub use` of a module (RFC 0084 D5) is followed to that module, so `lib.m.item` reaches
    /// `a.b.item`.
    fn split<'p>(&mut self, path: &'p [String]) -> Option<(String, &'p [String])> {
        if path.len() < 2 || !self.heads.contains(&path[0]) || self.locals.contains(&path[0]) {
            return None;
        }
        let (mut module, mut rest) = (1..path.len())
            .rev()
            .map(|k| (path[..k].join("."), &path[k..]))
            .find_map(|(m, rest)| Some((self.names.get(&m)?.clone(), rest)))?;
        // The hop limit stops a cycle of module aliases.
        for _ in 0..16 {
            let Some((head, tail)) = rest.split_first() else {
                break;
            };
            match self.module_alias(&module, head) {
                Some(target) => (module, rest) = (target, tail),
                None => break,
            }
        }
        Some((module, rest))
    }

    /// The module that `pub use` in `module` makes `name` an alias of, when its path names a module.
    fn module_alias(&mut self, module: &str, name: &str) -> Option<String> {
        let path = self
            .module(module)?
            .reexports
            .iter()
            .find(|r| r.name == name)?
            .path
            .join(".");
        match self.dep_of(module) {
            Some(dep) => dep.modules.contains(&path).then(|| dep.global(&path)),
            None => self.env.modules.contains(&path).then_some(path),
        }
    }

    /// The module and item that `pub use` in `module` makes of `item` (RFC 0084 D5), in the
    /// namespace of the package being elaborated. A re-export is an alias: the target is the item.
    fn reexported(&mut self, module: &str, name: &str, at: Span) -> Option<(String, String)> {
        let path = self
            .module(module)?
            .reexports
            .iter()
            .find(|r| r.name == name)?
            .path
            .clone();
        let (item, parent) = path.split_last()?;
        let rel = parent.join(".");
        let target = match self.dep_of(module) {
            Some(dep) => dep.modules.contains(&rel).then(|| dep.global(&rel)),
            None => self.env.modules.contains(&rel).then_some(rel),
        };
        let found = target.filter(|t| {
            let items = if *t == self.env.this {
                Some(&self.own_items[..])
            } else {
                self.module(t).map(|m| &m.items[..])
            };
            items.is_some_and(|i| item_vis(i, item).is_some())
        });
        if found.is_none() {
            let shown = path.join(".");
            self.error.get_or_insert(LinkError {
                what: format!(
                    "`pub use {shown}` in `{module}` names no item of the package; the target of a re-export must be an item that exists"
                ),
                code: Some("E337"),
                at: Some(at),
            });
        }
        Some((found?, item.clone()))
    }

    /// The expression for `item` of `module`: a bare variant, or the qualified name.
    fn resolve(&mut self, module: &str, item: &str, at: Span) -> String {
        let (mut module, mut item) = (module.to_owned(), item.to_owned());
        // A chain of re-exports is followed to its item; the hop limit stops a cycle.
        for _ in 0..16 {
            match self.reexported(&module, &item, at) {
                Some(t) => (module, item) = t,
                None => break,
            }
        }
        let (module, item) = (module.as_str(), item.as_str());
        if module == self.env.this {
            return item.to_owned();
        }
        self.check_visible(module, item, at);
        self.used.insert(module.to_owned());
        let sum = self.module(module).and_then(|m| {
            m.items.iter().find_map(|it| match &it.kind {
                j::ItemKind::Type(t) => match &t.rhs {
                    j::TypeRhs::Sum(vs) if vs.iter().any(|v| v.name == item) => {
                        Some(t.name.clone())
                    }
                    _ => None,
                },
                _ => None,
            })
        });
        if let Some(sum) = sum {
            self.declare(item, module);
            self.need(module, &sum);
            return item.to_owned();
        }
        self.need(module, item);
        format!("{module}.{item}")
    }

    fn declare(&mut self, variant: &str, module: &str) {
        match self.variants.get(variant) {
            Some(m) if m != module => {
                let first = if m.is_empty() { "this module" } else { m };
                let what = format!(
                    "variant name `{variant}` is declared by `{first}` and by `{module}`; variant names are wire tags and stay unique within a package"
                );
                self.fail(what);
            }
            _ => {
                self.variants.insert(variant.to_owned(), module.to_owned());
            }
        }
    }

    fn need(&mut self, module: &str, item: &str) {
        if self.done.insert((module.to_owned(), item.to_owned())) {
            self.queue.push((module.to_owned(), item.to_owned()));
        }
    }

    /// The dependency that holds `module`, by its local name.
    fn dep_of(&self, module: &str) -> Option<&'a DepScope> {
        let env: &'a PackageEnv<'a> = self.env;
        env.deps.iter().find(|d| {
            module == d.local
                || module
                    .strip_prefix(d.local.as_str())
                    .is_some_and(|r| r.starts_with('.'))
        })
    }

    /// Inside a dependency's module a path starts at the dependency's own modules. Returns the
    /// scope to restore.
    fn enter(&mut self, module: &str) -> Option<(HashMap<String, String>, HashSet<String>)> {
        let dep = self.dep_of(module)?;
        let names = dep
            .modules
            .iter()
            .map(|m| (m.clone(), dep.global(m)))
            .collect();
        let heads = dep
            .modules
            .iter()
            .filter_map(|m| m.split('.').next().map(str::to_owned))
            .collect();
        Some((
            std::mem::replace(&mut self.names, names),
            std::mem::replace(&mut self.heads, heads),
        ))
    }

    /// The copy of `item` of `module`, rewritten to qualified names.
    fn pull(&mut self, module: &str, item: &str) -> Result<Option<j::Item>, LinkError> {
        let Some(m) = self.module(module) else {
            return Ok(None);
        };
        let Some(found) = m
            .items
            .iter()
            .find(|it| {
                matches!(
                    &it.kind,
                    j::ItemKind::Func(_)
                        | j::ItemKind::Record(_)
                        | j::ItemKind::Type(_)
                        | j::ItemKind::Const { .. }
                ) && it.kind.name() == item
            })
            .cloned()
        else {
            return Ok(None);
        };
        let mut own = HashSet::new();
        let mut own_types = HashSet::new();
        for it in &m.items {
            match &it.kind {
                j::ItemKind::Func(f) => {
                    own.insert(f.name.clone());
                }
                j::ItemKind::Const { name, .. } => {
                    own.insert(name.clone());
                }
                j::ItemKind::Record(r) => {
                    own.insert(r.name.clone());
                    own_types.insert(r.name.clone());
                }
                j::ItemKind::Type(t) => {
                    own_types.insert(t.name.clone());
                }
                _ => {}
            }
        }
        let mut it = found;
        self.at = Some(module.to_owned());
        self.own = own;
        self.own_types = own_types;
        self.shift = true;
        let outer = self.enter(module);
        self.items(std::slice::from_mut(&mut it));
        if let Some((names, heads)) = outer {
            self.names = names;
            self.heads = heads;
        }
        self.shift = false;
        self.at = None;
        let q = format!("{module}.{item}");
        match &mut it.kind {
            j::ItemKind::Func(f) => {
                f.name = q;
                if let Some(b) = &mut f.body {
                    b.contracts.retain(|c| !matches!(c, j::Contract::Ex { .. }));
                }
            }
            j::ItemKind::Record(r) => r.name = q,
            j::ItemKind::Type(t) => {
                if let j::TypeRhs::Sum(vs) = &t.rhs {
                    for v in vs {
                        self.declare(&v.name, module);
                    }
                }
                t.name = q
            }
            j::ItemKind::Const { name, .. } => *name = q,
            _ => {}
        }
        it.vis = j::Vis::Private;
        Ok(Some(it))
    }

    // ---- the walk -------------------------------------------------------------------------

    fn sp(&self, s: &mut Span) {
        if self.shift {
            s.line += PULLED;
            s.end_line += PULLED;
        }
    }

    fn items(&mut self, items: &mut [j::Item]) {
        for it in items {
            self.sp(&mut it.span);
            self.sp(&mut it.name_span);
            self.site = it.name_span;
            self.locals.clear();
            self.tparams.clear();
            self.exported = (it.vis == j::Vis::Pub
                && self.at.is_none()
                && matches!(
                    it.kind,
                    j::ItemKind::Func(_)
                        | j::ItemKind::Record(_)
                        | j::ItemKind::Const { .. }
                        | j::ItemKind::Type(_)
                ))
            .then(|| it.kind.name().to_owned());
            match &mut it.kind {
                j::ItemKind::Func(f) => self.func(f),
                j::ItemKind::Record(r) => {
                    self.tps(&r.tparams);
                    for f in &mut r.fields {
                        self.field(f);
                    }
                }
                j::ItemKind::Type(t) => {
                    self.tps(&t.tparams);
                    match &mut t.rhs {
                        j::TypeRhs::Sum(vs) => {
                            for v in vs {
                                for f in &mut v.fields {
                                    self.field(f);
                                }
                            }
                        }
                        j::TypeRhs::Alias(ts) => ts.iter_mut().for_each(|t| self.ty(t)),
                        j::TypeRhs::Newtype(t) => self.ty(t),
                        j::TypeRhs::Caps(_) => {}
                    }
                    if let Some(w) = &mut t.where_ {
                        self.exported = None;
                        self.expr(w);
                    }
                }
                j::ItemKind::Const { ty, value, .. } => {
                    self.ty(ty);
                    self.exported = None;
                    self.expr(value);
                }
                j::ItemKind::Impl(i) => {
                    self.ty(&mut i.for_ty);
                    for f in &mut i.fns {
                        self.locals.clear();
                        self.func(f);
                    }
                }
                _ => {}
            }
        }
    }

    fn tps(&mut self, ps: &[j::TParam]) {
        for p in ps {
            let (j::TParam::Type { name, .. } | j::TParam::Value { name, .. }) = p;
            self.tparams.insert(name.clone());
        }
    }

    fn field(&mut self, f: &mut j::Field) {
        self.sp(&mut f.ty_span);
        self.ty(&mut f.ty);
        if let Some(d) = &mut f.default {
            let exported = self.exported.take();
            self.expr(d);
            self.exported = exported;
        }
    }

    fn func(&mut self, f: &mut j::FuncDef) {
        self.sp(&mut f.name_span);
        self.tps(&f.tparams);
        // A name bound anywhere in the function is a local throughout: bare names are resolved
        // flow-insensitively, so a local that shares a module item's name wins.
        let mut probe = Linker::new(self.env);
        probe.names.clear();
        probe.heads.clear();
        probe.collect(&mut f.clone());
        self.locals = probe.locals;
        for p in &mut f.params {
            if let j::Param::Value {
                span, ty, default, ..
            } = p
            {
                self.sp(span);
                self.ty(ty);
                if let Some(d) = default {
                    self.expr(d);
                }
            } else if let j::Param::Pack { ty, .. } = p {
                self.ty(ty);
            }
        }
        if let Some(r) = &mut f.ret {
            if let Some(t) = &mut r.ty {
                self.ty(t);
            }
            r.fail.iter_mut().for_each(|t| self.ty(t));
        }
        self.exported = None;
        if let Some(b) = &mut f.body {
            for c in &mut b.contracts {
                match c {
                    j::Contract::Pre(e) | j::Contract::Post(e) | j::Contract::Decreases(e) => {
                        self.expr(e)
                    }
                    j::Contract::Cost { steps, alloc } => {
                        for e in steps.iter_mut().chain(alloc) {
                            self.expr(e);
                        }
                    }
                    j::Contract::Ex { expr, with } => {
                        self.expr(expr);
                        for (_, e) in with {
                            self.expr(e);
                        }
                    }
                }
            }
            self.stmts(&mut b.stmts);
        }
    }

    /// The names a function binds, into `locals`; nothing is rewritten.
    fn collect(&mut self, f: &mut j::FuncDef) {
        for p in &f.params {
            match p {
                j::Param::Value { name, .. } | j::Param::Pack { name, .. } => {
                    self.locals.insert(name.clone());
                }
                j::Param::Slash => {}
            }
        }
        if let Some(b) = &mut f.body {
            self.stmts(&mut b.stmts);
        }
    }

    fn bind(&mut self, n: &str) {
        self.locals.insert(n.to_owned());
    }

    fn block(&mut self, b: &mut j::Block) {
        self.stmts(&mut b.stmts);
    }

    fn stmts(&mut self, ss: &mut [j::Stmt]) {
        for s in ss {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &mut j::Stmt) {
        use j::StmtKind as K;
        self.sp(&mut s.span);
        self.site = s.span;
        match &mut s.kind {
            K::Expr(e) | K::Yield(e) | K::Become(e) | K::Fail(e) | K::Assert(e) => self.expr(e),
            K::Assign { target, ty, value } => {
                if let j::ExprKind::Name(n) = &target.kind {
                    self.bind(&n.clone());
                }
                self.target(target);
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(value);
            }
            K::AugAssign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            K::Ghost { name, ty, value } => {
                self.bind(&name.clone());
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(value);
            }
            K::Bind {
                target,
                target_span,
                ty,
                value,
                ..
            } => {
                self.bind(&target.clone());
                self.sp(target_span);
                if let Some(t) = ty {
                    self.ty(t);
                }
                self.expr(value);
            }
            K::Return(e) => {
                if let Some(e) = e {
                    self.expr(e);
                }
            }
            K::Unsafe(b) => self.stmts(b),
            K::StageIf { cond, then, els } => {
                self.expr(cond);
                self.stmts(then);
                if let Some(e) = els {
                    self.stmts(e);
                }
            }
            K::StageFor { target, iter, body } => {
                self.bind(&target.clone());
                self.expr(iter);
                self.stmts(body);
            }
            K::If { arms, els } => {
                for (c, b) in arms {
                    self.expr(c);
                    self.block(b);
                }
                if let Some(b) = els {
                    self.block(b);
                }
            }
            K::While {
                cond,
                contracts,
                body,
                ..
            } => {
                self.expr(cond);
                self.loop_contracts(contracts);
                self.stmts(body);
            }
            K::For {
                target,
                iter,
                contracts,
                body,
                ..
            } => {
                for t in target.iter() {
                    self.bind(t);
                }
                self.expr(iter);
                self.loop_contracts(contracts);
                self.stmts(body);
            }
            K::Match { scrutinee, cases } => {
                self.expr(scrutinee);
                for c in cases {
                    self.sp(&mut c.span);
                    self.site = c.span;
                    self.pat(&mut c.pat);
                    if let Some(g) = &mut c.guard {
                        self.expr(g);
                    }
                    self.block(&mut c.body);
                }
            }
            K::With { kind, body } => {
                match kind {
                    j::WithKind::Bind(n, e) => {
                        self.bind(&n.clone());
                        self.expr(e);
                    }
                    j::WithKind::Deadline(e)
                    | j::WithKind::Timeout(e)
                    | j::WithKind::Shield(e)
                    | j::WithKind::Supervisor(e) => self.expr(e),
                }
                self.block(body);
            }
            K::Par { limit, body, .. } => {
                if let Some(l) = limit {
                    self.expr(l);
                }
                match body {
                    j::ParBody::Block(b) => self.stmts(b),
                    j::ParBody::For {
                        target,
                        iter,
                        contracts,
                        body,
                    } => {
                        for t in target.iter() {
                            self.bind(t);
                        }
                        self.expr(iter);
                        self.loop_contracts(contracts);
                        self.stmts(body);
                    }
                }
            }
            K::Select(arms) => {
                for a in arms {
                    self.sp(&mut a.span);
                    self.expr(&mut a.op);
                    if let Some(p) = &mut a.pat {
                        self.pat(p);
                    }
                    self.block(&mut a.body);
                }
            }
            K::Reveal(_)
            | K::Break
            | K::Continue
            | K::BreakTo(_)
            | K::ContinueTo(_)
            | K::Pass
            | K::Unreachable
            | K::Asm { .. } => {}
        }
    }

    fn loop_contracts(&mut self, cs: &mut [j::LoopContract]) {
        for c in cs {
            let (j::LoopContract::Invariant(e) | j::LoopContract::Decreases(e)) = c;
            self.expr(e);
        }
    }

    /// An assignment target: a bare name is a binding, not a reference.
    fn target(&mut self, e: &mut j::Expr) {
        match &mut e.kind {
            j::ExprKind::Name(_) => self.sp(&mut e.span),
            j::ExprKind::Tuple(es) => {
                for x in es {
                    if let j::ExprKind::Name(n) = &x.kind {
                        self.bind(&n.clone());
                    }
                    self.target(x);
                }
            }
            _ => self.expr(e),
        }
    }

    fn pat(&mut self, p: &mut j::Pat) {
        match p {
            j::Pat::Wild => {}
            j::Pat::Lit(e) => self.expr(e),
            j::Pat::Tuple(ps) => ps.iter_mut().for_each(|p| self.pat(p)),
            j::Pat::As(p, n) => {
                self.bind(&n.clone());
                self.pat(p);
            }
            j::Pat::Path { path, args } => {
                if let Some((module, item)) = self.split(path)
                    && let [item] = item
                {
                    let r = self.resolve(&module, item, self.site);
                    *path = vec![r];
                } else if path.len() == 1 && args.is_none() {
                    // A bare name is a binding unless it is a variant or constant.
                    let n = path[0].clone();
                    if self.variants.contains_key(&n) || self.own.contains(&n) {
                        self.own_ref(path);
                    } else {
                        self.bind(&n);
                    }
                } else if path.len() == 1 {
                    self.own_ref(path);
                }
                for (_, p) in args.iter_mut().flatten() {
                    self.pat(p);
                }
            }
        }
    }

    /// A bare name written inside a sibling module: qualified when it names one of its items.
    fn own_ref(&mut self, path: &mut Vec<String>) {
        let (Some(m), [n]) = (self.at.clone(), path.as_slice()) else {
            return;
        };
        if self.locals.contains(n) {
            return;
        }
        if self.own.contains(n) || self.own_types.contains(n) {
            let n = n.clone();
            self.need(&m, &n);
            *path = vec![format!("{m}.{n}")];
        }
    }

    fn ty(&mut self, t: &mut j::Type) {
        match t {
            j::Type::Named { path, args } => {
                if let Some((module, item)) = self.split(path)
                    && let [item] = item
                    && !self.tparams.contains(&path[0])
                {
                    *path = if module == self.env.this {
                        self.check_exported(&module, item);
                        vec![item.clone()]
                    } else {
                        self.check_visible(&module, item, self.site);
                        self.check_exported(&module, item);
                        self.used.insert(module.clone());
                        self.need(&module, item);
                        vec![format!("{module}.{item}")]
                    };
                } else if path.len() == 1 && !self.tparams.contains(&path[0]) {
                    let this = self.env.this.clone();
                    self.check_exported(&this, &path[0]);
                    self.own_ref(path);
                }
                self.targs(args);
            }
            j::Type::Callable {
                params, ret, fail, ..
            } => {
                params.iter_mut().for_each(|t| self.ty(t));
                self.ty(ret);
                fail.iter_mut().for_each(|t| self.ty(t));
            }
            j::Type::Dyn { args, .. } => self.targs(args),
            j::Type::Call { args, .. } => {
                for a in args {
                    self.arg(a);
                }
            }
            j::Type::None => {}
        }
    }

    fn targs(&mut self, args: &mut [j::TArg]) {
        for a in args {
            if let j::TArg::Type(t) = a {
                self.ty(t);
            }
        }
    }

    fn arg(&mut self, a: &mut j::Arg) {
        self.sp(&mut a.span);
        match &mut a.kind {
            j::ArgKind::Expr(e) | j::ArgKind::Ref(e) | j::ArgKind::Become(e) => self.expr(e),
        }
    }

    /// The dotted path an `Attr` chain over a name spells.
    fn spelled(e: &j::Expr) -> Option<Vec<String>> {
        match &e.kind {
            j::ExprKind::Name(n) => Some(vec![n.clone()]),
            j::ExprKind::Attr(inner, n, _) => {
                let mut p = Self::spelled(inner)?;
                p.push(n.clone());
                Some(p)
            }
            _ => None,
        }
    }

    fn expr(&mut self, e: &mut j::Expr) {
        use j::ExprKind as K;
        self.sp(&mut e.span);
        if let K::Attr(..) = &e.kind
            && let Some(path) = Self::spelled(e)
            && let Some((module, item)) = self.split(&path)
            && let [item] = item
        {
            let r = self.resolve(&module, item, e.span);
            e.kind = K::Name(r);
            return;
        }
        match &mut e.kind {
            K::Name(n) => {
                if !self.locals.contains(n.as_str()) {
                    let mut p = vec![n.clone()];
                    self.own_ref(&mut p);
                    *n = p.pop().unwrap_or_default();
                }
            }
            K::Lit(_) | K::Hole | K::Wild => {}
            K::FStr { pieces, .. } => {
                for p in pieces {
                    if let j::FPiece::Expr(e, _) = p {
                        self.expr(e);
                    }
                }
            }
            K::Tuple(es) | K::List(es) | K::Set(es) => es.iter_mut().for_each(|e| self.expr(e)),
            K::Dict(kv) => {
                for (k, v) in kv {
                    self.expr(k);
                    self.expr(v);
                }
            }
            K::Comp {
                elem,
                value,
                target,
                iter,
                cond,
                ..
            } => {
                for t in target.iter() {
                    self.bind(t);
                }
                self.expr(elem);
                if let Some(v) = value {
                    self.expr(v);
                }
                self.expr(iter);
                if let Some(c) = cond {
                    self.expr(c);
                }
            }
            K::Attr(inner, _, s) => {
                self.sp(s);
                self.expr(inner);
            }
            K::Call(f, args) => {
                self.expr(f);
                args.iter_mut().for_each(|a| self.arg(a));
            }
            K::Index(a, b) | K::Binary(_, a, b) | K::Compare(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            K::Slice(a, parts, _) => {
                self.expr(a);
                for p in parts.iter_mut().flatten() {
                    self.expr(p);
                }
            }
            K::Try(a) | K::Unary(_, a) | K::Not(a) | K::Reify(a) | K::Stage(a) => self.expr(a),
            K::Cond { then, cond, els } => {
                self.expr(then);
                self.expr(cond);
                self.expr(els);
            }
            K::Fallback(a, fb) => {
                self.expr(a);
                match &mut **fb {
                    j::Fallback::Value(e) | j::Fallback::Fail(e) => self.expr(e),
                    j::Fallback::Bind(n, e) => {
                        self.bind(&n.clone());
                        self.expr(e);
                    }
                    j::Fallback::Return(e) => {
                        if let Some(e) = e {
                            self.expr(e);
                        }
                    }
                    j::Fallback::Block(n, ss) => {
                        if let Some(n) = n {
                            self.bind(&n.clone());
                        }
                        self.stmts(ss);
                    }
                    j::Fallback::Break | j::Fallback::Continue => {}
                }
            }
            K::Lambda(ps, body) => {
                for p in ps.iter() {
                    self.bind(p);
                }
                self.expr(body);
            }
            K::Cast(a, t) => {
                self.expr(a);
                self.ty(t);
            }
        }
    }
}
