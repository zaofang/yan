//! HIR to the Yan syntax tree (16.4, `yan view --as yn`): `let` and `var` come back from the
//! first assignment of a name and R421's mutability rule, tail `return` becomes the tail value,
//! `len(x)` is `x.len()`, a lambda over `it` that is one field chain is an implicit subject. A form
//! Yan cannot spell is reported as the first construct outside the subset.

use std::collections::HashSet;

use yan_hir as h;
use yan_yn::ast::*;

type R<T> = Result<T, String>;

fn outside<T>(what: &str) -> R<T> {
    Err(what.to_owned())
}

/// `m` as a Yan module the Yan printer can print.
pub fn to_yn(m: &h::Module) -> R<Module> {
    if !m.reexports.is_empty() {
        return outside("`pub use` re-exports");
    }
    let var_methods = m
        .items
        .iter()
        .flat_map(fns_of)
        .filter(|f| {
            matches!(
                f.params.first(),
                Some(h::Param::Value {
                    conv: Some(h::Conv::Var),
                    ..
                })
            )
        })
        .map(|f| f.name.clone())
        .collect();
    let mut c = Conv {
        scopes: Vec::new(),
        mutable: HashSet::new(),
        var_methods,
        self_ty: None,
    };
    Ok(Module {
        header: m
            .header
            .iter()
            .map(|h| h.trim_start_matches('#').trim().to_owned())
            .collect(),
        doc: m.doc.clone(),
        attrs: m.attrs.iter().map(attr).collect::<R<_>>()?,
        items: m.items.iter().map(|i| c.item(i)).collect::<R<_>>()?,
        cst: Default::default(),
    })
}

fn fns_of(i: &h::Item) -> Vec<&h::FuncDef> {
    match &i.kind {
        h::ItemKind::Func(f) | h::ItemKind::Lemma(f) | h::ItemKind::Sub(f) => vec![f],
        h::ItemKind::Impl(i) => i.fns.iter().chain(&i.subs).collect(),
        h::ItemKind::Trait(t) => t.sigs.iter().map(|(_, f)| f).collect(),
        _ => Vec::new(),
    }
}

const NO: Span = Span {
    line: 0,
    col: 0,
    end_line: 0,
    end_col: 0,
};

/// List and dict methods whose receiver is `var` (R421).
const MUTATING: [&str; 11] = [
    "append", "pop", "insert", "extend", "remove", "clear", "sort", "reverse", "add", "discard",
    "update",
];

struct Conv {
    /// Names bound so far in each open block, parameters first.
    scopes: Vec<HashSet<String>>,
    /// The current function's locals that R421 makes `var`.
    mutable: HashSet<String>,
    /// Module functions whose first parameter is `var`.
    var_methods: HashSet<String>,
    /// The type a bare `self` stands for: an impl's, a trait's `Self`; none in a free function.
    self_ty: Option<h::Type>,
}

fn attr(d: &h::Decorator) -> R<Attr> {
    Ok(Attr {
        name: d.path.join("."),
        args: d.args.iter().flatten().map(args_one).collect::<R<_>>()?,
        span: NO,
    })
}

fn args_one(a: &h::Arg) -> R<Arg> {
    let kind = match &a.kind {
        h::ArgKind::Expr(e) => ArgKind::Value(match &e.kind {
            h::ExprKind::Lambda(ps, body) if ps.len() == 1 && ps[0] == "it" => {
                match subject(body) {
                    Some(s) => s?,
                    None => expr(e)?,
                }
            }
            _ => expr(e)?,
        }),
        h::ArgKind::Ref(e) => ArgKind::Ref(expr(e)?),
        h::ArgKind::Become(e) => ArgKind::Become(expr(e)?),
    };
    Ok(Arg {
        label: a.name.clone(),
        kind,
        span: NO,
    })
}

fn ty(t: &h::Type) -> R<Type> {
    let kind = match t {
        h::Type::None => TypeKind::Unit,
        h::Type::Named { path, args } => {
            let targs = |args: &[h::TArg]| -> R<Vec<Type>> {
                args.iter()
                    .map(|a| match a {
                        h::TArg::Type(t) => ty(t),
                        _ => outside("a dimension or error union type argument"),
                    })
                    .collect()
            };
            match (path.as_slice(), args.len()) {
                ([n], 1) if n == "Option" => TypeKind::Option(Box::new(targs(args)?.remove(0))),
                ([n], 1) if n == "Ref" => TypeKind::Ref(Box::new(targs(args)?.remove(0))),
                ([n], 1) if n == "Ptr" => TypeKind::Ptr(Box::new(targs(args)?.remove(0))),
                ([n], _) if n == "tuple" => TypeKind::Tuple(targs(args)?),
                _ => {
                    let name = match path.as_slice() {
                        [n] => yan_names::type_row(n, yan_names::Surface::Yir)
                            .filter(|r| r.arity == args.len())
                            .map(|r| r.yn.to_owned()),
                        _ => None,
                    };
                    TypeKind::Path {
                        path: name.map_or_else(|| path.clone(), |n| vec![n]),
                        args: args
                            .iter()
                            .map(|a| match a {
                                h::TArg::Type(t) => Ok(TArg::Type(ty(t)?)),
                                h::TArg::Int(n) => Ok(TArg::Nat(n.clone())),
                                h::TArg::Text(_) => outside("a dimension or error union argument"),
                            })
                            .collect::<R<_>>()?,
                    }
                }
            }
        }
        h::Type::Dyn { path, args } => TypeKind::Dyn {
            name: path.join("."),
            args: args
                .iter()
                .map(|a| match a {
                    h::TArg::Type(t) => ty(t),
                    _ => outside("a dimension or error union argument"),
                })
                .collect::<R<_>>()?,
        },
        h::Type::Callable {
            par,
            params,
            ret,
            fail,
            uses,
        } => TypeKind::Fn {
            par: *par,
            params: params.iter().map(ty).collect::<R<_>>()?,
            ret: Box::new(ty(ret)?),
            fail: union(fail)?.map(Box::new),
            uses: (!uses.is_empty()).then(|| {
                uses.iter()
                    .map(|n| CapItem::Named {
                        name: n.clone(),
                        ty: None,
                    })
                    .collect()
            }),
        },
        h::Type::Call { path, args: a } => TypeKind::Call {
            path: path.clone(),
            args: a.iter().map(args_one).collect::<R<_>>()?,
        },
    };
    Ok(Type { kind, span: NO })
}

