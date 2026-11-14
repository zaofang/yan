//! Yan to the shared high-level IR (15.3): the surface-specific parts of elaboration. Clause
//! form becomes a match on the parameters, a block's last expression its result, the implicit
//! subject a lambda, and non-`pub` functions get the capabilities their bodies use.

use std::collections::{HashMap, HashSet};

use yan_diag::{Diagnostic, Span};
use yan_hir as h;

use crate::ast::*;

/// A construct the shared IR has no node for yet, at the innermost statement or expression
/// that was being lowered (E017, R649).
#[derive(Debug)]
pub struct Unsupported {
    pub what: String,
    pub at: Option<Span>,
}

impl Unsupported {
    /// Keeps the innermost span: the first one set wins as the error propagates outward.
    fn at(mut self, span: Span) -> Self {
        self.at.get_or_insert(span);
        self
    }
}

type R<T> = Result<T, Unsupported>;

fn unsupported<T>(what: impl Into<String>) -> R<T> {
    Err(Unsupported {
        what: what.into(),
        at: None,
    })
}

/// The std capabilities a body can name without declaring them (7.1).
const CAPS: [&str; 9] = [
    "io", "net", "fs", "time", "rand", "log", "nondet", "env", "proc",
];

pub fn lower(file: &str, src: &str, m: &Module) -> R<(h::Module, Vec<Diagnostic>)> {
    let inferred = infer_caps(m);
    let inferred_fail = infer_fails(m);
    let mut l = Lower {
        file,
        src,
        diags: Vec::new(),
        inferred,
        inferred_fail,
        docs: m.docs().into_iter().collect(),
        doc: None,
        self_ty: named(&["Self"], Vec::new()),
        fresh: 0,
    };
    let mut items = Vec::new();
    for it in &m.items {
        match l.item(it) {
            Ok(Some(i)) => items.push(i),
            Ok(None) => {}
            // R649: report the construct and go on lowering the other items.
            Err(u) => {
                let msg = format!("{}: not supported by this toolchain yet", u.what);
                l.err("E017", u.at.unwrap_or(it.span), &msg);
            }
        }
    }
    let module = h::Module {
        header: Vec::new(),
        attrs: m.attrs.iter().map(|a| l.attr(a)).collect::<R<_>>()?,
        doc: None,
        items,
    };
    Ok((module, l.diags))
}

struct Lower<'a> {
    file: &'a str,
    src: &'a str,
    diags: Vec<Diagnostic>,
    inferred: HashMap<String, Vec<String>>,
    inferred_fail: HashMap<String, Vec<String>>,
    /// `---` lines by line number.
    docs: HashMap<u32, String>,
    /// The doc comment above the item being lowered.
    doc: Option<String>,
    /// What an unannotated `self` is: the impl's type, else `Self`.
    self_ty: h::Type,
    /// Names made up by desugaring, numbered.
    fresh: usize,
}

fn mk(kind: h::ExprKind, span: Span) -> h::Expr {
    h::Expr {
        kind,
        span,
        paren: false,
    }
}

fn stmt(kind: h::StmtKind, span: Span) -> h::Stmt {
    h::Stmt { kind, span }
}

fn named(path: &[&str], args: Vec<h::TArg>) -> h::Type {
    h::Type::Named {
        path: path.iter().map(|s| (*s).to_owned()).collect(),
        args,
    }
}

/// Whether a declared return type carries a value.
fn returns_value(sig: &Sig) -> bool {
    sig.ret
        .as_ref()
        .is_some_and(|t| !matches!(t.kind, TypeKind::Unit))
}

