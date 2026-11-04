//! M0 validator and type checker over Yan IR.
//!
//! It reports only what it can prove from the module alone. A type it cannot work out, such as
//! a std result or a generic instance, is unknown and never produces a mismatch.

use std::collections::{HashMap, HashSet};
use std::mem;

use yan_core::ast::*;
use yan_core::{intrinsics, type_text};
use yan_diag::{Diagnostic, Span};

/// Types whose `copy` is E416: they implement `Share` (4.2).
const SHARE: [&str; 5] = ["Rc", "Arc", "Weak", "Sender", "Receiver"];
const COPY_SCALARS: [&str; 6] = ["bool", "char", "Unit", "f32", "f64", "dec128"];
/// Types an integer or float literal can never have.
const NON_NUMERIC: [&str; 6] = ["Str", "Bytes", "bool", "char", "Unit", "Never"];

pub fn check(file: &str, src: &str, m: &Module) -> Vec<Diagnostic> {
    let g = Globals::new(m);
    let mut diags = Vec::new();
    let mut callees: HashMap<String, HashSet<String>> = HashMap::new();
    let module = m.path.join(".");
    let body = |def: &FnDef, is_gen: bool, item: String, diags: &mut Vec<Diagnostic>| {
        let mut cx = Cx::new(&g, file, src, item, def, is_gen);
        if let Some(b) = &def.body {
            cx.block(b);
        }
        cx.finish();
        diags.append(&mut cx.diags);
        cx.callees
    };
    for it in &m.items {
        match &it.kind {
            ItemKind::Fn(f) | ItemKind::Gen(f) => {
                let key = f.path.join(".");
                let c = body(
                    f,
                    matches!(it.kind, ItemKind::Gen(_)),
                    format!("{module}.{key}"),
                    &mut diags,
                );
                callees.insert(key, c);
            }
            ItemKind::Impl(imp) => {
                let is_drop = imp.trait_.path.last().is_some_and(|n| n == "Drop");
                for f in &imp.fns {
                    let item = format!("{module}.{}", f.path.join("."));
                    if is_drop {
                        check_drop(&g, file, src, &item, f, imp, &mut diags);
                    }
                    body(f, false, item, &mut diags);
                }
            }
            ItemKind::Trait(t) => {
                for f in t.sigs.iter().filter(|f| f.body.is_some()) {
                    body(
                        f,
                        false,
                        format!("{module}.{}", f.path.join(".")),
                        &mut diags,
                    );
                }
            }
            ItemKind::Test(t) => {
                let def = FnDef {
                    is_pub: false,
                    is_unsafe: false,
                    path: t.path.clone(),
                    name_span: it.span,
                    tparams: Vec::new(),
                    params: Vec::new(),
                    ret: Type::Named(Named {
                        path: vec!["Unit".into()],
                        args: Vec::new(),
                    }),
                    from: Vec::new(),
                    fail: None,
                    contracts: Vec::new(),
                    body: Some(t.body.clone()),
                };
                body(
                    &def,
                    false,
                    format!("{module}.{}", t.path.join(".")),
                    &mut diags,
                );
            }
            _ => {}
        }
    }
    for (key, f) in &g.fns {
        let decreases = f
            .contracts
            .iter()
            .any(|c| matches!(c, Contract::Decreases(_)));
        if !decreases && reaches(&callees, key) {
            diags.push(error(
                file,
                src,
                &format!("{module}.{key}"),
                "E602",
                f.name_span,
                format!("recursive function `{key}` has no `decreases` clause"),
            ));
        }
    }
    diags
}

/// Does a path through the call graph lead from `f` back to `f`?
fn reaches(callees: &HashMap<String, HashSet<String>>, f: &str) -> bool {
    let mut seen = HashSet::new();
    let mut stack: Vec<&str> = callees
        .get(f)
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
    while let Some(g) = stack.pop() {
        if g == f {
            return true;
        }
        if seen.insert(g) {
            stack.extend(callees.get(g).into_iter().flatten().map(String::as_str));
        }
    }
    false
}

/// 5.4: a `drop` body uses no capability except `alloc`, and a `@linear` type has no `Drop`.
fn check_drop(
    g: &Globals,
    file: &str,
    src: &str,
    item: &str,
    f: &FnDef,
    imp: &Impl,
    diags: &mut Vec<Diagnostic>,
) {
    if let Some(Param::Cap { name, .. }) = f
        .params
        .iter()
        .find(|p| matches!(p, Param::Cap { ty, .. } if ty.path.last().is_none_or(|n| n != "Alloc")))
    {
        diags.push(error(
            file,
            src,
            item,
            "E421",
            f.name_span,
            format!("`drop` takes the capability `{name}`"),
        ));
    }
    if base(&imp.for_ty).is_some_and(|t| g.linear.contains(t)) {
        diags.push(error(
            file,
            src,
            item,
            "E421",
            f.name_span,
            "a `@linear` type cannot implement `Drop`".into(),
        ));
    }
}

fn error(file: &str, src: &str, item: &str, code: &str, span: Span, message: String) -> Diagnostic {
    let line = src
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
    Diagnostic::error(code, file, span, quote, message).with_item(item)
}