/// One type, or a `|` union of several.
fn union(ts: &[h::Type]) -> R<Option<Type>> {
    Ok(match ts {
        [] => None,
        [t] => Some(ty(t)?),
        many => Some(Type {
            kind: TypeKind::Union(many.iter().map(ty).collect::<R<_>>()?),
            span: NO,
        }),
    })
}

fn tparams(ts: &[h::TParam]) -> R<Vec<TParam>> {
    ts.iter()
        .map(|t| {
            Ok(match t {
                h::TParam::Type { name, bounds } => TParam::Type {
                    name: name.clone(),
                    bounds: bounds
                        .iter()
                        .map(|b| {
                            Ok(match b {
                                h::Bound::Nat => Bound::Nat,
                                h::Bound::Path { path, args } => Bound::Trait {
                                    path: path.clone(),
                                    args: args.iter().map(ty).collect::<R<_>>()?,
                                },
                            })
                        })
                        .collect::<R<_>>()?,
                },
                h::TParam::Value { name, ty: t } => TParam::Value {
                    name: name.clone(),
                    ty: ty(t)?,
                },
            })
        })
        .collect()
}

fn caps(cs: &[h::CapRef]) -> Vec<CapItem> {
    cs.iter()
        .map(|c| match &c.alias {
            Some(a) => CapItem::Named {
                name: a.clone(),
                ty: Some(c.path.join(".")),
            },
            None => CapItem::Named {
                name: c.path.join("."),
                ty: None,
            },
        })
        .collect()
}

fn field(f: &h::Field) -> R<Field> {
    Ok(Field {
        attrs: f.decorators.iter().map(attr).collect::<R<_>>()?,
        name: f.name.clone(),
        ty: ty(&f.ty)?,
        default: f.default.as_ref().map(expr).transpose()?,
        span: NO,
    })
}

fn fn_doc(f: &h::FuncDef) -> Option<String> {
    f.body.as_ref().and_then(|b| b.doc.clone())
}

impl Conv {
    fn item(&mut self, i: &h::Item) -> R<Item> {
        let doc = match &i.kind {
            h::ItemKind::Func(f) | h::ItemKind::Lemma(f) | h::ItemKind::Sub(f) => fn_doc(f),
            h::ItemKind::Record(r) => r.doc.clone(),
            h::ItemKind::Trait(t) => t.doc.clone(),
            _ => None,
        };
        let kind = match &i.kind {
            h::ItemKind::Func(f) => ItemKind::Fn(self.func(f, FnKind::Plain)?),
            h::ItemKind::Lemma(f) => ItemKind::Fn(self.func(f, FnKind::Lemma)?),
            h::ItemKind::Sub(f) => ItemKind::Fn(self.func(f, FnKind::Sub)?),
            h::ItemKind::Record(r) => ItemKind::Type(TypeDef {
                name: r.name.clone(),
                name_span: NO,
                tparams: tparams(&r.tparams)?,
                rhs: TypeRhs::Record {
                    fields: r.fields.iter().map(field).collect::<R<_>>()?,
                    positional: (r.positional > 0).then_some(r.positional),
                },
                refine: None,
            }),
            h::ItemKind::Type(t) => ItemKind::Type(TypeDef {
                name: t.name.clone(),
                name_span: NO,
                tparams: tparams(&t.tparams)?,
                rhs: match &t.rhs {
                    h::TypeRhs::Sum(vs) => TypeRhs::Sum(
                        vs.iter()
                            .map(|v| {
                                Ok(Variant {
                                    attrs: v.decorators.iter().map(attr).collect::<R<_>>()?,
                                    name: v.name.clone(),
                                    fields: v.fields.iter().map(field).collect::<R<_>>()?,
                                    span: NO,
                                })
                            })
                            .collect::<R<_>>()?,
                    ),
                    h::TypeRhs::Alias(ts) => {
                        TypeRhs::Alias(union(ts)?.ok_or_else(|| "an empty type alias".to_owned())?)
                    }
                    h::TypeRhs::Newtype(t) => TypeRhs::Newtype(ty(t)?),
                    h::TypeRhs::Caps(cs) => TypeRhs::CapSet(caps(cs)),
                },
                refine: t.where_.as_ref().map(expr).transpose()?,
            }),
            h::ItemKind::Const { name, ty: t, value } => ItemKind::Const(ConstDef {
                name: name.clone(),
                ty: ty(t)?,
                value: expr(value)?,
            }),
            h::ItemKind::Trait(t) => {
                self.self_ty = Some(h::Type::bare("Self"));
                let def = TraitDef {
                    name: t.name.clone(),
                    tparams: tparams(&t.tparams)?,
                    assoc: t.assoc.clone(),
                    methods: t
                        .sigs
                        .iter()
                        .map(|(decs, f)| {
                            let d = self.func(f, FnKind::Plain)?;
                            Ok(TraitMethod {
                                attrs: decs.iter().map(attr).collect::<R<_>>()?,
                                sig: d.sig,
                                default: match d.body {
                                    Body::Block { contracts, stmts }
                                        if f.body.is_some()
                                            && (!stmts.is_empty() || !contracts.is_empty()) =>
                                    {
                                        Some(Body::Block { contracts, stmts })
                                    }
                                    b if f.body.is_some() => Some(b),
                                    _ => None,
                                },
                            })
                        })
                        .collect::<R<_>>()?,
                    derived: None,
                };
                self.self_ty = None;
                ItemKind::Trait(def)
            }
            h::ItemKind::Impl(i) => {
                self.self_ty = Some(i.for_ty.clone());
                let mut items = Vec::new();
                for (fs, k) in [(&i.fns, FnKind::Plain), (&i.subs, FnKind::Sub)] {
                    for f in fs {
                        items.push(Item {
                            doc: fn_doc(f),
                            attrs: Vec::new(),
                            vis: h::Vis::Private,
                            kind: ItemKind::Fn(self.func(f, k)?),
                            span: NO,
                        });
                    }
                }
                let def = ImplDef {
                    unsafe_: false,
                    trait_path: i.trait_.clone(),
                    trait_args: i.args.iter().map(ty).collect::<R<_>>()?,
                    for_type: ty(&i.for_ty)?,
                    assoc: i
                        .assoc
                        .iter()
                        .map(|(n, t)| Ok((n.clone(), ty(t)?)))
                        .collect::<R<_>>()?,
                    items,
                };
                self.self_ty = None;
                ItemKind::Impl(def)
            }
            h::ItemKind::Extern(_) => return outside("`extern` blocks"),
            h::ItemKind::Verbatim(_) => return outside("an erased lemma"),
        };
        Ok(Item {
            doc,
            attrs: i.decorators.iter().map(attr).collect::<R<_>>()?,
            vis: i.vis,
            kind,
            span: NO,
        })
    }