impl Lower<'_> {
    fn err(&mut self, code: &str, span: Span, msg: &str) {
        let line = self.src.lines().nth(span.line as usize - 1).unwrap_or("");
        let lo = (span.col as usize - 1).min(line.len());
        let hi = if span.end_line == span.line {
            (span.end_col as usize - 1).clamp(lo, line.len())
        } else {
            line.len()
        };
        let quote = line.get(lo..hi).unwrap_or("");
        self.diags
            .push(Diagnostic::error(code, self.file, span, quote, msg));
    }

    /// The consecutive `---` lines that end on the line before `line`.
    fn doc_above(&self, line: u32) -> Option<String> {
        let mut lines = Vec::new();
        let mut l = line.checked_sub(1)?;
        while let Some(t) = self.docs.get(&l) {
            lines.push(t.as_str());
            l = l.checked_sub(1)?;
        }
        if lines.is_empty() {
            return None;
        }
        lines.reverse();
        Some(lines.join("\n"))
    }

    fn attr(&mut self, a: &Attr) -> R<h::Decorator> {
        Ok(h::Decorator {
            path: vec![a.name.clone()],
            args: if a.args.is_empty() {
                None
            } else {
                Some(self.args(&a.args)?)
            },
        })
    }

    fn item(&mut self, it: &Item) -> R<Option<h::Item>> {
        let decorators = it.attrs.iter().map(|a| self.attr(a)).collect::<R<_>>()?;
        self.doc = self.doc_above(it.span.line);
        let (kind, name_span) = match &it.kind {
            ItemKind::Fn(f) => {
                let def = self.fn_def(&f.sig, Some(&f.body), !it.public)?;
                let span = f.sig.name_span;
                match f.kind {
                    FnKind::Plain => (h::ItemKind::Func(def), span),
                    FnKind::Lemma => (h::ItemKind::Lemma(def), span),
                    FnKind::Gen => return unsupported("generators"),
                    FnKind::Sub => return unsupported("subscript definitions"),
                }
            }
            ItemKind::Type(t) => (self.type_def(t)?, t.name_span),
            ItemKind::Const(c) => (
                h::ItemKind::Const {
                    name: c.name.clone(),
                    ty: self.ty(&c.ty)?,
                    value: self.expr(&c.value)?,
                },
                it.span,
            ),
            ItemKind::Trait(t) => (h::ItemKind::Trait(self.trait_def(t)?), it.span),
            ItemKind::Impl(i) => (h::ItemKind::Impl(self.impl_def(i)?), it.span),
            ItemKind::Extern(x) => (h::ItemKind::Extern(self.extern_def(x)?), it.span),
            ItemKind::Use(_) => return unsupported("use declarations"),
        };
        Ok(Some(h::Item {
            decorators,
            is_pub: it.public,
            kind,
            span: it.span,
            name_span,
        }))
    }

    fn extern_def(&mut self, x: &ExternDef) -> R<h::Extern> {
        let mut items = Vec::new();
        for xi in &x.items {
            items.push(match xi {
                XItem::Fn {
                    attrs,
                    safe,
                    name,
                    params,
                    ret,
                    from,
                    uses_none,
                    nonblocking,
                    span,
                } => {
                    if !from.is_empty() || attrs.iter().any(|a| a.name != "access") {
                        return unsupported(
                            "attributes other than `@access` and `from` on extern items",
                        );
                    }
                    let mut ps = Vec::new();
                    let mut variadic = false;
                    for p in params {
                        match p {
                            XParam::Variadic => variadic = true,
                            XParam::Typed { name, callback, ty } => ps.push(h::XParam {
                                name: name.clone(),
                                callback: callback.map(|c| match c {
                                    Callback::Scoped => h::Callback::Scoped,
                                    Callback::Escaping => h::Callback::Escaping,
                                }),
                                ty: self.ty(ty)?,
                                ty_span: ty.span,
                            }),
                        }
                    }
                    h::XItem::Fn {
                        attrs: attrs.iter().map(|a| self.attr(a)).collect::<R<_>>()?,
                        safe: *safe,
                        name: name.clone(),
                        name_span: self.find(*span, name),
                        params: ps,
                        variadic,
                        ret: match ret {
                            Some(t) => Some((self.ty(t)?, t.span)),
                            None => None,
                        },
                        uses_none: *uses_none,
                        nonblocking: *nonblocking,
                    }
                }
                XItem::Static {
                    attrs,
                    safe,
                    name,
                    ty,
                    ..
                } => {
                    if !attrs.is_empty() {
                        return unsupported("attributes on extern items");
                    }
                    h::XItem::Static {
                        safe: *safe,
                        name: name.clone(),
                        ty: self.ty(ty)?,
                        ty_span: ty.span,
                    }
                }
                XItem::Type { name, drop, .. } => h::XItem::Type {
                    name: name.clone(),
                    drop: drop.clone(),
                },
            });
        }
        Ok(h::Extern {
            abi: x.abi.clone(),
            lib: x.lib.clone(),
            link: x.link.clone(),
            items,
        })
    }

    fn asm_arg(&mut self, a: &AsmArg, at: Span) -> R<h::AsmArg> {
        let reg = |r: &AsmReg| match r {
            AsmReg::Class(n) => h::AsmReg::Class(n.clone()),
            AsmReg::Explicit(n) => {
                h::AsmReg::Explicit(n.clone(), self.find(at, &format!("\"{n}\"")))
            }
        };
        Ok(match a {
            AsmArg::Operand { dir, reg: r, value } => h::AsmArg::Operand {
                dir: dir.clone(),
                reg: reg(r),
                value: self.expr(value)?,
            },
            AsmArg::Typed {
                dir,
                reg: r,
                name,
                ty,
            } => h::AsmArg::Typed {
                dir: dir.clone(),
                reg: reg(r),
                name: name.clone(),
                ty: self.ty(ty)?,
            },
            AsmArg::ClobberAbi(s) => h::AsmArg::ClobberAbi(s.clone()),
            AsmArg::Option(n) => h::AsmArg::Option(n.clone()),
        })
    }

    /// The span of the first `text` at or after `from`, on its line or the lines below.
    fn find(&self, from: Span, text: &str) -> Span {
        for (k, line) in self.src.lines().enumerate().skip(from.line as usize - 1) {
            let start = if k + 1 == from.line as usize {
                (from.col as usize - 1).min(line.len())
            } else {
                0
            };
            if let Some(i) = line[start..].find(text) {
                let col = (start + i + 1) as u32;
                let line = k as u32 + 1;
                return Span {
                    line,
                    col,
                    end_line: line,
                    end_col: col + text.chars().count() as u32,
                };
            }
        }
        from
    }

    fn trait_def(&mut self, t: &TraitDef) -> R<h::Trait> {
        if t.derived.is_some() {
            return unsupported("derived traits");
        }
        let doc = self.doc.take();
        let mut sigs = Vec::new();
        for m in &t.methods {
            let decs = m.attrs.iter().map(|a| self.attr(a)).collect::<R<_>>()?;
            self.doc = self.doc_above(m.sig.span.line);
            sigs.push((decs, self.fn_def(&m.sig, m.default.as_ref(), false)?));
        }
        Ok(h::Trait {
            name: t.name.clone(),
            tparams: self.tparams(&t.tparams)?,
            doc,
            assoc: t.assoc.clone(),
            sigs,
        })
    }

    fn impl_def(&mut self, i: &ImplDef) -> R<h::Impl> {
        if i.unsafe_ {
            return unsupported("`unsafe impl`");
        }
        let for_ty = self.ty(&i.for_type)?;
        let outer = std::mem::replace(&mut self.self_ty, for_ty.clone());
        let fns = self.impl_fns(&i.items, FnKind::Plain);
        let subs = self.impl_fns(&i.items, FnKind::Sub);
        self.self_ty = outer;
        Ok(h::Impl {
            trait_: i.trait_path.clone(),
            args: i.trait_args.iter().map(|t| self.ty(t)).collect::<R<_>>()?,
            for_ty,
            assoc: i
                .assoc
                .iter()
                .map(|(n, t)| Ok((n.clone(), self.ty(t)?)))
                .collect::<R<_>>()?,
            fns: fns?,
            subs: subs?,
        })
    }

    fn impl_fns(&mut self, items: &[Item], want: FnKind) -> R<Vec<h::FuncDef>> {
        let mut fns = Vec::new();
        for it in items {
            let ItemKind::Fn(f) = &it.kind else {
                return unsupported("this impl item");
            };
            if !matches!(f.kind, FnKind::Plain | FnKind::Sub) {
                return unsupported("generators in an impl");
            }
            if f.kind != want {
                continue;
            }
            self.doc = self.doc_above(it.span.line);
            fns.push(self.fn_def(&f.sig, Some(&f.body), false)?);
        }
        Ok(fns)
    }

    fn type_def(&mut self, t: &TypeDef) -> R<h::ItemKind> {
        let tparams = self.tparams(&t.tparams)?;
        let rhs = match &t.rhs {
            TypeRhs::Record { fields, positional } => {
                if positional.is_some() {
                    return unsupported("positional-only record fields");
                }
                return Ok(h::ItemKind::Record(h::Record {
                    name: t.name.clone(),
                    tparams,
                    doc: self.doc.clone(),
                    fields: fields.iter().map(|f| self.field(f)).collect::<R<_>>()?,
                }));
            }
            TypeRhs::Sum(vs) => h::TypeRhs::Sum(
                vs.iter()
                    .map(|v| {
                        Ok(h::Variant {
                            decorators: v.attrs.iter().map(|a| self.attr(a)).collect::<R<_>>()?,
                            name: v.name.clone(),
                            fields: v.fields.iter().map(|f| self.field(f)).collect::<R<_>>()?,
                        })
                    })
                    .collect::<R<_>>()?,
            ),
            TypeRhs::Alias(ty) => h::TypeRhs::Alias(match &ty.kind {
                TypeKind::Union(ts) => ts.iter().map(|t| self.ty(t)).collect::<R<_>>()?,
                _ => vec![self.ty(ty)?],
            }),
            TypeRhs::Newtype(ty) => h::TypeRhs::Newtype(self.ty(ty)?),
            TypeRhs::CapSet(caps) => h::TypeRhs::Caps(self.caps(caps)),
        };
        Ok(h::ItemKind::Type(h::TypeDef {
            name: t.name.clone(),
            tparams,
            rhs,
            where_: t.refine.as_ref().map(|e| self.expr(e)).transpose()?,
        }))
    }

    fn field(&mut self, f: &Field) -> R<h::Field> {
        Ok(h::Field {
            decorators: f.attrs.iter().map(|a| self.attr(a)).collect::<R<_>>()?,
            name: f.name.clone(),
            ty: self.ty(&f.ty)?,
            ty_span: f.ty.span,
            default: f.default.as_ref().map(|e| self.expr(e)).transpose()?,
        })
    }

    fn tparams(&mut self, ts: &[TParam]) -> R<Vec<h::TParam>> {
        ts.iter()
            .map(|t| {
                Ok(match t {
                    TParam::Type { name, bounds } => h::TParam::Type {
                        name: name.clone(),
                        bounds: bounds
                            .iter()
                            .map(|b| {
                                Ok(match b {
                                    Bound::Nat => h::Bound::Nat,
                                    Bound::Trait { path, args } => h::Bound::Path {
                                        path: path.clone(),
                                        args: args.iter().map(|t| self.ty(t)).collect::<R<_>>()?,
                                    },
                                })
                            })
                            .collect::<R<_>>()?,
                    },
                    TParam::Value { name, ty } => h::TParam::Value {
                        name: name.clone(),
                        ty: self.ty(ty)?,
                    },
                })
            })
            .collect()
    }

    fn caps(&self, caps: &[CapItem]) -> Vec<h::CapRef> {
        caps.iter()
            .filter_map(|c| match c {
                CapItem::Named { name, ty: None } => Some(h::CapRef {
                    path: vec![name.clone()],
                    alias: None,
                }),
                CapItem::Named { name, ty: Some(t) } => Some(h::CapRef {
                    path: vec![t.clone()],
                    alias: Some(name.clone()),
                }),
                CapItem::Set(s) => Some(h::CapRef {
                    path: vec![s.clone()],
                    alias: None,
                }),
                CapItem::None => None,
            })
            .collect()
    }

    fn fail_paths(&mut self, t: &Type) -> R<Vec<h::Path>> {
        match &t.kind {
            TypeKind::Path { path, args } if args.is_empty() => Ok(vec![match path.as_slice() {
                [n] if yan_type(n, 0).is_some() => vec![yan_type(n, 0).expect("checked").into()],
                _ => path.clone(),
            }]),
            TypeKind::Union(ts) => {
                let mut out = Vec::new();
                for t in ts {
                    out.extend(self.fail_paths(t)?);
                }
                Ok(out)
            }
            _ => unsupported("this failure type"),
        }
    }

    fn ty(&mut self, t: &Type) -> R<h::Type> {
        Ok(match &t.kind {
            TypeKind::Path { path, args } => {
                let mut a = Vec::new();
                for arg in args {
                    a.push(match arg {
                        TArg::Type(t) => h::TArg::Type(self.ty(t)?),
                        TArg::Nat(n) => h::TArg::Int(n.clone()),
                    });
                }
                let p: Vec<&str> = path.iter().map(String::as_str).collect();
                match (p.as_slice(), a.len()) {
                    ([n], k) if yan_type(n, k).is_some() => {
                        named(&[yan_type(n, k).expect("checked")], a)
                    }
                    _ => h::Type::Named {
                        path: path.clone(),
                        args: a,
                    },
                }
            }
            TypeKind::Unit => h::Type::None,
            TypeKind::Tuple(ts) => named(
                &["tuple"],
                ts.iter()
                    .map(|t| Ok(h::TArg::Type(self.ty(t)?)))
                    .collect::<R<_>>()?,
            ),
            TypeKind::Option(t) => named(&["Option"], vec![h::TArg::Type(self.ty(t)?)]),
            TypeKind::Fn {
                par,
                params,
                ret,
                fail,
                uses,
            } => h::Type::Callable {
                par: *par,
                params: params.iter().map(|t| self.ty(t)).collect::<R<_>>()?,
                ret: Box::new(self.ty(ret)?),
                fail: match fail {
                    Some(f) => self.fail_paths(f)?,
                    None => Vec::new(),
                },
                uses: uses
                    .iter()
                    .flatten()
                    .filter_map(|c| match c {
                        CapItem::Named { name, .. } => Some(name.clone()),
                        CapItem::Set(s) => Some(s.clone()),
                        CapItem::None => None,
                    })
                    .collect(),
            },
            TypeKind::Union(_) => return unsupported("union types outside `fail` and aliases"),
            TypeKind::Dyn { .. } => return unsupported("`dyn` types"),
            TypeKind::Ptr(t) => named(&["Ptr"], vec![h::TArg::Type(self.ty(t)?)]),
            TypeKind::Ref(_) | TypeKind::Vec { .. } => {
                return unsupported("Ref and Vec types");
            }
            TypeKind::Record(_) => return unsupported("anonymous record types"),
            TypeKind::Call { path, args } => h::Type::Call {
                path: path.clone(),
                args: self.args(args)?,
            },
        })
    }

    /// `infer` is false for pub functions and for trait and impl methods, whose names are not
    /// module-level.
    fn fn_def(&mut self, s: &Sig, body: Option<&Body>, infer: bool) -> R<h::FuncDef> {
        let mut params = Vec::new();
        for p in &s.params {
            params.push(match p {
                Param::PosOnly(_) => h::Param::Slash,
                Param::Rest { name, ty, .. } => h::Param::Pack {
                    name: name.clone(),
                    ty: self.ty(ty)?,
                },
                Param::Value {
                    mode,
                    name,
                    ty,
                    default,
                    span,
                } => {
                    let ty = match ty {
                        Some(t) => self.ty(t)?,
                        None if name == "self" => self.self_ty.clone(),
                        None => {
                            self.err("E240", *span, "parameters are always annotated; add `: T`");
                            named(&["_"], Vec::new())
                        }
                    };
                    h::Param::Value {
                        conv: match mode {
                            ParamMode::Let => None,
                            ParamMode::Var => Some(h::Conv::Var),
                            ParamMode::Take => Some(h::Conv::Take),
                            ParamMode::Ghost => Some(h::Conv::Ghost),
                        },
                        name: name.clone(),
                        span: *span,
                        ty,
                        default: default.as_ref().map(|e| self.expr(e)).transpose()?,
                    }
                }
            });
        }
        let inferred_fail: Vec<h::Path> = match &s.fail {
            None if infer => self
                .inferred_fail
                .get(&s.name)
                .map(|ns| ns.iter().map(|n| vec![n.clone()]).collect())
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let ret = if s.ret.is_some() || s.fail.is_some() || !inferred_fail.is_empty() {
            Some(h::RetType {
                ty: match &s.ret {
                    Some(t) if !matches!(t.kind, TypeKind::Unit) => Some(self.ty(t)?),
                    _ => None,
                },
                fail: match &s.fail {
                    Some(t) => self.fail_paths(t)?,
                    None => inferred_fail,
                },
                from: s.from.clone(),
            })
        } else {
            None
        };
        let uses = match &s.uses {
            Some(caps) if caps.iter().any(|c| matches!(c, CapItem::None)) => Some(h::Uses::None),
            Some(caps) => Some(h::Uses::Caps(self.caps(caps))),
            None if !infer => None,
            None => self
                .inferred
                .get(&s.name)
                .filter(|c| !c.is_empty())
                .map(|cs| {
                    h::Uses::Caps(
                        cs.iter()
                            .map(|c| h::CapRef {
                                path: vec![c.clone()],
                                alias: None,
                            })
                            .collect(),
                    )
                }),
        };
        let body = body.map(|b| self.body(s, b)).transpose()?;
        Ok(h::FuncDef {
            name: s.name.clone(),
            name_span: s.name_span,
            tparams: self.tparams(&s.tparams)?,
            params,
            ret,
            uses,
            body,
        })
    }

    fn body(&mut self, s: &Sig, b: &Body) -> R<h::Body> {
        let value = returns_value(s);
        Ok(match b {
            Body::Expr(e) => {
                let span = e.span;
                let mut stmts = self.tail_expr(e, span)?;
                if value {
                    tail_return(&mut stmts);
                }
                h::Body {
                    doc: self.doc.clone(),
                    contracts: Vec::new(),
                    stmts,
                }
            }
            Body::Block { contracts, stmts } => {
                let mut out = self.stmts(stmts)?;
                if value {
                    tail_return(&mut out);
                }
                h::Body {
                    doc: self.doc.clone(),
                    contracts: self.contracts(contracts)?,
                    stmts: out,
                }
            }
            Body::Clauses { contracts, arms } => {
                let names: Vec<_> = s
                    .params
                    .iter()
                    .filter_map(|p| match p {
                        Param::Value { name, span, .. } => Some((name.clone(), *span)),
                        _ => None,
                    })
                    .collect();
                let span = arms.first().map(|a| a.span).unwrap_or(s.span);
                let scrutinee = match names.as_slice() {
                    [(n, sp)] => mk(h::ExprKind::Name(n.clone()), *sp),
                    many => mk(
                        h::ExprKind::Tuple(
                            many.iter()
                                .map(|(n, sp)| mk(h::ExprKind::Name(n.clone()), *sp))
                                .collect(),
                        ),
                        span,
                    ),
                };
                let cases = self.arms(arms)?;
                let mut out = vec![stmt(h::StmtKind::Match { scrutinee, cases }, span)];
                if value {
                    tail_return(&mut out);
                }
                h::Body {
                    doc: self.doc.clone(),
                    contracts: self.contracts(contracts)?,
                    stmts: out,
                }
            }
        })
    }

    /// A single-expression body or arm: a unit `()` is no statement at all.
    fn tail_expr(&mut self, e: &Expr, span: Span) -> R<Vec<h::Stmt>> {
        if matches!(e.kind, ExprKind::Unit) {
            return Ok(vec![stmt(h::StmtKind::Pass, span)]);
        }
        Ok(vec![stmt(h::StmtKind::Expr(self.expr(e)?), span)])
    }

    fn contracts(&mut self, cs: &[Contract]) -> R<Vec<h::Contract>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < cs.len() {
            let c = &cs[i];
            i += 1;
            let e = self.expr(&c.expr)?;
            out.push(match c.kind {
                ContractKind::Pre => h::Contract::Pre(e),
                ContractKind::Post => h::Contract::Post(e),
                ContractKind::Ex => h::Contract::Ex {
                    expr: e,
                    with: Vec::new(),
                },
                ContractKind::Decreases => h::Contract::Decreases(e),
                ContractKind::Invariant => return unsupported("`invariant` outside a loop"),
                ContractKind::CostSteps => {
                    // The `alloc` bound of the same line joins its `steps` bound.
                    let alloc = match cs.get(i) {
                        Some(n)
                            if n.kind == ContractKind::CostAlloc && n.span.line == c.span.line =>
                        {
                            i += 1;
                            Some(self.expr(&n.expr)?)
                        }
                        _ => None,
                    };
                    h::Contract::Cost {
                        steps: Some(e),
                        alloc,
                    }
                }
                ContractKind::CostAlloc => h::Contract::Cost {
                    steps: None,
                    alloc: Some(e),
                },
            });
        }
        Ok(out)
    }

    fn loop_contracts(&mut self, cs: &[Contract]) -> R<Vec<h::LoopContract>> {
        cs.iter()
            .map(|c| {
                let e = self.expr(&c.expr)?;
                Ok(match c.kind {
                    ContractKind::Invariant => h::LoopContract::Invariant(e),
                    _ => h::LoopContract::Decreases(e),
                })
            })
            .collect()
    }

    fn arms(&mut self, arms: &[Arm]) -> R<Vec<h::Case>> {
        let mut out = Vec::new();
        for a in arms {
            out.extend(self.arm(a)?);
        }
        Ok(out)
    }

    /// Or, range and slice patterns are Yan's alone (R558), so they stay here: an or-pattern
    /// becomes one case per alternative, and a range or slice pattern a fresh name with a guard.
    fn arm(&mut self, a: &Arm) -> R<Vec<h::Case>> {
        let span = a.pattern.span;
        let alts: Vec<&Pattern> = match &a.pattern.kind {
            PatKind::Or(ps) => ps.iter().collect(),
            _ => vec![&a.pattern],
        };
        let mut out = Vec::new();
        for p in alts {
            let mut guard = a.guard.as_ref().map(|g| self.expr(g)).transpose()?;
            let mut prefix = Vec::new();
            let pat = match &p.kind {
                PatKind::Range { lo, hi, inclusive } => {
                    let t = self.fresh_name("r");
                    let v = mk(h::ExprKind::Name(t.clone()), p.span);
                    let low = mk(
                        h::ExprKind::Compare("<=", Box::new(self.expr(lo)?), Box::new(v.clone())),
                        p.span,
                    );
                    let op = if *inclusive { "<=" } else { "<" };
                    let high = mk(
                        h::ExprKind::Compare(op, Box::new(v), Box::new(self.expr(hi)?)),
                        p.span,
                    );
                    guard = Some(and_all([Some(low), Some(high), guard], p.span));
                    bind_pat(t)
                }
                PatKind::Slice(ps) => {
                    let t = self.fresh_name("s");
                    let (check, binds) = self.slice(&t, ps, p.span)?;
                    guard = Some(and_all([Some(check), guard], p.span));
                    prefix = binds;
                    bind_pat(t)
                }
                _ => self.pattern(p)?,
            };
            let mut body = self.arm_body(&a.body, a.span)?;
            if !prefix.is_empty() {
                // The slice's names are bound by a one-case match, like any pattern's names.
                let (names, values): (Vec<_>, Vec<_>) = prefix.into_iter().unzip();
                let (pat, scrutinee) = if names.len() == 1 {
                    (
                        bind_pat(names[0].clone()),
                        values.into_iter().next().expect("one"),
                    )
                } else {
                    (
                        h::Pat::Tuple(names.into_iter().map(bind_pat).collect()),
                        mk(h::ExprKind::Tuple(values), span),
                    )
                };
                let inner = h::Case {
                    pat,
                    span,
                    guard: None,
                    body,
                };
                body = h::Block {
                    inline: false,
                    stmts: vec![stmt(
                        h::StmtKind::Match {
                            scrutinee,
                            cases: vec![inner],
                        },
                        span,
                    )],
                };
            }
            out.push(h::Case {
                pat,
                span,
                guard,
                body,
            });
        }
        Ok(out)
    }

    fn fresh_name(&mut self, base: &str) -> Name {
        self.fresh += 1;
        format!("_{base}{}", self.fresh)
    }

    /// `[a, b, ..rest, z]` against the list named `t`: the length test, and each name with
    /// the value it binds. Elements are names or `_`.
    fn slice(&mut self, t: &str, ps: &[SPat], at: Span) -> R<(h::Expr, Vec<(Name, h::Expr)>)> {
        let name = |n: &str| mk(h::ExprKind::Name(n.to_owned()), at);
        let int = |k: usize| mk(h::ExprKind::Lit(h::Lit::Int(k.to_string())), at);
        let len = || {
            mk(
                h::ExprKind::Call(
                    Box::new(name("len")),
                    vec![h::Arg {
                        name: None,
                        kind: h::ArgKind::Expr(name(t)),
                        span: at,
                    }],
                ),
                at,
            )
        };
        let rest = ps.iter().position(|p| matches!(p, SPat::Rest(_)));
        let fixed = ps.len() - usize::from(rest.is_some());
        let check = mk(
            h::ExprKind::Compare(
                if rest.is_some() { ">=" } else { "==" },
                Box::new(len()),
                Box::new(int(fixed)),
            ),
            at,
        );
        let mut binds = Vec::new();
        let mut assign = |target: &str, value: h::Expr| binds.push((target.to_owned(), value));
        for (k, p) in ps.iter().enumerate() {
            let after = rest.is_some_and(|r| k > r);
            let index = if after {
                // Counted from the end: `len(t) - (ps.len() - k)`.
                mk(
                    h::ExprKind::Binary("-", Box::new(len()), Box::new(int(ps.len() - k))),
                    at,
                )
            } else {
                int(k)
            };
            match p {
                SPat::Pat(Pattern {
                    kind: PatKind::Bind(n),
                    ..
                }) => assign(
                    n,
                    mk(h::ExprKind::Index(Box::new(name(t)), Box::new(index)), at),
                ),
                SPat::Pat(Pattern {
                    kind: PatKind::Wild,
                    ..
                })
                | SPat::Rest(None) => {}
                SPat::Rest(Some(n)) => {
                    let hi = mk(
                        h::ExprKind::Binary("-", Box::new(len()), Box::new(int(ps.len() - k - 1))),
                        at,
                    );
                    // `[t[i] for i in range(k, hi)]`: elab has no slices yet.
                    let i = self.fresh_name("i");
                    let range = mk(
                        h::ExprKind::Call(
                            Box::new(name("range")),
                            [int(k), hi]
                                .into_iter()
                                .map(|e| h::Arg {
                                    name: None,
                                    kind: h::ArgKind::Expr(e),
                                    span: at,
                                })
                                .collect(),
                        ),
                        at,
                    );
                    let whole = mk(
                        h::ExprKind::Comp {
                            kind: h::CompKind::List,
                            par: None,
                            elem: Box::new(mk(
                                h::ExprKind::Index(Box::new(name(t)), Box::new(name(&i))),
                                at,
                            )),
                            value: None,
                            target: vec![i],
                            iter: Box::new(range),
                            cond: None,
                        },
                        at,
                    );
                    assign(n, whole);
                }
                SPat::Pat(_) => return unsupported("a nested pattern inside a slice pattern"),
            }
        }
        Ok((check, binds))
    }

    fn arm_body(&mut self, b: &ArmBody, span: Span) -> R<h::Block> {
        Ok(h::Block {
            inline: false,
            stmts: match b {
                ArmBody::Expr(e) => self.tail_expr(e, span)?,
                ArmBody::Block(stmts) => self.stmts(stmts)?,
            },
        })
    }

    fn block(&mut self, stmts: &[Stmt]) -> R<h::Block> {
        Ok(h::Block {
            inline: false,
            stmts: self.stmts(stmts)?,
        })
    }

    fn stmts(&mut self, stmts: &[Stmt]) -> R<Vec<h::Stmt>> {
        let mut out = Vec::new();
        for s in stmts {
            out.push(self.stmt(s)?);
        }
        Ok(out)
    }

    fn stmt(&mut self, s: &Stmt) -> R<h::Stmt> {
        self.stmt_inner(s).map_err(|u| u.at(s.span))
    }

    fn stmt_inner(&mut self, s: &Stmt) -> R<h::Stmt> {
        use h::StmtKind as K;
        let kind = match &s.kind {
            StmtKind::Expr(e) => match &e.kind {
                ExprKind::ParFor { .. } => return unsupported("`par for` statements"),
                ExprKind::Asm { template, args } => K::Asm {
                    template: template.clone(),
                    args: args
                        .iter()
                        .map(|a| self.asm_arg(a, e.span))
                        .collect::<R<_>>()?,
                },
                _ => K::Expr(self.expr(e)?),
            },
            StmtKind::Assign { place, op, value } => {
                let target = self.expr(place)?;
                let value = self.expr(value)?;
                if *op == "=" {
                    K::Assign {
                        target,
                        ty: None,
                        value,
                    }
                } else {
                    K::AugAssign { target, op, value }
                }
            }
            StmtKind::Let {
                mutable,
                ghost,
                pattern,
                ty,
                value,
            } => {
                let ty = ty.as_ref().map(|t| self.ty(t)).transpose()?;
                let PatKind::Bind(name) = &pattern.kind else {
                    return Ok(stmt(self.let_pattern(pattern, ty, value)?, s.span));
                };
                let value = match &value.kind {
                    ExprKind::ParFor {
                        concurrent,
                        limit: None,
                        pattern: p,
                        iter,
                        body,
                    } => self.par_comp(*concurrent, p, iter, body, value.span)?,
                    ExprKind::Block(_) | ExprKind::ParFor { .. } => {
                        return unsupported("block and `par for` values");
                    }
                    _ => self.expr(value)?,
                };
                if *ghost {
                    K::Ghost {
                        name: name.clone(),
                        ty,
                        value,
                    }
                } else {
                    K::Bind {
                        mutable: *mutable,
                        target: name.clone(),
                        target_span: pattern.span,
                        ty,
                        value,
                    }
                }
            }
            StmtKind::Return(e) => K::Return(e.as_ref().map(|e| self.expr(e)).transpose()?),
            StmtKind::Fail(e) => K::Fail(self.expr(e)?),
            StmtKind::Become(e) => K::Become(self.expr(e)?),
            StmtKind::Break(None) => K::Break,
            StmtKind::Continue(None) => K::Continue,
            StmtKind::Break(Some(n)) => K::BreakTo(n.clone()),
            StmtKind::Continue(Some(n)) => K::ContinueTo(n.clone()),
            StmtKind::Assert(e) => K::Assert(self.expr(e)?),
            StmtKind::Unreachable => K::Unreachable,
            StmtKind::Reveal(p) => K::Reveal(p.clone()),
            StmtKind::Yield(e) => K::Yield(self.expr(e)?),
            // `if e is P` with no `elif` is a match on `e` (R558: `is` stays in Yan).
            StmtKind::If {
                branches,
                otherwise,
            } if matches!(branches.as_slice(), [(Expr { kind: ExprKind::Is { negated: false, pattern, .. }, .. }, _)]
                if !is_none(pattern)) =>
            {
                let [(cond, then)] = branches.as_slice() else {
                    unreachable!("matched")
                };
                let ExprKind::Is { value, pattern, .. } = &cond.kind else {
                    unreachable!("matched")
                };
                let arm = Arm {
                    pattern: pattern.clone(),
                    guard: None,
                    body: ArmBody::Block(then.clone()),
                    span: cond.span,
                };
                let mut cases = self.arm(&arm)?;
                // The statement's span marks this `_` as written by the desugar, not the user.
                cases.push(h::Case {
                    pat: h::Pat::Wild,
                    span: s.span,
                    guard: None,
                    body: match otherwise {
                        Some(b) => self.block(b)?,
                        None => h::Block {
                            inline: false,
                            stmts: vec![stmt(K::Pass, cond.span)],
                        },
                    },
                });
                K::Match {
                    scrutinee: self.expr(value)?,
                    cases,
                }
            }
            StmtKind::If {
                branches,
                otherwise,
            } => K::If {
                arms: branches
                    .iter()
                    .map(|(c, b)| Ok((self.expr(c)?, self.block(b)?)))
                    .collect::<R<_>>()?,
                els: otherwise.as_ref().map(|b| self.block(b)).transpose()?,
            },
            StmtKind::Match { scrutinee, arms } => K::Match {
                scrutinee: self.expr(scrutinee)?,
                cases: self.arms(arms)?,
            },
            StmtKind::For {
                label,
                mutable: false,
                pattern,
                iter,
                invariants,
                body,
            } => K::For {
                label: label.clone(),
                target: self.targets(pattern)?,
                iter: self.expr(iter)?,
                contracts: self.loop_contracts(invariants)?,
                body: self.stmts(body)?,
            },
            StmtKind::For {
                label,
                mutable: true,
                pattern,
                iter,
                invariants,
                body,
            } => self.for_var(label, pattern, iter, invariants, body)?,
            StmtKind::While {
                label,
                cond,
                invariants,
                body,
            } => K::While {
                label: label.clone(),
                cond: self.expr(cond)?,
                contracts: self.loop_contracts(invariants)?,
                body: self.stmts(body)?,
            },
            StmtKind::Loop {
                label,
                invariants,
                body,
            } => K::While {
                label: label.clone(),
                cond: mk(h::ExprKind::Lit(h::Lit::Bool(true)), s.span),
                contracts: self.loop_contracts(invariants)?,
                body: self.stmts(body)?,
            },
            StmtKind::Par { concurrent, body } => K::Par {
                concurrent: *concurrent,
                limit: None,
                body: h::ParBody::Block(self.stmts(body)?),
            },
            StmtKind::With { kind, body } => K::With {
                kind: match kind {
                    WithKind::Bind { dynamic: true, .. } => {
                        return unsupported("`with name = dyn e`");
                    }
                    WithKind::Bind { name, value, .. } => {
                        h::WithKind::Bind(name.clone(), self.expr(value)?)
                    }
                    WithKind::Deadline(e) => h::WithKind::Deadline(self.expr(e)?),
                    WithKind::Timeout(e) => h::WithKind::Timeout(self.expr(e)?),
                    WithKind::Supervisor(e) => h::WithKind::Supervisor(self.expr(e)?),
                },
                body: self.block(body)?,
            },
            StmtKind::Select(arms) => K::Select(
                arms.iter()
                    .map(|a| {
                        Ok(h::SelectArm {
                            op: self.expr(&a.op)?,
                            pat: a.pattern.as_ref().map(|p| self.pattern(p)).transpose()?,
                            body: self.arm_body(&a.body, a.span)?,
                            span: a.span,
                        })
                    })
                    .collect::<R<_>>()?,
            ),
            StmtKind::Unsafe(body) => K::Unsafe(self.stmts(body)?),
            StmtKind::StageIf {
                cond,
                then,
                otherwise,
            } => K::StageIf {
                cond: self.expr(cond)?,
                then: self.stmts(then)?,
                els: otherwise.as_ref().map(|b| self.stmts(b)).transpose()?,
            },
            StmtKind::StageFor {
                pattern:
                    Pattern {
                        kind: PatKind::Bind(target),
                        ..
                    },
                iter,
                body,
            } => K::StageFor {
                target: target.clone(),
                iter: self.expr(iter)?,
                body: self.stmts(body)?,
            },
            StmtKind::StageFor { .. } => return unsupported("`#for` over a pattern"),
        };
        Ok(stmt(kind, s.span))
    }

    /// `for var x in xs` (R434): an index loop that reads each element into `x` and writes it
    /// back. The list is exclusively borrowed for the loop, so the body may not name it (E402).
    fn for_var(
        &mut self,
        label: &Option<Name>,
        pattern: &Pattern,
        iter: &Expr,
        invariants: &[Contract],
        body: &[Stmt],
    ) -> R<h::StmtKind> {
        let (PatKind::Bind(x), ExprKind::Name(xs)) = (&pattern.kind, &iter.kind) else {
            return unsupported("`for var` over anything but a named list");
        };
        if continues(body) {
            return unsupported("`continue` in a `for var` body");
        }
        let mut used = Names::new();
        body.iter()
            .for_each(|s| stmt_names(s, &mut used, &mut HashSet::new()));
        if let Some(at) = used.get(xs) {
            self.err(
                "E402",
                *at,
                &format!("`{xs}` is borrowed by `for var` for the whole loop; use `{x}`"),
            );
        }
        self.fresh += 1;
        let i = format!("_i{}", self.fresh);
        let (at, ps) = (iter.span, pattern.span);
        let name = |n: &str, sp| mk(h::ExprKind::Name(n.to_owned()), sp);
        let arg = |e| h::Arg {
            name: None,
            kind: h::ArgKind::Expr(e),
            span: at,
        };
        let len = mk(
            h::ExprKind::Call(Box::new(name("len", at)), vec![arg(name(xs, at))]),
            at,
        );
        let zero = mk(h::ExprKind::Lit(h::Lit::Int("0".into())), at);
        let range = mk(
            h::ExprKind::Call(Box::new(name("range", at)), vec![arg(zero), arg(len)]),
            at,
        );
        let elem = mk(
            h::ExprKind::Index(Box::new(name(xs, at)), Box::new(name(&i, at))),
            at,
        );
        let mut stmts = vec![stmt(
            h::StmtKind::Assign {
                target: name(x, ps),
                ty: None,
                value: elem.clone(),
            },
            ps,
        )];
        stmts.extend(self.stmts(body)?);
        stmts.push(stmt(
            h::StmtKind::Assign {
                target: elem,
                ty: None,
                value: name(x, ps),
            },
            ps,
        ));
        Ok(h::StmtKind::For {
            label: label.clone(),
            target: vec![i],
            iter: range,
            contracts: self.loop_contracts(invariants)?,
            body: stmts,
        })
    }

    /// `let p = e` with a pattern: a tuple of names unpacks, `let p = e else ...` is let-else.
    fn let_pattern(&mut self, p: &Pattern, ty: Option<h::Type>, value: &Expr) -> R<h::StmtKind> {
        if let ExprKind::Fallback(_, fb) = &value.kind {
            // `let Some(x) = e else ...` and `let Ok(x) = ...` are the fallback `x = e else ...`.
            let unwrapped = match &p.kind {
                PatKind::Ctor {
                    name,
                    args: Some(args),
                } if (name == "Some" || name == "Ok") => match args.as_slice() {
                    [
                        PPat::Pos(Pattern {
                            kind: PatKind::Bind(x),
                            span,
                        }),
                    ] => Some((x, *span)),
                    _ => None,
                },
                _ => None,
            };
            let Some((x, at)) = unwrapped else {
                return unsupported("let-else with a pattern other than `Some(x)` or `Ok(x)`");
            };
            if !matches!(fb.as_ref(), Fallback::Diverge(_) | Fallback::Block(None, _)) {
                return unsupported("this let-else");
            }
            return Ok(h::StmtKind::Assign {
                target: mk(h::ExprKind::Name(x.clone()), at),
                ty,
                value: self.expr(value)?,
            });
        }
        Ok(match &p.kind {
            PatKind::Wild => h::StmtKind::Expr(self.expr(value)?),
            PatKind::Tuple(ps) if ps.iter().all(|p| matches!(p.kind, PatKind::Bind(_))) => {
                let names = ps
                    .iter()
                    .map(|p| match &p.kind {
                        PatKind::Bind(n) => mk(h::ExprKind::Name(n.clone()), p.span),
                        _ => unreachable!("checked above"),
                    })
                    .collect();
                h::StmtKind::Assign {
                    target: mk(h::ExprKind::Tuple(names), p.span),
                    ty,
                    value: self.expr(value)?,
                }
            }
            _ => return unsupported("this pattern binding"),
        })
    }

    fn par_comp(
        &mut self,
        concurrent: bool,
        p: &Pattern,
        iter: &Expr,
        body: &[Stmt],
        span: Span,
    ) -> R<h::Expr> {
        let [
            Stmt {
                kind: StmtKind::Expr(elem),
                ..
            },
        ] = body
        else {
            return unsupported("a `par for` body of more than one expression");
        };
        Ok(mk(
            h::ExprKind::Comp {
                kind: h::CompKind::List,
                par: Some(concurrent),
                elem: Box::new(self.expr(elem)?),
                value: None,
                target: self.targets(p)?,
                iter: Box::new(self.expr(iter)?),
                cond: None,
            },
            span,
        ))
    }

    fn targets(&mut self, p: &Pattern) -> R<Vec<Name>> {
        match &p.kind {
            PatKind::Bind(n) => Ok(vec![n.clone()]),
            PatKind::Wild => Ok(vec!["_".to_owned()]),
            PatKind::Tuple(ps) => {
                let mut out = Vec::new();
                for p in ps {
                    match &p.kind {
                        PatKind::Bind(n) => out.push(n.clone()),
                        PatKind::Wild => out.push("_".to_owned()),
                        _ => return unsupported("nested loop patterns"),
                    }
                }
                Ok(out)
            }
            _ => unsupported("this loop pattern"),
        }
    }

    fn pattern(&mut self, p: &Pattern) -> R<h::Pat> {
        Ok(match &p.kind {
            PatKind::Wild => h::Pat::Wild,
            PatKind::Lit(e) => h::Pat::Lit(self.expr(e)?),
            PatKind::Bind(n) => h::Pat::Path {
                path: vec![n.clone()],
                args: None,
            },
            PatKind::Ctor { name, args } => h::Pat::Path {
                path: vec![name.clone()],
                args: match args {
                    None => None,
                    Some(args) => {
                        let mut out = Vec::new();
                        for a in args {
                            out.push(match a {
                                PPat::Pos(p) => (None, self.pattern(p)?),
                                PPat::Named(n, Some(p)) => (Some(n.clone()), self.pattern(p)?),
                                PPat::Named(n, None) => (
                                    Some(n.clone()),
                                    h::Pat::Path {
                                        path: vec![n.clone()],
                                        args: None,
                                    },
                                ),
                                PPat::Rest => return unsupported("`..` in a constructor pattern"),
                            });
                        }
                        Some(out)
                    }
                },
            },
            PatKind::Tuple(ps) => {
                h::Pat::Tuple(ps.iter().map(|p| self.pattern(p)).collect::<R<_>>()?)
            }
            PatKind::As(p, n) => h::Pat::As(Box::new(self.pattern(p)?), n.clone()),
            PatKind::Range { .. } | PatKind::Slice(_) | PatKind::Or(_) => {
                return unsupported("or, range and slice patterns below the top of an arm");
            }
        })
    }

    fn args(&mut self, args: &[Arg]) -> R<Vec<h::Arg>> {
        let mut out = Vec::new();
        for a in args {
            let kind = match &a.kind {
                ArgKind::Value(e) => h::ArgKind::Expr(self.expr(e)?),
                ArgKind::Ref(e) => h::ArgKind::Ref(self.expr(e)?),
                ArgKind::Become(e) => h::ArgKind::Become(self.expr(e)?),
                ArgKind::Pun => {
                    let name = a.label.clone().expect("a pun has a label");
                    h::ArgKind::Expr(mk(h::ExprKind::Name(name), a.span))
                }
            };
            out.push(h::Arg {
                name: a.label.clone(),
                kind,
                span: a.span,
            });
        }
        Ok(out)
    }

    fn lit(&mut self, kind: h::Lit, span: Span) -> h::Expr {
        mk(h::ExprKind::Lit(kind), span)
    }

    fn string(&mut self, s: &StrLit, span: Span) -> R<h::Expr> {
        let interpolated = s.parts.iter().any(|p| matches!(p, StrPart::Interp { .. }));
        if !interpolated && s.kind != StrKind::Template {
            let value = s
                .parts
                .iter()
                .map(|p| match p {
                    StrPart::Text(t) => t.as_str(),
                    StrPart::Interp { .. } => "",
                })
                .collect();
            let prefix = match s.kind {
                StrKind::Raw => "r",
                StrKind::Bytes => "b",
                StrKind::C => "c",
                _ => "",
            };
            return Ok(self.lit(
                h::Lit::Str {
                    prefix: prefix.to_owned(),
                    value,
                },
                span,
            ));
        }
        if matches!(s.kind, StrKind::Bytes | StrKind::C) {
            return unsupported("interpolated byte and C strings");
        }
        let mut pieces = Vec::new();
        for p in &s.parts {
            pieces.push(match p {
                StrPart::Text(t) => h::FPiece::Lit(t.clone()),
                StrPart::Interp { expr, spec } => h::FPiece::Expr(self.expr(expr)?, spec.clone()),
            });
        }
        Ok(mk(
            h::ExprKind::FStr {
                template: s.kind == StrKind::Template,
                pieces,
            },
            span,
        ))
    }

    pub fn expr(&mut self, e: &Expr) -> R<h::Expr> {
        self.expr_inner(e).map_err(|u| u.at(e.span))
    }

    fn expr_inner(&mut self, e: &Expr) -> R<h::Expr> {
        use h::ExprKind as K;
        let b = |x: h::Expr| Box::new(x);
        let span = e.span;
        let kind = match &e.kind {
            ExprKind::Int(s) => K::Lit(h::Lit::Int(s.clone())),
            ExprKind::Float(s) => K::Lit(h::Lit::Float(s.clone())),
            ExprKind::Str(s) => return self.string(s, span),
            ExprKind::Char(c) => {
                let mut cs = c.chars();
                match (cs.next(), cs.next()) {
                    (Some(c), None) => K::Lit(h::Lit::Char(c)),
                    _ => return unsupported("a char literal that is not one character"),
                }
            }
            ExprKind::Bool(v) => K::Lit(h::Lit::Bool(*v)),
            ExprKind::NoneLit | ExprKind::Unit => K::Lit(h::Lit::None),
            ExprKind::Name(n) | ExprKind::TypeName(n) => K::Name(n.clone()),
            ExprKind::SelfValue => K::Name("self".to_owned()),
            ExprKind::Hole => K::Hole,
            ExprKind::Paren(inner) => {
                let mut x = self.expr(inner)?;
                x.paren = true;
                return Ok(x);
            }
            ExprKind::Tuple(xs) => K::Tuple(self.exprs(xs)?),
            ExprKind::List(xs) => K::List(self.exprs(xs)?),
            ExprKind::Set(xs) => K::Set(self.exprs(xs)?),
            ExprKind::Map(kvs) => K::Dict(
                kvs.iter()
                    .map(|(k, v)| Ok((self.expr(k)?, self.expr(v)?)))
                    .collect::<R<_>>()?,
            ),
            ExprKind::Field(x, n) => {
                let at = Span {
                    line: span.end_line,
                    col: span.end_col.saturating_sub(n.len() as u32),
                    ..span
                };
                K::Attr(b(self.expr(x)?), n.clone(), at)
            }
            // `x.len()` is the builtin `len(x)`, so E504 and the bounds solver see it (15.3).
            ExprKind::Call(f, args)
                if args.is_empty() && matches!(&f.kind, ExprKind::Field(_, m) if m == "len") =>
            {
                let ExprKind::Field(x, _) = &f.kind else {
                    unreachable!("matched")
                };
                let arg = h::Arg {
                    name: None,
                    kind: h::ArgKind::Expr(self.expr(x)?),
                    span: x.span,
                };
                K::Call(b(mk(K::Name("len".into()), f.span)), vec![arg])
            }
            ExprKind::Call(f, args) => K::Call(b(self.expr(f)?), self.args(args)?),
            ExprKind::Index(x, ix) => match ix.as_slice() {
                [IxArg::Expr(i)] => K::Index(b(self.expr(x)?), b(self.expr(i)?)),
                [
                    IxArg::Slice {
                        lo,
                        hi,
                        inclusive: false,
                    },
                ] => K::Slice(
                    b(self.expr(x)?),
                    [
                        lo.as_ref()
                            .map(|e| self.expr(e).map(Box::new))
                            .transpose()?,
                        hi.as_ref()
                            .map(|e| self.expr(e).map(Box::new))
                            .transpose()?,
                        None,
                    ],
                    false,
                ),
                // `a[i, j]` on an `Array`: the index is a tuple of axes (11.2, RFC 0055).
                axes if axes.len() > 1 && axes.iter().all(|a| matches!(a, IxArg::Expr(_))) => {
                    let mut idx = Vec::new();
                    for a in axes {
                        if let IxArg::Expr(i) = a {
                            idx.push(self.expr(i)?);
                        }
                    }
                    let tuple = mk(K::Tuple(idx), x.span);
                    K::Index(b(self.expr(x)?), b(tuple))
                }
                _ => return unsupported("multi-axis, labelled and inclusive indexing"),
            },
            ExprKind::Question(x) => K::Try(b(self.expr(x)?)),
            ExprKind::Unary(op, x) => {
                let inner = self.expr(x)?;
                match op {
                    UnOp::Neg => K::Unary("-", b(inner)),
                    UnOp::Pos => K::Unary("+", b(inner)),
                    UnOp::BitNot => K::Unary("~", b(inner)),
                    UnOp::Not => K::Not(b(inner)),
                    UnOp::Copy | UnOp::Share => {
                        let m = if *op == UnOp::Copy { "copy" } else { "share" };
                        let callee = mk(K::Attr(b(inner), m.to_owned(), span), span);
                        K::Call(b(callee), Vec::new())
                    }
                    UnOp::Try => K::Reify(b(inner)),
                }
            }
            ExprKind::Binary(op, l, r) => {
                let (l, r) = (b(self.expr(l)?), b(self.expr(r)?));
                let text = binop_text(*op);
                if op.is_comparison() {
                    K::Compare(text, l, r)
                } else {
                    K::Binary(text, l, r)
                }
            }
            ExprKind::Is {
                value,
                negated,
                pattern,
            } if matches!(pattern.kind, PatKind::Lit(ref l) if matches!(l.kind, ExprKind::NoneLit)) =>
            {
                let none = mk(K::Lit(h::Lit::None), pattern.span);
                K::Compare(
                    if *negated { "is not" } else { "is" },
                    b(self.expr(value)?),
                    b(none),
                )
            }
            ExprKind::Is { .. } => {
                return unsupported("`is` with a pattern outside `if e is P`");
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => K::Cond {
                then: b(self.expr(then)?),
                cond: b(self.expr(cond)?),
                els: b(self.expr(otherwise)?),
            },
            ExprKind::Fallback(lhs, fb) => {
                let lhs = b(self.expr(lhs)?);
                let fb = match fb.as_ref() {
                    Fallback::Value(v) => h::Fallback::Value(self.expr(v)?),
                    Fallback::Bind(n, v) => h::Fallback::Bind(n.clone(), self.expr(v)?),
                    Fallback::Diverge(s) => match &s.kind {
                        StmtKind::Return(v) => {
                            h::Fallback::Return(v.as_ref().map(|v| self.expr(v)).transpose()?)
                        }
                        StmtKind::Fail(v) => h::Fallback::Fail(self.expr(v)?),
                        StmtKind::Break(None) => h::Fallback::Break,
                        StmtKind::Continue(None) => h::Fallback::Continue,
                        _ => return unsupported("this fallback"),
                    },
                    Fallback::Block(n, ss) => h::Fallback::Block(n.clone(), self.stmts(ss)?),
                };
                K::Fallback(lhs, Box::new(fb))
            }
            ExprKind::Lambda { params, body } => K::Lambda(params.clone(), b(self.expr(body)?)),
            ExprKind::Subject(_) => return self.subject(e),
            ExprKind::Cast(x, t) => K::Cast(b(self.expr(x)?), self.ty(t)?),
            ExprKind::UnitNum { num, unit } => {
                let n = num.replace('_', "");
                let lit = if n.contains(['.', 'e', 'E']) {
                    h::Lit::Float(n)
                } else {
                    h::Lit::Int(n)
                };
                K::Binary(
                    "*",
                    b(mk(h::ExprKind::Lit(lit), e.span)),
                    b(mk(h::ExprKind::Name(unit.clone()), e.span)),
                )
            }
            ExprKind::Temporal { .. } => return unsupported("date and time literals"),
            // A grid is its rows as nested lists; the expected `Array` type checks the shape (R463).
            ExprKind::Grid(rows) => K::List(
                rows.iter()
                    .map(|r| Ok(mk(K::List(self.exprs(r)?), e.span)))
                    .collect::<R<_>>()?,
            ),
            ExprKind::Stage(x) => K::Stage(b(self.expr(x)?)),
            ExprKind::Asm { .. } => return unsupported("`asm`"),
            ExprKind::TypeField(..) | ExprKind::Reflect(..) => {
                return unsupported("type fields and reflection");
            }
            ExprKind::Block(_) | ExprKind::ParFor { .. } => {
                return unsupported("block and `par for` values");
            }
        };
        Ok(mk(kind, span))
    }

    fn exprs(&mut self, xs: &[Expr]) -> R<Vec<h::Expr>> {
        xs.iter().map(|x| self.expr(x)).collect()
    }

    /// `.a.b(c)` in argument position: a closure over a fresh parameter (15.3). Only the
    /// closure reading is lowered; `ColExpr` subjects need the expected type.
    fn subject(&mut self, e: &Expr) -> R<h::Expr> {
        const IT: &str = "it";
        fn reroot(e: &Expr) -> Expr {
            let kind = match &e.kind {
                ExprKind::Subject(n) => ExprKind::Field(
                    Box::new(Expr {
                        kind: ExprKind::Name(IT.to_owned()),
                        span: e.span,
                    }),
                    n.clone(),
                ),
                ExprKind::Field(x, n) => ExprKind::Field(Box::new(reroot(x)), n.clone()),
                ExprKind::Call(f, a) => ExprKind::Call(Box::new(reroot(f)), a.clone()),
                ExprKind::Index(x, i) => ExprKind::Index(Box::new(reroot(x)), i.clone()),
                ExprKind::Question(x) => ExprKind::Question(Box::new(reroot(x))),
                k => k.clone(),
            };
            Expr { kind, span: e.span }
        }
        let body = self.expr(&reroot(e))?;
        Ok(mk(
            h::ExprKind::Lambda(vec![IT.to_owned()], Box::new(body)),
            e.span,
        ))
    }
}