fn base(t: &Type) -> Option<&str> {
    match t {
        Type::Named(n) => n.path.last().map(String::as_str),
        _ => None,
    }
}

fn is_copy(t: Option<&Type>) -> bool {
    t.and_then(base)
        .is_some_and(|b| intrinsics::INTS.contains(&b) || COPY_SCALARS.contains(&b))
}

fn conv_text(c: Conv) -> &'static str {
    match c {
        Conv::Let => "let",
        Conv::Inout => "inout",
        Conv::Sink => "sink",
    }
}

/// Parameter conventions and types, return, failure and capabilities (E271 compares these).
fn signature(f: &FnDef) -> String {
    let params: Vec<String> = f
        .params
        .iter()
        .map(|p| match p {
            Param::Value { conv, ty, .. } => format!("{} {}", conv_text(*conv), type_text(ty)),
            Param::Cap { ty, .. } => format!("cap {}", type_text(&Type::Named(ty.clone()))),
        })
        .collect();
    let fail = f.fail.as_ref().map(type_text).unwrap_or_default();
    format!(
        "({}) -> {} fail {fail}",
        params.join(", "),
        type_text(&f.ret)
    )
}

struct Globals<'m> {
    module: String,
    fns: HashMap<String, &'m FnDef>,
    gens: HashSet<String>,
    records: HashMap<String, &'m Record>,
    /// Sum name to variant names, with the prelude's `Option` and `Result`.
    sums: HashMap<String, Vec<String>>,
    /// Variant name to its sum, for sums without type parameters.
    variant_of: HashMap<String, String>,
    linear: HashSet<String>,
    /// Trait methods and extern functions: callable, but their types are not tracked yet.
    other_fns: HashSet<String>,
}

impl<'m> Globals<'m> {
    fn new(m: &'m Module) -> Self {
        let mut g = Globals {
            module: m.path.join("."),
            fns: HashMap::new(),
            gens: HashSet::new(),
            records: HashMap::new(),
            sums: HashMap::from([
                ("Option".into(), vec!["Some".into(), "None".into()]),
                ("Result".into(), vec!["Ok".into(), "Err".into()]),
            ]),
            variant_of: HashMap::new(),
            linear: HashSet::new(),
            other_fns: HashSet::new(),
        };
        for it in &m.items {
            let linear = it.attrs.iter().any(|a| a.name == "linear");
            match &it.kind {
                ItemKind::Fn(f) => {
                    g.fns.insert(f.path.join("."), f);
                }
                ItemKind::Gen(f) => {
                    g.fns.insert(f.path.join("."), f);
                    g.gens.insert(f.path.join("."));
                }
                ItemKind::Record(r) => {
                    let name = r.path.join(".");
                    if linear {
                        g.linear.insert(name.clone());
                    }
                    g.records.insert(name, r);
                }
                ItemKind::Sum(s) => {
                    let name = s.path.join(".");
                    if linear {
                        g.linear.insert(name.clone());
                    }
                    if s.tparams.is_empty() {
                        g.variant_of
                            .extend(s.variants.iter().map(|v| (v.name.clone(), name.clone())));
                    }
                    g.sums
                        .insert(name, s.variants.iter().map(|v| v.name.clone()).collect());
                }
                ItemKind::Trait(t) => g.other_fns.extend(t.sigs.iter().map(|f| f.path.join("."))),
                ItemKind::Impl(i) => g.other_fns.extend(i.fns.iter().map(|f| f.path.join("."))),
                ItemKind::Extern(x) => {
                    for xi in &x.items {
                        if let XItem::Fn { name, .. } = xi {
                            g.other_fns.insert(name.clone());
                            g.other_fns.insert(format!("{}.{name}", x.lib));
                        }
                    }
                }
                _ => {}
            }
        }
        g
    }