    fn func(&mut self, f: &h::FuncDef, kind: FnKind) -> R<FnDef> {
        let params = f
            .params
            .iter()
            .map(|p| {
                Ok(match p {
                    h::Param::Slash => Param::PosOnly(NO),
                    h::Param::Pack { name, ty: t } => Param::Rest {
                        name: name.clone(),
                        ty: ty(t)?,
                        span: NO,
                    },
                    h::Param::Value {
                        conv,
                        name,
                        ty: t,
                        default,
                        ..
                    } => Param::Value {
                        mode: match conv {
                            None => ParamMode::Let,
                            Some(h::Conv::Var) => ParamMode::Var,
                            Some(h::Conv::Take) => ParamMode::Take,
                            Some(h::Conv::Ghost) => ParamMode::Ghost,
                        },
                        name: name.clone(),
                        ty: (name != "self" || self.self_ty.as_ref() != Some(t))
                            .then(|| ty(t))
                            .transpose()?,
                        default: default.as_ref().map(expr).transpose()?,
                        span: NO,
                    },
                })
            })
            .collect::<R<_>>()?;
        let ret_ty = f.ret.as_ref().and_then(|r| r.ty.as_ref());
        let sig = Sig {
            name: f.name.clone(),
            name_span: NO,
            tparams: tparams(&f.tparams)?,
            params,
            ret: ret_ty.map(ty).transpose()?,
            from: f.ret.as_ref().map(|r| r.from.clone()).unwrap_or_default(),
            fail: match &f.ret {
                Some(r) => union(&r.fail)?,
                None => None,
            },
            uses: match &f.uses {
                None => None,
                Some(h::Uses::None) => Some(vec![CapItem::None]),
                Some(h::Uses::Caps(cs)) => Some(caps(cs)),
            },
            span: NO,
        };
        let body = match &f.body {
            None => Body::Block {
                contracts: Vec::new(),
                stmts: vec![Stmt {
                    kind: StmtKind::Expr(Expr {
                        kind: ExprKind::Hole,
                        span: NO,
                    }),
                    span: NO,
                }],
            },
            Some(b) => self.body(f, b, ret_ty.is_some())?,
        };
        Ok(FnDef {
            unsafe_: f.unsafe_at.map(|_| NO),
            kind,
            sig,
            body,
        })
    }

    fn body(&mut self, f: &h::FuncDef, b: &h::Body, value: bool) -> R<Body> {
        self.mutable = mutables(&b.stmts, &self.var_methods);
        let mut top = HashSet::new();
        for p in &f.params {
            if let h::Param::Value { name, .. } | h::Param::Pack { name, .. } = p {
                top.insert(name.clone());
            }
        }
        self.scopes = vec![top];
        let mut stmts = b.stmts.clone();
        if value {
            untail(&mut stmts);
        }
        let contracts = b
            .contracts
            .iter()
            .flat_map(contract)
            .collect::<R<Vec<_>>>()?;
        let (out, _) = self.block(&stmts)?;
        if contracts.is_empty()
            && let [
                Stmt {
                    kind: StmtKind::Expr(e),
                    ..
                },
            ] = out.as_slice()
        {
            return Ok(Body::Expr(e.clone()));
        }
        Ok(Body::Block {
            contracts,
            stmts: out,
        })
    }

    /// The block's statements and the names it first bound, in a new scope.
    fn block(&mut self, stmts: &[h::Stmt]) -> R<(Vec<Stmt>, HashSet<String>)> {
        self.scopes.push(HashSet::new());
        let out = self.stmts(stmts);
        let bound = self.scopes.pop().expect("pushed above");
        Ok((out?, bound))
    }

    fn stmts(&mut self, stmts: &[h::Stmt]) -> R<Vec<Stmt>> {
        let mut out = Vec::new();
        for (i, s) in stmts.iter().enumerate() {
            let (st, leaked) = self.stmt(s)?;
            // R832: a name bound on every path is visible after the statement; Yan's `let` is not.
            if !leaked.is_empty() {
                let mut used = HashSet::new();
                visit(&stmts[i + 1..], &mut |e| {
                    if let h::ExprKind::Name(n) = &e.kind {
                        used.insert(n.clone());
                    }
                });
                if let Some(n) = leaked.iter().find(|n| used.contains(*n)) {
                    return Err(format!(
                        "`{n}` bound inside a block and used after it (R832)"
                    ));
                }
            }
            out.push(st);
        }
        Ok(out)
    }