fn is_none(p: &Pattern) -> bool {
    matches!(&p.kind, PatKind::Lit(l) if matches!(l.kind, ExprKind::NoneLit))
}

fn bind_pat(n: Name) -> h::Pat {
    h::Pat::Path {
        path: vec![n],
        args: None,
    }
}

/// The present conditions joined by `and`.
fn and_all<const N: usize>(cs: [Option<h::Expr>; N], at: Span) -> h::Expr {
    cs.into_iter()
        .flatten()
        .reduce(|a, b| mk(h::ExprKind::Binary("and", Box::new(a), Box::new(b)), at))
        .expect("at least one condition")
}

/// Whether a loop body has a `continue` for its own loop.
fn continues(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match &s.kind {
        StmtKind::Continue(_) => true,
        StmtKind::If {
            branches,
            otherwise,
        } => {
            branches.iter().any(|(_, b)| continues(b))
                || otherwise.as_deref().is_some_and(continues)
        }
        StmtKind::Match { arms, .. } => arms.iter().any(|a| match &a.body {
            ArmBody::Block(b) => continues(b),
            ArmBody::Expr(_) => false,
        }),
        StmtKind::With { body, .. } | StmtKind::Unsafe(body) => continues(body),
        _ => false,
    })
}

/// Rewrites a body whose last statement is a value into one that returns it.
fn tail_return(stmts: &mut [h::Stmt]) {
    let Some(last) = stmts.last_mut() else {
        return;
    };
    match &mut last.kind {
        h::StmtKind::Expr(e) => {
            let e = e.clone();
            last.kind = h::StmtKind::Return(Some(e));
        }
        h::StmtKind::If { arms, els } => {
            arms.iter_mut().for_each(|(_, b)| tail_return(&mut b.stmts));
            if let Some(b) = els {
                tail_return(&mut b.stmts);
            }
        }
        h::StmtKind::Match { cases, .. } => {
            cases
                .iter_mut()
                .for_each(|c| tail_return(&mut c.body.stmts));
        }
        _ => {}
    }
}