    fn function(&self, path: &str) -> Option<(&str, &'m FnDef)> {
        let key = path
            .strip_prefix(&format!("{}.", self.module))
            .unwrap_or(path);
        self.fns.get_key_value(key).map(|(k, f)| (k.as_str(), *f))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Param(Conv),
    Cap,
    Let,
    Var,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum How {
    Read,
    Inout,
    Sink,
    Ret,
    Store,
    Move,
    Drop,
}

impl How {
    fn consumes(self) -> bool {
        matches!(
            self,
            How::Sink | How::Ret | How::Store | How::Move | How::Drop
        )
    }

    fn escapes(self) -> bool {
        matches!(self, How::Sink | How::Ret | How::Store)
    }
}

#[derive(Clone)]
struct Binding {
    ty: Option<Type>,
    kind: Kind,
    span: Span,
    /// The `par` block and branch that made this binding.
    tag: Option<(usize, usize)>,
    /// Bound by `let` or `var` in this body, not a parameter.
    local: bool,
    try_at: Option<Span>,
    /// Captures that make the bound closure non-escaping: E406, or E306 for a capability (R467).
    bad_captures: Vec<(&'static str, Span)>,
}

struct Cx<'g, 'm> {
    g: &'g Globals<'m>,
    file: &'g str,
    src: &'g str,
    item: String,
    def: &'g FnDef,
    is_gen: bool,
    env: HashMap<Name, Binding>,
    /// Bindings in order, for the end-of-body checks.
    order: Vec<Name>,
    uses: HashMap<Name, Vec<How>>,
    /// Moved names and where each moved.
    moved: HashMap<Name, Span>,
    reported: HashSet<Name>,
    par: Vec<(usize, usize)>,
    next_par: usize,
    /// Accesses inside `par` branches to places bound outside them: name, par, branch, written.
    par_uses: Vec<(Name, usize, usize, bool)>,
    with_depth: usize,
    callees: HashSet<String>,
    /// Each join point's parameter names, so a jump's arguments carry their captures in.
    jps: HashMap<Name, Vec<Name>>,
    diags: Vec<Diagnostic>,
}

fn diverges(b: &Block) -> bool {
    b.stmts.last().is_some_and(|s| {
        matches!(
            s.kind,
            StmtKind::Ret(_)
                | StmtKind::Fail(_)
                | StmtKind::Jump { .. }
                | StmtKind::Become(_)
                | StmtKind::Break { .. }
                | StmtKind::Continue { .. }
                | StmtKind::Unreachable
        )
    })
}

fn pat_names(p: &Pat, out: &mut Vec<Name>, variants: &HashMap<String, String>) {
    match p {
        Pat::Bind(n) if !variants.contains_key(n) && n != "None" => out.push(n.clone()),
        Pat::Ctor {
            fields: Some((fs, _)),
            ..
        } => fs.iter().for_each(|(_, p)| pat_names(p, out, variants)),
        Pat::Tuple(ps) => ps.iter().for_each(|p| pat_names(p, out, variants)),
        Pat::Or(ps) => ps.iter().take(1).for_each(|p| pat_names(p, out, variants)),
        Pat::Slice { elems, rest } => {
            elems.iter().for_each(|p| pat_names(p, out, variants));
            if let Some(Some(n)) = rest {
                out.push(n.clone());
            }
        }
        _ => {}
    }
}

impl<'g, 'm> Cx<'g, 'm> {
    fn new(
        g: &'g Globals<'m>,
        file: &'g str,
        src: &'g str,
        item: String,
        def: &'g FnDef,
        is_gen: bool,
    ) -> Self {
        let mut cx = Cx {
            g,
            file,
            src,
            item,
            def,
            is_gen,
            env: HashMap::new(),
            order: Vec::new(),
            uses: HashMap::new(),
            moved: HashMap::new(),
            reported: HashSet::new(),
            par: Vec::new(),
            next_par: 0,
            par_uses: Vec::new(),
            with_depth: 0,
            callees: HashSet::new(),
            jps: HashMap::new(),
            diags: Vec::new(),
        };
        for p in &def.params {
            cx.bind_param(p, def.name_span);
        }
        cx
    }

    fn bind_param(&mut self, p: &Param, span: Span) {
        let (name, ty, kind) = match p {
            Param::Value { conv, name, ty, .. } => (name, ty.clone(), Kind::Param(*conv)),
            Param::Cap { name, ty } => (name, Type::Named(ty.clone()), Kind::Cap),
        };
        self.bind(name, Some(ty), kind, span, false);
    }

    fn bind(
        &mut self,
        name: &Name,
        ty: Option<Type>,
        kind: Kind,
        span: Span,
        local: bool,
    ) -> &mut Binding {
        self.order.push(name.clone());
        self.moved.remove(name);
        let b = Binding {
            ty,
            kind,
            span,
            tag: self.par.last().copied(),
            local,
            try_at: None,
            bad_captures: Vec::new(),
        };
        self.env.insert(name.clone(), b);
        self.env.get_mut(name).expect("just inserted")
    }

    fn err(&mut self, code: &str, span: Span, message: String) {
        let d = error(self.file, self.src, &self.item, code, span, message);
        self.diags.push(d);
    }

    fn finish(&mut self) {
        let order = mem::take(&mut self.order);
        let mut seen = HashSet::new();
        for n in order.iter().filter(|n| seen.insert(n.as_str())) {
            let Some(b) = self.env.get(n).cloned() else {
                continue;
            };
            let uses = self.uses.get(n).cloned().unwrap_or_default();
            if let Some(at) = b.try_at
                && uses.is_empty()
            {
                self.err(
                    "E311",
                    at,
                    format!("the `try` result `{n}` is never inspected"),
                );
            }
            let linear =
                b.ty.as_ref()
                    .and_then(base)
                    .is_some_and(|t| self.g.linear.contains(t));
            if b.local && linear && !uses.iter().any(|h| h.consumes()) {
                self.err(
                    "E412",
                    b.span,
                    format!("`@linear` value `{n}` is never consumed"),
                );
            }
            if uses.iter().any(|h| h.escapes()) {
                for (code, at) in b.bad_captures {
                    let what = if code == "E306" {
                        "a capability"
                    } else {
                        "a borrow"
                    };
                    // A closure the elaborator named (`_t2`) has no name in the source.
                    let closure = if n.starts_with('_') {
                        "this closure".to_owned()
                    } else {
                        format!("closure `{n}`")
                    };
                    self.err(code, at, format!("{closure} escapes but captures {what}"));
                }
            }
        }
    }

    fn use_name(&mut self, n: &Name, span: Span, how: How) {
        if let Some(&at) = self.moved.get(n)
            && self.reported.insert(n.clone())
        {
            let d = error(
                self.file,
                self.src,
                &self.item,
                "E401",
                span,
                format!("`{n}` is used after it was moved"),
            );
            self.diags.push(d.with_label(at, "moved"));
        }
        if let Some(b) = self.env.get(n)
            && let Some((pid, branch)) = b.tag
            && self.par.iter().any(|&(p, x)| p == pid && x != branch)
        {
            let d = error(
                self.file,
                self.src,
                &self.item,
                "E705",
                span,
                format!("`{n}` is bound in another branch of this `par`"),
            );
            self.diags.push(d.with_label(b.span, "bound"));
        }
        self.par_conflict(n, span, how);
        self.uses.entry(n.clone()).or_default().push(how);
    }

    /// E705: one `par` branch writes a place that another branch reads or writes.
    fn par_conflict(&mut self, n: &Name, span: Span, how: How) {
        let Some(b) = self.env.get(n) else {
            return;
        };
        let inner = b.tag;
        let write = how == How::Inout;
        for &(pid, branch) in &self.par.clone() {
            if inner.is_some_and(|(p, _)| p == pid) {
                continue;
            }
            let clash = self
                .par_uses
                .iter()
                .any(|(m, p, x, w)| m == n && *p == pid && *x != branch && (*w || write));
            if clash && self.reported.insert(format!("par:{n}")) {
                self.err(
                    "E705",
                    span,
                    format!("`{n}` is written in one branch of this `par` and accessed in another"),
                );
            }
            self.par_uses.push((n.clone(), pid, branch, write));
        }
    }

    fn atom(&mut self, a: &Atom, how: How) -> Option<String> {
        match a {
            Atom::Name(n, sp) => {
                if matches!(how, How::Ret | How::Store)
                    && self.env.get(n).is_some_and(|b| b.kind == Kind::Cap)
                {
                    self.err(
                        "E306",
                        *sp,
                        format!("capability `{n}` cannot be returned or stored"),
                    );
                }
                self.use_name(n, *sp, how);
                self.env.get(n).and_then(|b| b.ty.as_ref()).map(type_text)
            }
            Atom::Lit(l, _) => Some(
                match l {
                    Literal::Int(_) => "{int}",
                    Literal::Float(_) => "{float}",
                    Literal::Str(_) => "Str",
                    Literal::Char(_) => "char",
                    Literal::Bool(_) => "bool",
                    Literal::Unit => "Unit",
                }
                .into(),
            ),
        }
    }

    fn expr(&mut self, e: &Expr) {
        match e {
            Expr::Atom(a) => {
                self.atom(a, How::Read);
            }
            Expr::Call(c) => {
                self.call(c);
            }
        }
    }

    /// Run `f` from the current move state for each branch and merge the branches that continue.
    fn branches<'b>(&mut self, blocks: impl IntoIterator<Item = &'b Block>, fallthrough: bool) {
        let before = self.moved.clone();
        let mut merged = if fallthrough {
            before.clone()
        } else {
            HashMap::new()
        };
        for b in blocks {
            self.moved = before.clone();
            self.block(b);
            if !diverges(b) {
                merged.extend(self.moved.drain());
            }
        }
        self.moved = merged;
    }

    fn block(&mut self, b: &Block) {
        for s in &b.stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let {
                mutable,
                pat,
                pat_span,
                ty,
                rhs,
                rhs_span,
            } => {
                let bad_captures = match rhs {
                    Rhs::Closure(c) => c
                        .captures
                        .iter()
                        .filter_map(|(cv, n, sp)| {
                            if self.env.get(n).is_some_and(|b| b.kind == Kind::Cap) {
                                Some(("E306", *sp))
                            } else {
                                (*cv == Conv::Inout).then_some(("E406", *sp))
                            }
                        })
                        .collect(),
                    _ => self.carried_captures(rhs),
                };
                let got = self.rhs(rhs, *rhs_span);
                self.compatible(ty, got, *rhs_span);
                let kind = if *mutable { Kind::Var } else { Kind::Let };
                if let Pat::Bind(n) = pat {
                    let b = self.bind(n, Some(ty.clone()), kind, *pat_span, true);
                    b.try_at = matches!(rhs, Rhs::Try(_)).then_some(*rhs_span);
                    b.bad_captures = bad_captures;
                } else {
                    self.bind_pat(pat, *pat_span);
                }
            }
            StmtKind::Assign {
                place,
                rhs,
                rhs_span,
            } => {
                // A closure copied into another binding (an `if` or `match` result) escapes
                // through it, so its bad captures travel with it.
                let carried = self.carried_captures(rhs);
                if !carried.is_empty()
                    && let Some(b) = self.env.get_mut(&place.root)
                {
                    b.bad_captures.extend(carried);
                }
                let got = self.rhs(rhs, *rhs_span);
                if let Some(Kind::Let | Kind::Cap | Kind::Param(Conv::Let | Conv::Sink)) =
                    self.env.get(&place.root).map(|b| b.kind)
                {
                    self.err("E403", s.span, format!("`{}` is not mutable", place.root));
                }
                for p in &place.projs {
                    if let Proj::Index(a) = p {
                        self.atom(a, How::Read);
                    }
                }
                if let Some(t) = self.place_ty(place) {
                    self.compatible(&t, got, *rhs_span);
                }
                if place.projs.is_empty() {
                    self.moved.remove(&place.root);
                }
            }
            StmtKind::If { cond, then, els } | StmtKind::ComptimeIf { cond, then, els } => {
                self.atom(cond, How::Read);
                self.branches(std::iter::once(then).chain(els), els.is_none());
            }
            StmtKind::Match { scrutinee, arms } => {
                self.atom(scrutinee, How::Read);
                self.exhaustive(scrutinee, arms, s.span);
                for (p, _) in arms {
                    self.bind_pat(p, s.span);
                }
                self.branches(arms.iter().map(|(_, b)| b), false);
            }
            StmtKind::Loop {
                invariants,
                decreases,
                body,
                ..
            } => {
                invariants
                    .iter()
                    .chain(decreases)
                    .for_each(|e| self.expr(e));
                self.branches([body], true);
            }
            StmtKind::Break { value, .. } => {
                if let Some(v) = value {
                    self.atom(v, How::Read);
                }
            }
            StmtKind::Continue { .. } | StmtKind::Unreachable | StmtKind::Asm { .. } => {}
            StmtKind::Jp { name, params, body } => {
                let names = params
                    .iter()
                    .map(|p| match p {
                        Param::Value { name, .. } | Param::Cap { name, .. } => name.clone(),
                    })
                    .collect();
                self.jps.insert(name.clone(), names);
                for p in params {
                    self.bind_param(p, s.span);
                }
                let before = self.moved.clone();
                self.block(body);
                self.moved = before;
            }
            StmtKind::Jump { name, args } => {
                let params = self.jps.get(name).cloned().unwrap_or_default();
                for (a, p) in args.iter().zip(&params) {
                    let carried = self.carried_captures(&Rhs::Atom(a.clone()));
                    if let Some(b) = self.env.get_mut(p) {
                        b.bad_captures.extend(carried);
                    }
                }
                for a in args {
                    self.atom(a, How::Read);
                }
            }
            StmtKind::Ret(a) => {
                self.atom(a, How::Ret);
            }
            StmtKind::Fail(a) => {
                self.atom(a, How::Ret);
            }
            StmtKind::Yield(a) => {
                if !self.is_gen {
                    self.err("E141", s.span, "`yield` outside a generator".into());
                }
                self.atom(a, How::Read);
            }
            StmtKind::Become(c) => self.become_(c, s.span),
            StmtKind::With {
                name,
                handler,
                body,
                ..
            } => {
                self.atom(handler, How::Read);
                self.bind(name, None, Kind::Cap, s.span, false);
                self.with_depth += 1;
                self.block(body);
                self.with_depth -= 1;
            }
            StmtKind::Par { blocks, .. } => {
                let id = self.next_par;
                self.next_par += 1;
                for (i, b) in blocks.iter().enumerate() {
                    self.par.push((id, i));
                    self.block(b);
                    self.par.pop();
                }
            }
            StmtKind::ComptimeFor {
                name,
                ty,
                iter,
                body,
            } => {
                self.atom(iter, How::Read);
                self.bind(name, Some(ty.clone()), Kind::Let, s.span, false);
                self.branches([body], true);
            }
            StmtKind::Assert(e) => self.expr(e),
            StmtKind::Unsafe(b) => self.block(b),
            StmtKind::Drop(n) => self.use_name(n, s.span, How::Drop),
            StmtKind::Reuse { name, ctor } => {
                self.use_name(name, s.span, How::Move);
                self.rhs(&Rhs::Ctor(ctor.clone()), s.span);
            }
            StmtKind::Rhs(rhs) => {
                let got = self.rhs(rhs, s.span);
                match rhs {
                    Rhs::Try(_) => self.err("E311", s.span, "the `try` result is discarded".into()),
                    Rhs::Call(_) | Rhs::Propagate { .. } => {
                        if let Some(t) = got.filter(|t| t != "Unit" && t != "Never") {
                            self.err(
                                "E312",
                                s.span,
                                format!("a result of type `{t}` is discarded"),
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn bind_pat(&mut self, p: &Pat, span: Span) {
        let mut names = Vec::new();
        pat_names(p, &mut names, &self.g.variant_of);
        for n in names {
            self.bind(&n, None, Kind::Let, span, true);
        }
    }

    /// E201 when `got` is known and cannot have the declared type.
    fn compatible(&mut self, decl: &Type, got: Option<String>, at: Span) {
        let (Type::Named(_), Some(got)) = (decl, got) else {
            return;
        };
        let d = type_text(decl);
        let ok = match got.as_str() {
            "{int}" => !NON_NUMERIC.contains(&d.as_str()),
            "{float}" => {
                !NON_NUMERIC.contains(&d.as_str()) && !intrinsics::INTS.contains(&d.as_str())
            }
            "Never" => true,
            g => g == d,
        };
        if !ok {
            let got = got.trim_matches(|c| c == '{' || c == '}');
            self.err("E201", at, format!("expected `{d}`, found {got}"));
        }
    }

    fn place_ty(&mut self, p: &Place) -> Option<Type> {
        let mut t = self.env.get(&p.root)?.ty.clone()?;
        for proj in &p.projs {
            let Proj::Field(f, at) = proj else {
                return None;
            };
            let Type::Named(n) = &t else { return None };
            let r = *self.g.records.get(&n.path.join("."))?;
            match r.fields.iter().find(|fd| fd.name == *f) {
                Some(fd) if r.tparams.is_empty() => t = fd.ty.clone(),
                Some(_) => return None,
                None => {
                    self.err(
                        "E203",
                        *at,
                        format!("`{}` has no field `{f}`", n.path.join(".")),
                    );
                    return None;
                }
            }
        }
        Some(t)
    }

    fn place(&mut self, p: &Place, how: How) -> Option<Type> {
        self.use_name(&p.root, p.root_span, how);
        for proj in &p.projs {
            if let Proj::Index(a) = proj {
                self.atom(a, How::Read);
            }
        }
        self.place_ty(p)
    }

    /// The bad captures of the closure binding `rhs` names, if it names one.
    fn carried_captures(&self, rhs: &Rhs) -> Vec<(&'static str, Span)> {
        match rhs {
            Rhs::Atom(Atom::Name(n, _)) => self
                .env
                .get(n)
                .map(|b| b.bad_captures.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The type text of `rhs` when it is known.
    fn rhs(&mut self, rhs: &Rhs, at: Span) -> Option<String> {
        match rhs {
            Rhs::Atom(a) => self.atom(a, How::Read),
            Rhs::Call(c) => self.call(c).0,
            Rhs::Propagate { call, via } => {
                let (ret, callee) = self.call(call);
                let callee_fail = callee.and_then(|f| f.fail.as_ref());
                if let (None, Some(cft), Some(own)) = (via, callee_fail, self.def.fail.as_ref()) {
                    let cf = type_text(cft);
                    let member = |c: &Type| match own {
                        Type::Union(ts) => ts.iter().any(|t| type_text(t) == type_text(c)),
                        t => type_text(t) == type_text(c),
                    };
                    let injects = type_text(own) == cf
                        || match cft {
                            Type::Union(cs) => cs.iter().all(member),
                            c => member(c),
                        };
                    if !injects {
                        let own = type_text(own);
                        self.err(
                            "E310",
                            call.span,
                            format!(
                                "no `From` conversion from `{cf}` to `{own}`; name one with `via`"
                            ),
                        );
                    }
                }
                ret
            }
            Rhs::Try(c) => {
                let (ret, callee) = self.call(c);
                let fail = callee
                    .and_then(|f| f.fail.as_ref())
                    .filter(|t| !matches!(t, Type::Union(_)));
                Some(format!("Result[{}, {}]", ret?, type_text(fail?)))
            }
            Rhs::Ctor(Ctor::Tuple(xs)) => {
                xs.iter().for_each(|a| {
                    self.atom(a, How::Store);
                });
                None
            }
            Rhs::Ctor(Ctor::Named {
                path,
                targs,
                fields,
            }) => {
                for (_, a) in fields {
                    self.atom(a, How::Store);
                }
                let name = path.join(".");
                if !targs.is_empty() {
                    return None;
                }
                match self.g.records.get(&name) {
                    Some(r) if r.tparams.is_empty() => Some(name),
                    Some(_) => None,
                    None => self.g.variant_of.get(&name).cloned(),
                }
            }
            Rhs::Proj(p) => self.place(p, How::Read).as_ref().map(type_text),
            Rhs::Copy(p) => {
                let t = self.place(p, How::Read)?;
                if let Some(b) = base(&t).filter(|b| SHARE.contains(b)) {
                    self.err("E416", at, format!("`copy` of a `{b}`; use `share`"));
                }
                Some(type_text(&t))
            }
            Rhs::Move(p) => {
                let t = self.place(p, How::Move);
                if p.projs.is_empty() {
                    self.moved.insert(p.root.clone(), at);
                }
                t.as_ref().map(type_text)
            }
            Rhs::Closure(c) => {
                for (cv, n, sp) in &c.captures {
                    self.use_name(
                        n,
                        *sp,
                        if *cv == Conv::Inout {
                            How::Inout
                        } else {
                            How::Read
                        },
                    );
                }
                let saved = (
                    self.env.clone(),
                    mem::take(&mut self.par),
                    self.moved.clone(),
                    self.is_gen,
                );
                for p in &c.params {
                    self.bind_param(p, at);
                }
                self.is_gen = false;
                self.block(&c.body);
                (self.env, self.par, self.moved, self.is_gen) = saved;
                None
            }
        }
    }

    /// Check a call and its arguments. Returns the result type, when known, and the callee
    /// when it is a function of this module.
    fn call(&mut self, c: &Call) -> (Option<String>, Option<&'m FnDef>) {
        for a in &c.args {
            match a {
                Arg::Value(conv, Atom::Name(n, sp), _) => {
                    let how = match conv {
                        Conv::Let => How::Read,
                        Conv::Inout => How::Inout,
                        Conv::Sink => How::Sink,
                    };
                    // An inout argument is written back (4.1), so it must be a mutable place; a
                    // `sink` parameter is owned and may be changed in place. Names starting with
                    // `_` are elaborator temporaries, which the surface reports.
                    if *conv == Conv::Inout
                        && !n.starts_with('_')
                        && let Some(Kind::Let | Kind::Param(Conv::Let)) =
                            self.env.get(n).map(|b| b.kind)
                    {
                        self.err("E403", *sp, format!("`{n}` is not mutable"));
                    }
                    self.use_name(n, *sp, how);
                }
                Arg::Value(_, Atom::Lit(..), _) => {}
                Arg::Cap(n, sp) => {
                    if self.env.get(n).is_some_and(|b| b.kind == Kind::Cap) {
                        self.use_name(n, *sp, How::Read);
                    } else {
                        self.err("E301", *sp, format!("capability `{n}` is not in scope"));
                    }
                }
            }
        }
        self.overlap(&c.args);
        for a in &c.args {
            if let Arg::Value(Conv::Sink, Atom::Name(n, at), _) = a
                && !is_copy(self.env.get(n).and_then(|b| b.ty.as_ref()))
            {
                self.moved.insert(n.clone(), *at);
            }
        }

        let name = c.path.join(".");
        if c.path.len() == 1 && self.env.contains_key(&name) {
            self.use_name(&name, c.span, How::Read);
            return match self.env.get(&name).and_then(|b| b.ty.clone()) {
                Some(Type::Fn(ft)) => {
                    self.arity(c, ft.params.len());
                    (Some(type_text(&ft.ret)), None)
                }
                _ => (None, None),
            };
        }
        if let Some((key, f)) = self.g.function(&name) {
            self.callees.insert(key.to_owned());
            self.arity(c, f.params.len());
            let generic = !f.tparams.is_empty() || self.g.gens.contains(key);
            return ((!generic).then(|| type_text(&f.ret)), Some(f));
        }
        if let Some(s) = intrinsics::lookup(&name) {
            self.arity(c, s.arity);
            return (Some(s.ret), None);
        }
        if !name.starts_with("std.") && !self.g.other_fns.contains(&name) {
            self.err("E200", c.span, format!("cannot find function `{name}`"));
        }
        (None, None)
    }

    fn arity(&mut self, c: &Call, expected: usize) {
        if c.args.len() != expected {
            let name = c.path.join(".");
            self.err(
                "E202",
                c.span,
                format!(
                    "`{name}` takes {expected} arguments, {} given",
                    c.args.len()
                ),
            );
        }
    }

    /// E402: a place passed `inout` may not be passed again in the same call.
    fn overlap(&mut self, args: &[Arg]) {
        let named = |a: &Arg| match a {
            Arg::Value(cv, Atom::Name(n, _), sp) => Some((*cv, n.clone(), *sp)),
            _ => None,
        };
        let args: Vec<_> = args.iter().filter_map(named).collect();
        for (j, (cj, nj, sj)) in args.iter().enumerate() {
            if args[..j]
                .iter()
                .any(|(ci, ni, _)| ni == nj && (*ci == Conv::Inout || *cj == Conv::Inout))
            {
                self.err(
                    "E402",
                    *sj,
                    format!("`{nj}` is passed for mutation and accessed again in this call"),
                );
                return;
            }
        }
    }

    /// R260: `become` rules (E273, E271, E420).
    fn become_(&mut self, c: &Call, at: Span) {
        if !self.par.is_empty() || self.with_depth > 0 || self.is_gen {
            self.err(
                "E273",
                at,
                "`become` inside `par`, `with` or a generator".into(),
            );
            return;
        }
        let name = c.path.join(".");
        if c.path.len() == 1 && self.env.get(&name).is_some_and(|b| b.local) {
            self.err("E273", c.span, "`become` through a closure".into());
            return;
        }
        let (_, callee) = self.call(c);
        if let Some(f) = callee
            && f.params.len() == c.args.len()
            && f.tparams.is_empty()
            && self.def.tparams.is_empty()
            && signature(f) != signature(self.def)
        {
            self.err(
                "E271",
                c.span,
                format!("`become` target `{name}` has a different signature"),
            );
        }
        for a in &c.args {
            if let Arg::Value(Conv::Let | Conv::Inout, Atom::Name(n, _), sp) = a
                && let Some(b) = self.env.get(n)
                && b.local
                && !is_copy(b.ty.as_ref())
            {
                self.err(
                    "E420",
                    *sp,
                    format!("`become` argument borrows the local `{n}`; move or copy it"),
                );
            }
        }
    }

    /// E503: a match on a sum names every variant or has a catch-all arm.
    fn exhaustive(&mut self, scrutinee: &Atom, arms: &[(Pat, Block)], at: Span) {
        let Atom::Name(n, _) = scrutinee else { return };
        let Some(sum) = self.env.get(n).and_then(|b| b.ty.as_ref()).and_then(base) else {
            return;
        };
        let Some(variants) = self.g.sums.get(sum) else {
            return;
        };
        let mut covered = HashSet::new();
        fn cover<'p>(p: &'p Pat, variants: &[String], covered: &mut HashSet<&'p str>) -> bool {
            match p {
                Pat::Wild => true,
                Pat::Bind(n) if !variants.contains(n) => true,
                Pat::Bind(n) => {
                    covered.insert(n);
                    false
                }
                Pat::Ctor { path, .. } => {
                    if let Some(v) = path.last() {
                        covered.insert(v);
                    }
                    false
                }
                // Every alternative is visited, so each one marks its variant.
                Pat::Or(ps) => ps
                    .iter()
                    .map(|p| cover(p, variants, covered))
                    .collect::<Vec<_>>()
                    .contains(&true),
                _ => false,
            }
        }
        let mut catch_all = false;
        for (p, _) in arms {
            catch_all |= cover(p, variants, &mut covered);
        }
        let missing: Vec<&str> = variants
            .iter()
            .map(String::as_str)
            .filter(|v| !covered.contains(v))
            .collect();
        if !catch_all && !missing.is_empty() {
            self.err(
                "E503",
                at,
                format!("match on `{sum}` does not handle {}", missing.join(", ")),
            );
        }
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    fn codes(body: &str) -> Vec<(String, u32, u32)> {
        let src = format!("module t\n{body}");
        let (m, d) = yan_core::parse("t.yir", &src);
        assert!(
            d.is_empty(),
            "{:?}",
            d.iter().map(|d| d.render_line(None)).collect::<Vec<_>>()
        );
        check("t.yir", &src, &m.unwrap())
            .into_iter()
            .map(|d| (d.code, d.span.line, d.span.col))
            .collect()
    }

    #[test]
    fn a_move_in_a_diverging_branch_does_not_leak() {
        let src = "fn keep(sink xs: List[i64]) -> i64 {\n    ret 0\n}\n\
                   fn f(let c: bool, sink xs: List[i64]) -> i64 {\n    if c {\n        let n: i64 = keep(sink xs)\n        ret n\n    }\n\
                   \x20   let m: i64 = keep(sink xs)\n    ret m\n}\n";
        assert_eq!(codes(src), []);
    }

    #[test]
    fn a_move_in_one_branch_is_seen_after_the_if() {
        let src = "fn keep(sink xs: List[i64]) -> i64 {\n    ret 0\n}\n\
                   fn f(let c: bool, sink xs: List[i64]) -> i64 {\n    if c {\n        let n: i64 = keep(sink xs)\n    }\n\
                   \x20   let m: i64 = keep(sink xs)\n    ret m\n}\n";
        assert_eq!(codes(src), [("E401".into(), 9, 28)]);
    }

    #[test]
    fn use_after_move_labels_the_move() {
        let src = "module t\nfn keep(sink xs: List[i64]) -> i64 {\n    ret 0\n}\n\
                   fn f(sink xs: List[i64]) -> i64 {\n    let a: i64 = keep(sink xs)\n    let b: i64 = keep(sink xs)\n    ret b\n}\n";
        let (m, _) = yan_core::parse("t.yir", src);
        let d = check("t.yir", src, &m.unwrap());
        assert_eq!(d[0].code, "E401");
        assert_eq!(
            (d[0].labels[0].text.as_str(), d[0].labels[0].span.line),
            ("moved", 6)
        );
    }

    #[test]
    fn scalars_are_copied_not_moved() {
        let src = "fn eat(sink x: i64) -> i64 {\n    ret x\n}\n\
                   fn f(sink x: i64) -> i64 {\n    let a: i64 = eat(sink x)\n    let b: i64 = eat(sink x)\n    ret b\n}\n";
        assert_eq!(codes(src), []);
    }

    #[test]
    fn mutual_recursion_needs_decreases() {
        let src = "fn a(let n: i64) -> i64 {\n    let r: i64 = b(let n)\n    ret r\n}\n\
                   fn b(let n: i64) -> i64 {\n    let r: i64 = a(let n)\n    ret r\n}\n";
        let mut got = codes(src);
        got.sort();
        assert_eq!(got, [("E602".into(), 2, 4), ("E602".into(), 6, 4)]);
    }

    #[test]
    fn literal_kinds() {
        let src = "fn f() -> Unit {\n    let a: f64 = 1\n    let b: i64 = 1.5\n    let c: bool = 0\n    ret ()\n}\n";
        assert_eq!(codes(src), [("E201".into(), 4, 18), ("E201".into(), 5, 19)]);
    }
}