    fn declared(&self, n: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(n))
    }

    fn declare(&mut self, n: &str) {
        self.scopes
            .last_mut()
            .expect("in a body")
            .insert(n.to_owned());
    }

    fn pattern_binds(&mut self, p: &Pattern) {
        match &p.kind {
            PatKind::Bind(n) => self.declare(n),
            PatKind::Tuple(ps) => ps.iter().for_each(|p| self.pattern_binds(p)),
            PatKind::As(p, n) => {
                self.pattern_binds(p);
                self.declare(n);
            }
            PatKind::Ctor {
                args: Some(args), ..
            } => {
                for a in args {
                    match a {
                        PPat::Pos(p) | PPat::Named(_, Some(p)) => self.pattern_binds(p),
                        PPat::Named(n, None) => self.declare(n),
                        PPat::Rest => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// The statement and the names its blocks bound on every path through it.
    fn stmt(&mut self, s: &h::Stmt) -> R<(Stmt, HashSet<String>)> {
        use h::StmtKind as K;
        let mut leaked = HashSet::new();
        let kind = match &s.kind {
            K::Expr(e) => StmtKind::Expr(expr(e)?),
            K::Assign {
                target,
                ty: t,
                value,
            } => return Ok((self.assign(target, t, value)?, leaked)),
            K::AugAssign { target, op, value } => StmtKind::Assign {
                place: expr(target)?,
                op,
                value: expr(value)?,
            },
            K::Ghost { name, ty: t, value } => {
                self.declare(name);
                StmtKind::Let {
                    mutable: false,
                    ghost: true,
                    pattern: bind(name),
                    ty: t.as_ref().map(ty).transpose()?,
                    value: expr(value)?,
                }
            }
            K::Bind {
                mutable,
                target,
                ty: t,
                value,
                ..
            } => {
                self.declare(target);
                StmtKind::Let {
                    mutable: *mutable,
                    ghost: false,
                    pattern: bind(target),
                    ty: t.as_ref().map(ty).transpose()?,
                    value: expr(value)?,
                }
            }
            K::Return(e) => StmtKind::Return(e.as_ref().map(expr).transpose()?),
            K::Yield(e) => StmtKind::Yield(expr(e)?),
            K::Become(e) => StmtKind::Become(expr(e)?),
            K::Fail(e) => StmtKind::Fail(expr(e)?),
            K::Break => StmtKind::Break(None),
            K::Continue => StmtKind::Continue(None),
            K::BreakTo(l) => StmtKind::Break(Some(l.clone())),
            K::ContinueTo(l) => StmtKind::Continue(Some(l.clone())),
            K::Assert(e) => StmtKind::Assert(expr(e)?),
            K::Unreachable => StmtKind::Unreachable,
            K::Reveal(p) => StmtKind::Reveal(p.clone()),
            K::Pass => return outside("`pass` outside a sole body or arm"),
            K::Unsafe(body) => StmtKind::Unsafe(self.block(body)?.0),
            K::If { arms, els } => {
                let mut branches = Vec::new();
                let mut sets = Vec::new();
                for (c, b) in arms {
                    let (stmts, bound) = self.block(&b.stmts)?;
                    branches.push((expr(c)?, stmts));
                    if falls_through(&b.stmts) {
                        sets.push(bound);
                    }
                }
                let otherwise = match els {
                    Some(b) => {
                        let (stmts, bound) = self.block(&b.stmts)?;
                        if falls_through(&b.stmts) {
                            sets.push(bound);
                        }
                        Some(stmts)
                    }
                    None => {
                        sets.clear();
                        None
                    }
                };
                if let Some(first) = sets.first() {
                    leaked = first
                        .iter()
                        .filter(|n| sets.iter().all(|s| s.contains(*n)))
                        .cloned()
                        .collect();
                }
                StmtKind::If {
                    branches,
                    otherwise,
                }
            }
            K::While {
                label,
                cond,
                contracts,
                body,
            } => {
                let invariants = loop_contracts(contracts)?;
                let body = self.block(body)?.0;
                match &cond.kind {
                    h::ExprKind::Lit(h::Lit::Bool(true)) => StmtKind::Loop {
                        label: label.clone(),
                        invariants,
                        body,
                    },
                    _ => StmtKind::While {
                        label: label.clone(),
                        cond: expr(cond)?,
                        invariants,
                        body,
                    },
                }
            }
            K::For {
                label,
                target,
                iter,
                contracts,
                body,
            } => {
                let pattern = targets(target);
                self.scopes.push(HashSet::new());
                self.pattern_binds(&pattern);
                let body = self.block(body);
                self.scopes.pop();
                StmtKind::For {
                    label: label.clone(),
                    mutable: false,
                    pattern,
                    iter: expr(iter)?,
                    invariants: loop_contracts(contracts)?,
                    body: body?.0,
                }
            }
            K::Match { scrutinee, cases } => StmtKind::Match {
                scrutinee: expr(scrutinee)?,
                arms: cases.iter().map(|c| self.arm(c)).collect::<R<_>>()?,
            },
            K::With { kind, body } => {
                let (stmts, bound) = self.block(&body.stmts)?;
                leaked = bound;
                StmtKind::With {
                    kind: match kind {
                        h::WithKind::Bind(n, e) => WithKind::Bind {
                            name: n.clone(),
                            dynamic: None,
                            value: expr(e)?,
                        },
                        h::WithKind::Deadline(e) => WithKind::Deadline(expr(e)?),
                        h::WithKind::Timeout(e) => WithKind::Timeout(expr(e)?),
                        h::WithKind::Shield(e) => WithKind::Shield(expr(e)?),
                        h::WithKind::Supervisor(e) => WithKind::Supervisor(expr(e)?),
                    },
                    body: stmts,
                }
            }
            K::Par {
                concurrent,
                limit: None,
                body: h::ParBody::Block(body),
            } => StmtKind::Par {
                concurrent: *concurrent,
                body: self.block(body)?.0,
            },
            K::Par { .. } => return outside("`par for` and `par(limit)`"),
            K::StageIf { cond, then, els } => StmtKind::StageIf {
                cond: expr(cond)?,
                then: self.block(then)?.0,
                otherwise: els
                    .as_ref()
                    .map(|b| self.block(b).map(|b| b.0))
                    .transpose()?,
            },
            K::StageFor { target, iter, body } => StmtKind::StageFor {
                pattern: bind(target),
                iter: expr(iter)?,
                body: self.block(body)?.0,
            },
            K::Asm { .. } => return outside("`asm`"),
            K::Select(_) => return outside("`select`"),
        };
        Ok((Stmt { kind, span: NO }, leaked))
    }

    fn assign(&mut self, target: &h::Expr, t: &Option<h::Type>, value: &h::Expr) -> R<Stmt> {
        let names: Option<Vec<&String>> = match &target.kind {
            h::ExprKind::Name(n) => Some(vec![n]),
            h::ExprKind::Tuple(xs) => xs
                .iter()
                .map(|x| match &x.kind {
                    h::ExprKind::Name(n) => Some(n),
                    _ => None,
                })
                .collect(),
            _ => None,
        };
        let fresh = names
            .as_ref()
            .is_some_and(|ns| ns.iter().all(|n| *n != "_" && !self.declared(n)));
        let kind = match (names, fresh) {
            (Some(ns), true) => {
                let mutable = ns.iter().any(|n| self.mutable.contains(*n));
                ns.iter().for_each(|n| self.declare(n));
                StmtKind::Let {
                    mutable,
                    ghost: false,
                    pattern: match ns.as_slice() {
                        [n] => bind(n),
                        many => Pattern {
                            kind: PatKind::Tuple(many.iter().map(|n| bind(n)).collect()),
                            span: NO,
                        },
                    },
                    ty: t.as_ref().map(ty).transpose()?,
                    value: expr(value)?,
                }
            }
            (Some(ns), false)
                if matches!(target.kind, h::ExprKind::Tuple(_))
                    && ns.iter().any(|n| !self.declared(n)) =>
            {
                return outside("a tuple assignment that binds and assigns");
            }
            _ => StmtKind::Assign {
                place: expr(target)?,
                op: "=",
                value: expr(value)?,
            },
        };
        Ok(Stmt { kind, span: NO })
    }

    fn arm(&mut self, c: &h::Case) -> R<Arm> {
        let pattern = pat(&c.pat)?;
        self.scopes.push(HashSet::new());
        self.pattern_binds(&pattern);
        let body = match c.body.stmts.as_slice() {
            [
                h::Stmt {
                    kind: h::StmtKind::Pass,
                    ..
                },
            ] => Ok(ArmBody::Expr(Expr {
                kind: ExprKind::Unit,
                span: NO,
            })),
            _ => self
                .stmts(&c.body.stmts)
                .map(|stmts| match stmts.as_slice() {
                    [
                        Stmt {
                            kind: StmtKind::Expr(e),
                            ..
                        },
                    ] => ArmBody::Expr(e.clone()),
                    _ => ArmBody::Block(stmts),
                }),
        };
        let guard = c.guard.as_ref().map(expr).transpose();
        self.scopes.pop();
        Ok(Arm {
            pattern,
            guard: guard?,
            body: body?,
            span: NO,
        })
    }
}

fn bind(n: &str) -> Pattern {
    Pattern {
        kind: PatKind::Bind(n.to_owned()),
        span: NO,
    }
}

fn targets(ns: &[String]) -> Pattern {
    match ns {
        [n] => bind(n),
        many => Pattern {
            kind: PatKind::Tuple(many.iter().map(|n| bind(n)).collect()),
            span: NO,
        },
    }
}

fn contract(c: &h::Contract) -> Vec<R<yan_yn::ast::Contract>> {
    let mk = |kind, e: &h::Expr| {
        expr(e).map(|expr| yan_yn::ast::Contract {
            kind,
            expr,
            span: NO,
        })
    };
    match c {
        h::Contract::Pre(e) => vec![mk(ContractKind::Pre, e)],
        h::Contract::Post(e) => vec![mk(ContractKind::Post, e)],
        h::Contract::Decreases(e) => vec![mk(ContractKind::Decreases, e)],
        h::Contract::Ex { expr: e, with } if with.is_empty() => vec![mk(ContractKind::Ex, e)],
        h::Contract::Ex { .. } => vec![Err("an `ex` contract with inputs".to_owned())],
        h::Contract::Cost { steps, alloc } => steps
            .iter()
            .map(|e| mk(ContractKind::CostSteps, e))
            .chain(alloc.iter().map(|e| mk(ContractKind::CostAlloc, e)))
            .collect(),
    }
}

fn loop_contracts(cs: &[h::LoopContract]) -> R<Vec<yan_yn::ast::Contract>> {
    cs.iter()
        .map(|c| {
            let (kind, e) = match c {
                h::LoopContract::Invariant(e) => (ContractKind::Invariant, e),
                h::LoopContract::Decreases(e) => (ContractKind::Decreases, e),
            };
            Ok(yan_yn::ast::Contract {
                kind,
                expr: expr(e)?,
                span: NO,
            })
        })
        .collect()
}

fn pat(p: &h::Pat) -> R<Pattern> {
    let kind = match p {
        h::Pat::Wild => PatKind::Wild,
        h::Pat::Lit(e) => PatKind::Lit(Box::new(expr(e)?)),
        h::Pat::Path { path, args: None }
            if path.len() == 1 && !path[0].starts_with(char::is_uppercase) =>
        {
            PatKind::Bind(path[0].clone())
        }
        h::Pat::Path { path, args } => PatKind::Ctor {
            path: path.clone(),
            args: args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|(n, p)| {
                            Ok(match n {
                                None => PPat::Pos(pat(p)?),
                                Some(n) => PPat::Named(n.clone(), Some(pat(p)?)),
                            })
                        })
                        .collect::<R<_>>()
                })
                .transpose()?,
        },
        h::Pat::Tuple(ps) => PatKind::Tuple(ps.iter().map(pat).collect::<R<_>>()?),
        h::Pat::As(p, n) => PatKind::As(Box::new(pat(p)?), n.clone()),
    };
    Ok(Pattern { kind, span: NO })
}

/// The inverse of the lowering's `tail_return`: a returned tail is the tail value.
fn untail(stmts: &mut [h::Stmt]) {
    let Some(last) = stmts.last_mut() else {
        return;
    };
    match &mut last.kind {
        h::StmtKind::Return(Some(e)) if !matches!(&e.kind, h::ExprKind::Unary(op, _) if *op == "-" || *op == "+") =>
        {
            last.kind = h::StmtKind::Expr(e.clone());
        }
        h::StmtKind::If { arms, els } => {
            arms.iter_mut().for_each(|(_, b)| untail(&mut b.stmts));
            if let Some(b) = els {
                untail(&mut b.stmts);
            }
        }
        h::StmtKind::Match { cases, .. } => {
            cases.iter_mut().for_each(|c| untail(&mut c.body.stmts))
        }
        h::StmtKind::With { body, .. } => untail(&mut body.stmts),
        h::StmtKind::Unsafe(body) => untail(body),
        _ => {}
    }
}

/// Whether control can leave `stmts` through its end (not by `return`, `fail`, `break`, `continue`).
fn falls_through(stmts: &[h::Stmt]) -> bool {
    !matches!(
        stmts.last().map(|s| &s.kind),
        Some(
            h::StmtKind::Return(_)
                | h::StmtKind::Fail(_)
                | h::StmtKind::Become(_)
                | h::StmtKind::Break
                | h::StmtKind::Continue
                | h::StmtKind::BreakTo(_)
                | h::StmtKind::ContinueTo(_)
                | h::StmtKind::Unreachable
        )
    )
}

// ---- R421: which locals are `var`

fn mutables(stmts: &[h::Stmt], var_methods: &HashSet<String>) -> HashSet<String> {
    let out = std::cell::RefCell::new(HashSet::new());
    let mut assigned: std::collections::HashMap<String, usize> = Default::default();
    visit_stmts(
        stmts,
        &mut |s| match &s.kind {
            h::StmtKind::Assign { target, .. } => {
                let ts: Vec<&h::Expr> = match &target.kind {
                    h::ExprKind::Tuple(ts) => ts.iter().collect(),
                    _ => vec![target],
                };
                for t in ts {
                    match &t.kind {
                        h::ExprKind::Name(n) => *assigned.entry(n.clone()).or_default() += 1,
                        _ => out.borrow_mut().extend(root(t).map(str::to_owned)),
                    }
                }
            }
            h::StmtKind::AugAssign { target, .. } => {
                out.borrow_mut().extend(root(target).map(str::to_owned))
            }
            _ => {}
        },
        &mut |e| {
            if let h::ExprKind::Call(f, args) = &e.kind {
                for a in args {
                    if let h::ArgKind::Ref(x) = &a.kind {
                        out.borrow_mut().extend(root(x).map(str::to_owned));
                    }
                }
                if let h::ExprKind::Attr(recv, m, _) = &f.kind
                    && (MUTATING.contains(&m.as_str()) || var_methods.contains(m))
                {
                    out.borrow_mut().extend(root(recv).map(str::to_owned));
                }
            }
        },
    );
    let mut out = out.into_inner();
    out.extend(assigned.into_iter().filter(|(_, k)| *k > 1).map(|(n, _)| n));
    out
}

fn root(e: &h::Expr) -> Option<&str> {
    match &e.kind {
        h::ExprKind::Name(n) => Some(n),
        h::ExprKind::Attr(x, ..) | h::ExprKind::Index(x, _) | h::ExprKind::Slice(x, ..) => root(x),
        _ => None,
    }
}

// ---- walking the HIR

fn visit(stmts: &[h::Stmt], fe: &mut dyn FnMut(&h::Expr)) {
    visit_stmts(stmts, &mut |_| {}, fe);
}

fn visit_stmts(stmts: &[h::Stmt], fs: &mut dyn FnMut(&h::Stmt), fe: &mut dyn FnMut(&h::Expr)) {
    for s in stmts {
        fs(s);
        let mut exprs: Vec<&h::Expr> = Vec::new();
        let mut blocks: Vec<&[h::Stmt]> = Vec::new();
        stmt_parts(s, &mut exprs, &mut blocks);
        for e in exprs {
            visit_expr(e, fs, fe);
        }
        for b in blocks {
            visit_stmts(b, fs, fe);
        }
    }
}

fn visit_expr(e: &h::Expr, fs: &mut dyn FnMut(&h::Stmt), fe: &mut dyn FnMut(&h::Expr)) {
    fe(e);
    let mut exprs: Vec<&h::Expr> = Vec::new();
    let mut blocks: Vec<&[h::Stmt]> = Vec::new();
    expr_parts(e, &mut exprs, &mut blocks);
    for x in exprs {
        visit_expr(x, fs, fe);
    }
    for b in blocks {
        visit_stmts(b, fs, fe);
    }
}

fn stmt_parts<'a>(s: &'a h::Stmt, es: &mut Vec<&'a h::Expr>, bs: &mut Vec<&'a [h::Stmt]>) {
    use h::StmtKind as K;
    match &s.kind {
        K::Expr(e) | K::Yield(e) | K::Become(e) | K::Fail(e) | K::Assert(e) => es.push(e),
        K::Assign { target, value, .. } | K::AugAssign { target, value, .. } => {
            es.push(target);
            es.push(value);
        }
        K::Ghost { value, .. } | K::Bind { value, .. } => es.push(value),
        K::Return(e) => es.extend(e),
        K::Unsafe(b) => bs.push(b),
        K::StageIf { cond, then, els } => {
            es.push(cond);
            bs.push(then);
            bs.extend(els.as_deref());
        }
        K::StageFor { iter, body, .. } => {
            es.push(iter);
            bs.push(body);
        }
        K::If { arms, els } => {
            for (c, b) in arms {
                es.push(c);
                bs.push(&b.stmts);
            }
            bs.extend(els.as_ref().map(|b| b.stmts.as_slice()));
        }
        K::While {
            cond,
            contracts,
            body,
            ..
        } => {
            es.push(cond);
            es.extend(contracts.iter().map(|c| match c {
                h::LoopContract::Invariant(e) | h::LoopContract::Decreases(e) => e,
            }));
            bs.push(body);
        }
        K::For {
            iter,
            contracts,
            body,
            ..
        } => {
            es.push(iter);
            es.extend(contracts.iter().map(|c| match c {
                h::LoopContract::Invariant(e) | h::LoopContract::Decreases(e) => e,
            }));
            bs.push(body);
        }
        K::Match { scrutinee, cases } => {
            es.push(scrutinee);
            for c in cases {
                es.extend(&c.guard);
                bs.push(&c.body.stmts);
            }
        }
        K::With { kind, body } => {
            match kind {
                h::WithKind::Bind(_, e)
                | h::WithKind::Deadline(e)
                | h::WithKind::Timeout(e)
                | h::WithKind::Shield(e)
                | h::WithKind::Supervisor(e) => es.push(e),
            }
            bs.push(&body.stmts);
        }
        K::Par { limit, body, .. } => {
            es.extend(limit);
            match body {
                h::ParBody::Block(b) => bs.push(b),
                h::ParBody::For { iter, body, .. } => {
                    es.push(iter);
                    bs.push(body);
                }
            }
        }
        K::Select(arms) => {
            for a in arms {
                es.push(&a.op);
                bs.push(&a.body.stmts);
            }
        }
        K::Asm { args, .. } => {
            for a in args {
                if let h::AsmArg::Operand { value, .. } = a {
                    es.push(value);
                }
            }
        }
        K::Break
        | K::Continue
        | K::BreakTo(_)
        | K::ContinueTo(_)
        | K::Pass
        | K::Unreachable
        | K::Reveal(_) => {}
    }
}