/// Yan's built-in type `n` with `k` type arguments, as Yan Agent spells it (16.3).
fn yan_type(n: &str, k: usize) -> Option<&'static str> {
    yan_names::type_row(n, yan_names::Surface::Yn)
        .filter(|r| r.arity == k)
        .map(|r| r.yir)
}

fn binop_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Or => "or",
        BinOp::And => "and",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::In => "in",
        BinOp::NotIn => "not in",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::FloorDiv => "//",
        BinOp::Mod => "%",
        BinOp::MatMul => "@",
        BinOp::Pow => "**",
    }
}

// ---- capability inference for non-`pub` functions (15.3)

fn infer_caps(m: &Module) -> HashMap<String, Vec<String>> {
    let fns: HashMap<&str, &FnDef> = m
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Fn(f) => Some((f.sig.name.as_str(), f)),
            _ => None,
        })
        .collect();
    let mut caps: HashMap<String, HashSet<String>> = HashMap::new();
    let mut calls: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, f) in &fns {
        let mut names = Names::new();
        let mut bound = HashSet::new();
        for p in &f.sig.params {
            if let Param::Value { name, .. } = p {
                bound.insert(name.clone());
            }
        }
        body_names(&f.body, &mut names, &mut bound);
        let declared: HashSet<String> = match &f.sig.uses {
            Some(cs) => cs
                .iter()
                .filter_map(|c| match c {
                    CapItem::Named { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect(),
            None => names
                .keys()
                .filter(|n| CAPS.contains(&n.as_str()) && !bound.contains(*n))
                .cloned()
                .collect(),
        };
        caps.insert((*name).to_owned(), declared);
        let callees = names
            .keys()
            .filter(|n| fns.contains_key(n.as_str()) && !bound.contains(*n) && n.as_str() != *name)
            .cloned()
            .collect();
        calls.insert((*name).to_owned(), callees);
    }
    loop {
        let mut changed = false;
        for (name, f) in &fns {
            if f.sig.uses.is_some() {
                continue;
            }
            let extra: HashSet<String> = calls[*name]
                .iter()
                .flat_map(|c| caps[c].iter().cloned())
                .collect();
            let own = caps.get_mut(*name).expect("every fn has an entry");
            for c in extra {
                changed |= own.insert(c);
            }
        }
        if !changed {
            break;
        }
    }
    caps.into_iter()
        .map(|(k, v)| {
            let mut v: Vec<_> = v.into_iter().collect();
            v.sort();
            (k, v)
        })
        .collect()
}

/// Each free name a body reads, at its first use.
#[derive(Default)]
struct Names {
    seen: HashMap<String, Span>,
    /// Failure types named by `fail` statements.
    fails: HashSet<String>,
    /// Functions called under `?`.
    tried: HashSet<String>,
}

impl Names {
    fn new() -> Self {
        Self::default()
    }
}

impl std::ops::Deref for Names {
    type Target = HashMap<String, Span>;
    fn deref(&self) -> &Self::Target {
        &self.seen
    }
}

impl std::ops::DerefMut for Names {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.seen
    }
}

/// The type a `fail` expression constructs, when it is a constructor call.
fn failed_type(e: &Expr) -> Option<String> {
    match &e.kind {
        ExprKind::Paren(x) => failed_type(x),
        ExprKind::TypeName(n) => Some(n.clone()),
        ExprKind::TypeField(x, _) => failed_type(x),
        ExprKind::Call(f, _) => failed_type(f),
        _ => None,
    }
}

fn type_names(t: &Type) -> Vec<String> {
    match &t.kind {
        TypeKind::Path { path, args } if args.is_empty() && path.len() == 1 => path.clone(),
        TypeKind::Union(ts) => ts.iter().flat_map(type_names).collect(),
        _ => Vec::new(),
    }
}

/// Failure types of non-`pub` functions that omit `fail`, per strongly connected component (15.3):
/// what their `fail` statements construct plus what the local functions they call under `?` fail with.
fn infer_fails(m: &Module) -> HashMap<String, Vec<String>> {
    let fns: HashMap<&str, (&FnDef, bool)> = m
        .items
        .iter()
        .filter_map(|i| match &i.kind {
            ItemKind::Fn(f) => Some((f.sig.name.as_str(), (f, i.public))),
            _ => None,
        })
        .collect();
    let mut fails: HashMap<String, HashSet<String>> = HashMap::new();
    let mut tried: HashMap<String, HashSet<String>> = HashMap::new();
    for (name, (f, public)) in &fns {
        if let Some(t) = &f.sig.fail {
            fails.insert((*name).to_owned(), type_names(t).into_iter().collect());
            continue;
        }
        if *public {
            fails.insert((*name).to_owned(), HashSet::new());
            continue;
        }
        let mut names = Names::new();
        let mut bound = HashSet::new();
        for p in &f.sig.params {
            if let Param::Value { name, .. } = p {
                bound.insert(name.clone());
            }
        }
        body_names(&f.body, &mut names, &mut bound);
        fails.insert((*name).to_owned(), names.fails);
        tried.insert(
            (*name).to_owned(),
            names
                .tried
                .into_iter()
                .filter(|c| fns.contains_key(c.as_str()))
                .collect(),
        );
    }
    loop {
        let mut changed = false;
        for (name, callees) in &tried {
            let extra: HashSet<String> = callees
                .iter()
                .flat_map(|c| fails[c].iter().cloned())
                .collect();
            let own = fails.get_mut(name).expect("every fn has an entry");
            for c in extra {
                changed |= own.insert(c);
            }
        }
        if !changed {
            break;
        }
    }
    fails
        .into_iter()
        .map(|(k, v)| {
            let mut v: Vec<_> = v.into_iter().collect();
            v.sort();
            (k, v)
        })
        .collect()
}

fn body_names(b: &Body, names: &mut Names, bound: &mut HashSet<String>) {
    match b {
        Body::Expr(e) => expr_names(e, names, bound),
        Body::Block { contracts, stmts } => {
            contracts
                .iter()
                .for_each(|c| expr_names(&c.expr, names, bound));
            stmts.iter().for_each(|s| stmt_names(s, names, bound));
        }
        Body::Clauses { contracts, arms } => {
            contracts
                .iter()
                .for_each(|c| expr_names(&c.expr, names, bound));
            arms.iter().for_each(|a| arm_names(a, names, bound));
        }
    }
}

fn pat_binds(p: &Pattern, bound: &mut HashSet<String>) {
    match &p.kind {
        PatKind::Bind(n) => {
            bound.insert(n.clone());
        }
        PatKind::Ctor {
            args: Some(args), ..
        } => {
            for a in args {
                match a {
                    PPat::Pos(p) | PPat::Named(_, Some(p)) => pat_binds(p, bound),
                    PPat::Named(n, None) => {
                        bound.insert(n.clone());
                    }
                    PPat::Rest => {}
                }
            }
        }
        PatKind::Tuple(ps) | PatKind::Or(ps) => ps.iter().for_each(|p| pat_binds(p, bound)),
        PatKind::Slice(ps) => {
            for s in ps {
                match s {
                    SPat::Pat(p) => pat_binds(p, bound),
                    SPat::Rest(Some(n)) => {
                        bound.insert(n.clone());
                    }
                    SPat::Rest(None) => {}
                }
            }
        }
        PatKind::As(p, n) => {
            pat_binds(p, bound);
            bound.insert(n.clone());
        }
        _ => {}
    }
}

fn arm_names(a: &Arm, names: &mut Names, bound: &mut HashSet<String>) {
    pat_binds(&a.pattern, bound);
    if let Some(g) = &a.guard {
        expr_names(g, names, bound);
    }
    arm_body_names(&a.body, names, bound);
}

fn arm_body_names(b: &ArmBody, names: &mut Names, bound: &mut HashSet<String>) {
    match b {
        ArmBody::Expr(e) => expr_names(e, names, bound),
        ArmBody::Block(ss) => ss.iter().for_each(|s| stmt_names(s, names, bound)),
    }
}

fn stmt_names(s: &Stmt, names: &mut Names, bound: &mut HashSet<String>) {
    let e = |x: &Expr, names: &mut Names, bound: &mut HashSet<String>| expr_names(x, names, bound);
    let block = |ss: &[Stmt], names: &mut Names, bound: &mut HashSet<String>| {
        ss.iter().for_each(|s| stmt_names(s, names, bound))
    };
    match &s.kind {
        StmtKind::Fail(x) => {
            names.fails.extend(failed_type(x));
            e(x, names, bound)
        }
        StmtKind::Expr(x)
        | StmtKind::Yield(x)
        | StmtKind::Assert(x)
        | StmtKind::Become(x)
        | StmtKind::Return(Some(x)) => e(x, names, bound),
        StmtKind::Assign { place, value, .. } => {
            e(place, names, bound);
            e(value, names, bound);
        }
        StmtKind::Let { pattern, value, .. } => {
            pat_binds(pattern, bound);
            e(value, names, bound);
        }
        StmtKind::If {
            branches,
            otherwise,
        } => {
            for (c, b) in branches {
                e(c, names, bound);
                block(b, names, bound);
            }
            if let Some(b) = otherwise {
                block(b, names, bound);
            }
        }
        StmtKind::Match { scrutinee, arms } => {
            e(scrutinee, names, bound);
            arms.iter().for_each(|a| arm_names(a, names, bound));
        }
        StmtKind::For {
            pattern,
            iter,
            invariants,
            body,
            ..
        } => {
            pat_binds(pattern, bound);
            e(iter, names, bound);
            invariants
                .iter()
                .for_each(|c| expr_names(&c.expr, names, bound));
            block(body, names, bound);
        }
        StmtKind::While {
            cond,
            invariants,
            body,
            ..
        } => {
            e(cond, names, bound);
            invariants
                .iter()
                .for_each(|c| expr_names(&c.expr, names, bound));
            block(body, names, bound);
        }
        StmtKind::Loop { body, .. } | StmtKind::Par { body, .. } | StmtKind::Unsafe(body) => {
            block(body, names, bound)
        }
        StmtKind::With { kind, body } => {
            match kind {
                WithKind::Bind { name, value, .. } => {
                    e(value, names, bound);
                    bound.insert(name.clone());
                }
                WithKind::Deadline(x) | WithKind::Timeout(x) => {
                    // A cancel scope can expire (R472).
                    names.fails.insert("Timeout".into());
                    e(x, names, bound)
                }
                WithKind::Supervisor(x) => e(x, names, bound),
            }
            block(body, names, bound);
        }
        StmtKind::Select(arms) => {
            for a in arms {
                e(&a.op, names, bound);
                if let Some(p) = &a.pattern {
                    pat_binds(p, bound);
                }
                arm_body_names(&a.body, names, bound);
            }
        }
        StmtKind::StageFor {
            pattern,
            iter,
            body,
        } => {
            pat_binds(pattern, bound);
            e(iter, names, bound);
            block(body, names, bound);
        }
        StmtKind::StageIf {
            cond,
            then,
            otherwise,
        } => {
            e(cond, names, bound);
            block(then, names, bound);
            if let Some(b) = otherwise {
                block(b, names, bound);
            }
        }
        StmtKind::Return(None)
        | StmtKind::Reveal(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Unreachable => {}
    }
}

fn expr_names(e: &Expr, names: &mut Names, bound: &mut HashSet<String>) {
    let mut go = |x: &Expr| expr_names(x, names, bound);
    match &e.kind {
        ExprKind::Name(n) => {
            names.entry(n.clone()).or_insert(e.span);
        }
        ExprKind::Str(s) => {
            for p in &s.parts {
                if let StrPart::Interp { expr, .. } = p {
                    go(expr);
                }
            }
        }
        ExprKind::Paren(x)
        | ExprKind::Stage(x)
        | ExprKind::Field(x, _)
        | ExprKind::TypeField(x, _)
        | ExprKind::Reflect(x, _)
        | ExprKind::Cast(x, _)
        | ExprKind::Unary(_, x) => go(x),
        ExprKind::Question(x) => {
            if let ExprKind::Call(f, _) = &x.kind
                && let ExprKind::Name(n) = &f.kind
            {
                names.tried.insert(n.clone());
            }
            expr_names(x, names, bound)
        }
        ExprKind::Tuple(xs) | ExprKind::List(xs) | ExprKind::Set(xs) => xs.iter().for_each(go),
        ExprKind::Grid(rows) => rows.iter().flatten().for_each(go),
        ExprKind::Map(kvs) => kvs.iter().for_each(|(k, v)| {
            go(k);
            go(v);
        }),
        ExprKind::Call(f, args) => {
            expr_names(f, names, bound);
            for a in args {
                match &a.kind {
                    ArgKind::Value(x) | ArgKind::Ref(x) | ArgKind::Become(x) => {
                        expr_names(x, names, bound)
                    }
                    ArgKind::Pun => {
                        if let Some(l) = &a.label {
                            names.entry(l.clone()).or_insert(a.span);
                        }
                    }
                }
            }
        }
        ExprKind::Index(x, ix) => {
            go(x);
            fn ix_names(i: &IxArg, names: &mut Names, bound: &mut HashSet<String>) {
                match i {
                    IxArg::Expr(x) => expr_names(x, names, bound),
                    IxArg::Slice { lo, hi, .. } => lo
                        .iter()
                        .chain(hi)
                        .for_each(|x| expr_names(x, names, bound)),
                    IxArg::Labeled(_, i) => ix_names(i, names, bound),
                }
            }
            ix.iter().for_each(|i| ix_names(i, names, bound));
        }
        ExprKind::Binary(_, l, r) => {
            go(l);
            go(r);
        }
        ExprKind::Is { value, pattern, .. } => {
            go(value);
            pat_binds(pattern, bound);
        }
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            go(cond);
            go(then);
            go(otherwise);
        }
        ExprKind::Fallback(lhs, fb) => {
            go(lhs);
            match fb.as_ref() {
                Fallback::Value(v) => go(v),
                Fallback::Bind(n, v) => {
                    bound.insert(n.clone());
                    expr_names(v, names, bound);
                }
                Fallback::Diverge(s) => stmt_names(s, names, bound),
                Fallback::Block(n, ss) => {
                    if let Some(n) = n {
                        bound.insert(n.clone());
                    }
                    ss.iter().for_each(|s| stmt_names(s, names, bound));
                }
            }
        }
        ExprKind::Lambda { params, body } => {
            bound.extend(params.iter().cloned());
            expr_names(body, names, bound);
        }
        ExprKind::Block(ss) => ss.iter().for_each(|s| stmt_names(s, names, bound)),
        ExprKind::ParFor {
            limit,
            pattern,
            iter,
            body,
            ..
        } => {
            if let Some(l) = limit {
                expr_names(l, names, bound);
            }
            pat_binds(pattern, bound);
            expr_names(iter, names, bound);
            body.iter().for_each(|s| stmt_names(s, names, bound));
        }
        ExprKind::Asm { args, .. } => {
            for a in args {
                if let AsmArg::Operand { value, .. } = a {
                    expr_names(value, names, bound);
                }
            }
        }
        _ => {}
    }
}