fn expr_parts<'a>(e: &'a h::Expr, es: &mut Vec<&'a h::Expr>, bs: &mut Vec<&'a [h::Stmt]>) {
    use h::ExprKind as K;
    match &e.kind {
        K::Name(_) | K::Lit(_) | K::Hole | K::Wild => {}
        K::FStr { pieces, .. } => {
            for p in pieces {
                if let h::FPiece::Expr(x, _) = p {
                    es.push(x);
                }
            }
        }
        K::Tuple(xs) | K::List(xs) | K::Set(xs) => es.extend(xs),
        K::Dict(kvs) => kvs.iter().for_each(|(k, v)| es.extend([k, v])),
        K::Comp {
            elem,
            value,
            iter,
            cond,
            ..
        } => {
            es.push(elem);
            es.extend(value.as_deref());
            es.push(iter);
            es.extend(cond.as_deref());
        }
        K::Attr(x, ..)
        | K::Try(x)
        | K::Unary(_, x)
        | K::Not(x)
        | K::Reify(x)
        | K::Cast(x, _)
        | K::Stage(x) => es.push(x),
        K::Call(f, args) => {
            es.push(f);
            for a in args {
                match &a.kind {
                    h::ArgKind::Expr(x) | h::ArgKind::Ref(x) | h::ArgKind::Become(x) => es.push(x),
                }
            }
        }
        K::Index(a, b) | K::Binary(_, a, b) | K::Compare(_, a, b) => es.extend([&**a, &**b]),
        K::Slice(x, parts, _) => {
            es.push(x);
            es.extend(parts.iter().flatten().map(|p| &**p));
        }
        K::Cond { then, cond, els } => es.extend([&**then, &**cond, &**els]),
        K::Lambda(_, body) => es.push(body),
        K::Fallback(x, fb) => {
            es.push(x);
            match &**fb {
                h::Fallback::Value(v) | h::Fallback::Bind(_, v) | h::Fallback::Fail(v) => {
                    es.push(v)
                }
                h::Fallback::Return(v) => es.extend(v),
                h::Fallback::Block(_, ss) => bs.push(ss),
                h::Fallback::Break | h::Fallback::Continue => {}
            }
        }
    }
}

// ---- expressions

fn bx(e: &h::Expr) -> R<Box<Expr>> {
    Ok(Box::new(expr(e)?))
}

fn mk(kind: ExprKind) -> Expr {
    Expr { kind, span: NO }
}

fn binop(op: &str) -> R<BinOp> {
    Ok(match op {
        "or" => BinOp::Or,
        "and" => BinOp::And,
        "==" => BinOp::Eq,
        "!=" => BinOp::Ne,
        "<" => BinOp::Lt,
        "<=" => BinOp::Le,
        ">" => BinOp::Gt,
        ">=" => BinOp::Ge,
        "in" => BinOp::In,
        "not in" => BinOp::NotIn,
        "|" => BinOp::BitOr,
        "^" => BinOp::BitXor,
        "&" => BinOp::BitAnd,
        "<<" => BinOp::Shl,
        ">>" => BinOp::Shr,
        "+" => BinOp::Add,
        "-" => BinOp::Sub,
        "*" => BinOp::Mul,
        "/" => BinOp::Div,
        "//" => BinOp::FloorDiv,
        "%" => BinOp::Mod,
        "@" => BinOp::MatMul,
        "**" => BinOp::Pow,
        _ => return Err(format!("the operator `{op}`")),
    })
}

/// `it.a.b(c)`: the chain a lambda over `it` is, as an implicit subject (15.3).
fn subject(e: &h::Expr) -> Option<R<Expr>> {
    use h::ExprKind as K;
    let kind = match &e.kind {
        K::Attr(x, n, _) => match &x.kind {
            K::Name(it) if it == "it" => ExprKind::Subject(n.clone()),
            _ => ExprKind::Field(Box::new(subject(x)?.ok()?), n.clone()),
        },
        K::Call(f, args) => ExprKind::Call(
            Box::new(subject(f)?.ok()?),
            args.iter().map(args_one).collect::<R<_>>().ok()?,
        ),
        K::Index(x, i) => ExprKind::Index(
            Box::new(subject(x)?.ok()?),
            vec![IxArg::Expr(expr(i).ok()?)],
        ),
        K::Try(x) => ExprKind::Question(Box::new(subject(x)?.ok()?)),
        _ => return None,
    };
    Some(Ok(mk(kind)))
}

pub fn expr(e: &h::Expr) -> R<Expr> {
    use h::ExprKind as K;
    let kind = match &e.kind {
        K::Name(n) if n == "self" => ExprKind::SelfValue,
        K::Name(n) => ExprKind::Name(n.clone()),
        K::Lit(h::Lit::Int(s)) => ExprKind::Int(s.clone()),
        K::Lit(h::Lit::Float(s)) => ExprKind::Float(s.clone()),
        K::Lit(h::Lit::Char(c)) => ExprKind::Char(c.to_string()),
        K::Lit(h::Lit::Bool(b)) => ExprKind::Bool(*b),
        K::Lit(h::Lit::None) => ExprKind::NoneLit,
        K::Lit(h::Lit::Str { prefix, value }) => ExprKind::Str(StrLit {
            kind: match prefix.as_str() {
                "" => StrKind::Plain,
                "r" => StrKind::Raw,
                "b" => StrKind::Bytes,
                "c" => StrKind::C,
                _ => return outside("this string prefix"),
            },
            parts: vec![StrPart::Text(value.clone())],
        }),
        K::FStr { template, pieces } => ExprKind::Str(StrLit {
            kind: if *template {
                StrKind::Template
            } else {
                StrKind::Plain
            },
            parts: pieces
                .iter()
                .map(|p| {
                    Ok(match p {
                        h::FPiece::Lit(t) => StrPart::Text(t.clone()),
                        h::FPiece::Expr(x, spec) => StrPart::Interp {
                            expr: expr(x)?,
                            spec: spec.clone(),
                        },
                    })
                })
                .collect::<R<_>>()?,
        }),
        K::Hole => ExprKind::Hole,
        K::Wild => return outside("a wildcard expression"),
        K::Tuple(xs) => ExprKind::Tuple(xs.iter().map(expr).collect::<R<_>>()?),
        K::List(xs) => ExprKind::List(xs.iter().map(expr).collect::<R<_>>()?),
        K::Set(xs) => ExprKind::Set(xs.iter().map(expr).collect::<R<_>>()?),
        K::Dict(kvs) => ExprKind::Map(
            kvs.iter()
                .map(|(k, v)| Ok((expr(k)?, expr(v)?)))
                .collect::<R<_>>()?,
        ),
        K::Comp { .. } => return outside("comprehensions"),
        K::Attr(x, n, _) => ExprKind::Field(bx(x)?, n.clone()),
        // `x.len()` is the builtin `len(x)` (15.3).
        K::Call(f, args)
            if matches!(&f.kind, K::Name(n) if n == "len")
                && matches!(
                    args.as_slice(),
                    [h::Arg {
                        name: None,
                        kind: h::ArgKind::Expr(_),
                        ..
                    }]
                ) =>
        {
            let h::ArgKind::Expr(x) = &args[0].kind else {
                unreachable!("matched")
            };
            ExprKind::Call(
                Box::new(mk(ExprKind::Field(bx(x)?, "len".into()))),
                Vec::new(),
            )
        }
        K::Call(f, args) => ExprKind::Call(bx(f)?, args.iter().map(args_one).collect::<R<_>>()?),
        K::Index(x, i) => ExprKind::Index(bx(x)?, vec![IxArg::Expr(expr(i)?)]),
        K::Slice(x, [lo, hi, None], false) => ExprKind::Index(
            bx(x)?,
            vec![IxArg::Slice {
                lo: lo.as_deref().map(expr).transpose()?,
                hi: hi.as_deref().map(expr).transpose()?,
                inclusive: false,
            }],
        ),
        K::Slice(..) => return outside("stepped and inclusive slices"),
        K::Try(x) => ExprKind::Question(bx(x)?),
        K::Unary(op, x) => ExprKind::Unary(
            match *op {
                "-" => UnOp::Neg,
                "+" => UnOp::Pos,
                "~" => UnOp::BitNot,
                _ => return Err(format!("the operator `{op}`")),
            },
            bx(x)?,
        ),
        K::Not(x) => ExprKind::Unary(UnOp::Not, bx(x)?),
        K::Reify(x) => ExprKind::Unary(UnOp::Try, bx(x)?),
        K::Binary(op, l, r) => ExprKind::Binary(binop(op)?, bx(l)?, bx(r)?),
        K::Compare(op @ ("is" | "is not"), l, r) => {
            if !matches!(r.kind, K::Lit(h::Lit::None)) {
                return outside("`is` against anything but `None`");
            }
            ExprKind::Is {
                value: bx(l)?,
                negated: *op == "is not",
                pattern: Pattern {
                    kind: PatKind::Lit(Box::new(mk(ExprKind::NoneLit))),
                    span: NO,
                },
            }
        }
        K::Compare(op, l, r) => ExprKind::Binary(binop(op)?, bx(l)?, bx(r)?),
        K::Cond { then, cond, els } => ExprKind::If {
            cond: bx(cond)?,
            then: bx(then)?,
            otherwise: bx(els)?,
        },
        K::Fallback(x, fb) => ExprKind::Fallback(
            bx(x)?,
            Box::new(match &**fb {
                h::Fallback::Value(v) => Fallback::Value(expr(v)?),
                h::Fallback::Bind(n, v) => Fallback::Bind(n.clone(), expr(v)?),
                h::Fallback::Return(v) => {
                    Fallback::Diverge(stmt_of(StmtKind::Return(v.as_ref().map(expr).transpose()?)))
                }
                h::Fallback::Fail(v) => Fallback::Diverge(stmt_of(StmtKind::Fail(expr(v)?))),
                h::Fallback::Break => Fallback::Diverge(stmt_of(StmtKind::Break(None))),
                h::Fallback::Continue => Fallback::Diverge(stmt_of(StmtKind::Continue(None))),
                h::Fallback::Block(..) => return outside("a fallback block"),
            }),
        ),
        K::Lambda(ps, body) => ExprKind::Lambda {
            params: ps.clone(),
            body: bx(body)?,
        },
        K::Cast(x, t) => ExprKind::Cast(bx(x)?, ty(t)?),
        K::Stage(x) => ExprKind::Stage(bx(x)?),
    };
    Ok(mk(kind))
}

fn stmt_of(kind: StmtKind) -> Stmt {
    Stmt { kind, span: NO }
}

#[cfg(test)]
mod tests {
    fn yn(src: &str) -> String {
        let (m, d) = yan_yag::parse("t.yag", src, false);
        assert!(d.is_empty(), "{d:?}");
        yan_yn::print(&super::to_yn(&m.expect("parses")).expect("in the subset"))
    }

    #[test]
    fn let_var_tail_and_len() {
        let src = "def f(xs: list[int]) -> int:\n    n = 0\n    for x in xs:\n        n += x\n    k = len(xs)\n    return n + k\n";
        assert_eq!(
            yn(src),
            "f(xs: List[int]) -> int =\n  var n = 0\n  for x in xs\n    n += x\n  let k = xs.len()\n  n + k\n"
        );
    }

    #[test]
    fn docs_and_headers() {
        let src = "# spec: 1\n\n\"\"\"Module.\"\"\"\n\ndef f(x: int) -> int:\n    \"\"\"Add one.\"\"\"\n    return x + 1\n";
        let out = yn(src);
        println!("{out}");
        assert!(
            out.starts_with("-- spec: 1\n\n--- Module.\n\n--- Add one.\nf("),
            "{out}"
        );
        let (m, d) = yan_yn::parse("t.yn", &out);
        assert!(d.is_empty(), "{d:?}");
        assert_eq!(yan_yn::print(&m), out);
    }

    #[test]
    fn implicit_subject() {
        let src = "def f(xs: list[Pos]) -> list[str]:\n    return xs.map(lambda it: it.sym)\n";
        assert_eq!(yn(src), "f(xs: List[Pos]) -> List[str] = xs.map(.sym)\n");
    }

    #[test]
    fn outside_the_subset() {
        let (m, _) = yan_yag::parse(
            "t.yag",
            "def f(xs: list[int]) -> list[int]:\n    return [x for x in xs]\n",
            false,
        );
        assert_eq!(
            super::to_yn(&m.expect("parses")).unwrap_err(),
            "comprehensions"
        );
    }
}
