//! Raise (16.4, R559, R565): checked Yan Core back to HIR, written once for every surface. It folds
//! the temporaries elaboration introduced back into expressions, turns intrinsic and std calls
//! back into operators, methods and builtins, drops capability arguments, and recognises the
//! loops and fallbacks elaboration expands.
//!
//! Yan carries no origin marks yet, so raise works as 16.5 says it does for unmarked Yan Core: a
//! local is inserted when its name is one elaboration generates (`_t1`, `_i3`), and a form is
//! recognised only in the shape elaboration emits.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use yan_core::ast as y;
use yan_diag::Span;
use yan_surface::{Mark, Trivia, TriviaTable};

use crate::*;

const AT: Span = Span {
    line: 0,
    col: 0,
    end_line: 0,
    end_col: 0,
};

/// A Yan Core form raise does not handle yet.
#[derive(Debug, PartialEq)]
pub struct Unraised(pub String);

type R<T> = Result<T, Unraised>;

fn no<T>(what: impl Into<String>) -> R<T> {
    Err(Unraised(what.into()))
}

/// What raise needs to know about std, which only the elaborator has loaded.
pub trait Std {
    /// Whether elaboration copied a std item of this path (a record, a sum or a function body)
    /// into the module.
    fn inserted(&self, name: &str) -> bool;
    /// The std module below `std.` that declares type `name`.
    fn type_module(&self, name: &str) -> Option<String>;
    /// A std function's parameters by path, receiver first.
    fn params(&self, path: &str) -> Option<Vec<StdParam>>;
}

pub struct StdParam {
    pub name: Name,
    pub default: Option<Expr>,
    /// Before a `/`: never passed by keyword.
    pub positional: bool,
    /// Its type is `iter[T]`.
    pub iter: bool,
}

/// A mark `raise` used: its item key (the identity hash) and node path.
pub type Consumed = (String, Vec<u32>);

/// What [`raise`] produced and which marks of the trivia table it used.
pub struct Raised {
    pub module: Module,
    pub consumed: Vec<Consumed>,
}

/// `m` as HIR, reading spelling marks from `trivia` (RFC 0034). Items elaboration copied in from
/// std are left out.
pub fn raise(m: &y::Module, trivia: &TriviaTable, std: &dyn Std) -> R<Raised> {
    let fns = signatures(m);
    let mut items = Vec::new();
    let mut consumed = Vec::new();
    for (it, h) in m.items.iter().zip(yan_core::hash::hash_module(m)) {
        let marks = Marks::new(trivia, h.identity.to_string(), it, &fns);
        if let Some(i) = item(it, std, &marks)? {
            items.push(i);
        }
        consumed.extend(marks.finish());
    }
    Ok(Raised {
        module: Module {
            header: Vec::new(),
            attrs: Vec::new(),
            doc: None,
            items,
        },
        consumed,
    })
}

/// The value parameter names of each function of `m`, by call path.
fn signatures(m: &y::Module) -> HashMap<String, Vec<String>> {
    m.items
        .iter()
        .filter_map(|it| match &it.kind {
            y::ItemKind::Fn(f) => Some((
                f.path.join("."),
                f.params
                    .iter()
                    .filter_map(|p| match p {
                        y::Param::Value { name, .. } => Some(name.clone()),
                        y::Param::Cap { .. } => None,
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

/// An argument's `Keyword` or `Positional` mark and its path.
type ArgMark = Option<(Mark, Vec<u32>)>;

/// The marks of one item (RFC 0034 D3, D5): where its calls are, and which marks `raise` has used.
struct Marks<'a> {
    trivia: &'a TriviaTable,
    key: String,
    /// Whether the item has a name-table entry, which makes its marks authoritative (D3).
    sourced: bool,
    paths: HashMap<*const y::Call, Vec<u32>>,
    fields: HashMap<(*const y::Ctor, usize), Vec<u32>>,
    stmts: HashMap<*const y::Stmt, Vec<u32>>,
    fns: &'a HashMap<String, Vec<String>>,
    seen: RefCell<HashSet<Vec<u32>>>,
    rejected: RefCell<HashSet<Vec<u32>>>,
}

impl<'a> Marks<'a> {
    fn new(
        trivia: &'a TriviaTable,
        key: String,
        it: &y::Item,
        fns: &'a HashMap<String, Vec<String>>,
    ) -> Marks<'a> {
        let mut paths = HashMap::new();
        let mut fields = HashMap::new();
        let mut stmts = HashMap::new();
        yan_core::path::walk(it, &mut |p, n| match n {
            yan_core::path::Node::Stmt(s) => {
                stmts.insert(s as *const y::Stmt, p.to_vec());
            }
            yan_core::path::Node::Call(c) => {
                paths.insert(c as *const y::Call, p.to_vec());
            }
            yan_core::path::Node::Field(c, k) => {
                fields.insert((c as *const y::Ctor, k), p.to_vec());
            }
            _ => {}
        });
        Marks {
            sourced: trivia.has_names(&key),
            trivia,
            key,
            paths,
            fields,
            stmts,
            fns,
            seen: RefCell::default(),
            rejected: RefCell::default(),
        }
    }

    /// The mark of each value argument of `c`, in order.
    fn args(&self, c: &y::Call) -> Vec<ArgMark> {
        let at = self.paths.get(&(c as *const y::Call));
        let mut out = Vec::new();
        for (k, a) in c.args.iter().enumerate() {
            if !matches!(a, y::Arg::Value(..)) {
                continue;
            }
            out.push(at.filter(|_| self.sourced).and_then(|p| {
                let mut p = p.clone();
                p.push(k as u32);
                let m = self.trivia.at(&self.key, &p).iter().find_map(|t| match t {
                    Trivia::Mark(m @ (Mark::Keyword | Mark::Positional)) => Some(m.clone()),
                    _ => None,
                })?;
                Some((m, p))
            }));
        }
        out
    }

    /// How the source spelled call `c`: its `Method` or `Function` mark, consumed. `None` when
    /// the item has no marks to go by (D3) or the call has none.
    fn spelling(&self, c: &y::Call) -> Option<Mark> {
        let p = self
            .paths
            .get(&(c as *const y::Call))
            .filter(|_| self.sourced)?;
        let m = self.trivia.at(&self.key, p).iter().find_map(|t| match t {
            Trivia::Mark(m @ (Mark::Method | Mark::Function)) => Some(m.clone()),
            _ => None,
        })?;
        self.seen.borrow_mut().insert(p.clone());
        Some(m)
    }

    /// The mark of field `k` of constructor `c`.
    fn field(&self, c: &y::Ctor, k: usize) -> ArgMark {
        let p = self
            .fields
            .get(&(c as *const y::Ctor, k))
            .filter(|_| self.sourced)?;
        let m = self.trivia.at(&self.key, p).iter().find_map(|t| match t {
            Trivia::Mark(m @ (Mark::Keyword | Mark::Positional)) => Some(m.clone()),
            _ => None,
        })?;
        Some((m, p.clone()))
    }

    /// Whether the source wrote a type on the `let` `s`: `None` when the item has no marks to
    /// go by (D3), else the `Annotated` mark's presence. A `let` is consumed either way.
    fn annotated(&self, s: &y::Stmt) -> Option<bool> {
        let p = self
            .stmts
            .get(&(s as *const y::Stmt))
            .filter(|_| self.sourced)?;
        let here = self
            .trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Mark(Mark::Annotated)));
        if here {
            self.seen.borrow_mut().insert(p.clone());
        }
        Some(here)
    }

    /// Whether `s` carries `FromPre`, consumed. False when the item has no marks to go by (D3).
    fn marked_pre(&self, s: &y::Stmt) -> bool {
        let Some(p) = self
            .stmts
            .get(&(s as *const y::Stmt))
            .filter(|_| self.sourced)
        else {
            return false;
        };
        let here = self
            .trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Mark(Mark::FromPre)));
        if here {
            self.seen.borrow_mut().insert(p.clone());
        }
        here
    }

    /// An `Annotated` mark that fails its check: the default prints.
    fn reject_stmt(&self, s: &y::Stmt) {
        if let Some(p) = self.stmts.get(&(s as *const y::Stmt)) {
            self.rejected.borrow_mut().insert(p.clone());
        }
    }

    fn consume(&self, marks: &[ArgMark]) {
        self.seen
            .borrow_mut()
            .extend(marks.iter().flatten().map(|(_, p)| p.clone()));
    }

    /// A mark that fails its check: the default prints, and strict mode reports the mark.
    fn reject(&self, m: &ArgMark) {
        if let Some((_, p)) = m {
            self.rejected.borrow_mut().insert(p.clone());
        }
    }

    fn finish(self) -> Vec<Consumed> {
        let rejected = self.rejected.into_inner();
        self.seen
            .into_inner()
            .into_iter()
            .filter(|p| !rejected.contains(p))
            .map(|p| (self.key.clone(), p))
            .collect()
    }
}

/// The marks of `trivia` that `raised` did not consume, as `(item, path, mark)` lines.
pub fn unconsumed(trivia: &TriviaTable, raised: &Raised) -> Vec<String> {
    trivia
        .entries()
        .filter_map(|(i, p, t)| match t {
            Trivia::Mark(mk) if !raised.consumed.iter().any(|(ci, cp)| ci == i && cp == p) => {
                Some(format!("{mk:?} at {i} {p:?}"))
            }
            _ => None,
        })
        .collect()
}

fn e(kind: ExprKind) -> Expr {
    Expr {
        kind,
        span: AT,
        paren: false,
    }
}

fn name(n: &str) -> Expr {
    e(ExprKind::Name(n.to_owned()))
}

fn stmt(kind: StmtKind) -> Stmt {
    Stmt { kind, span: AT }
}

fn last(p: &[y::Name]) -> String {
    p.last().cloned().unwrap_or_default()
}

fn item(it: &y::Item, std: &dyn Std, marks: &Marks) -> R<Option<Item>> {
    let (is_pub, kind) = match &it.kind {
        y::ItemKind::Fn(f) if std.inserted(&f.path.join(".")) => return Ok(None),
        y::ItemKind::Fn(f) => (f.is_pub, ItemKind::Func(func(f, std, marks)?)),
        y::ItemKind::Record(r) if std.inserted(&last(&r.path)) => return Ok(None),
        y::ItemKind::Sum(s) if std.inserted(&last(&s.path)) => return Ok(None),
        y::ItemKind::Record(r) => (
            r.is_pub,
            ItemKind::Record(Record {
                name: last(&r.path),
                tparams: tparams(&r.tparams),
                doc: None,
                fields: r.fields.iter().map(field).collect::<R<_>>()?,
            }),
        ),
        y::ItemKind::Sum(s) => (
            s.is_pub,
            ItemKind::Type(TypeDef {
                name: last(&s.path),
                tparams: tparams(&s.tparams),
                rhs: TypeRhs::Sum(
                    s.variants
                        .iter()
                        .map(|v| {
                            Ok(Variant {
                                name: v.name.clone(),
                                fields: v.fields.iter().flatten().map(field).collect::<R<_>>()?,
                            })
                        })
                        .collect::<R<_>>()?,
                ),
                where_: None,
            }),
        ),
        y::ItemKind::Gen(_) => return no("generators"),
        y::ItemKind::Newtype(_) => return no("newtypes"),
        y::ItemKind::Trait(_) => return no("traits"),
        y::ItemKind::Impl(_) => return no("impls"),
        y::ItemKind::Const(_) => return no("constants"),
        y::ItemKind::Test(_) => return no("tests"),
        y::ItemKind::Extern(_) => return no("extern blocks"),
        y::ItemKind::Inst(_) => return no("instances, which only optimized Yan Core has"),
    };
    let decorators = it
        .attrs
        .iter()
        .filter(|a| a.name != "doc")
        .map(|a| match a.args {
            None => Ok(Decorator {
                path: vec![a.name.clone()],
                args: None,
            }),
            Some(_) => no("attributes with arguments"),
        })
        .collect::<R<_>>()?;
    Ok(Some(Item {
        decorators,
        is_pub,
        kind,
        span: it.span,
        name_span: it.span,
    }))
}

fn field(f: &y::Field) -> R<Field> {
    if f.default.is_some() {
        return no("field defaults");
    }
    Ok(Field {
        name: f.name.clone(),
        ty: ty(&f.ty)?,
        default: None,
    })
}

fn tparams(ps: &[y::TParam]) -> Vec<TParam> {
    ps.iter()
        .map(|p| TParam::Type {
            name: p.name.clone(),
            bounds: p
                .bounds
                .iter()
                .map(|b| Bound::Path {
                    path: b.clone(),
                    args: Vec::new(),
                })
                .collect(),
        })
        .collect()
}

fn ty(t: &y::Type) -> R<Type> {
    Ok(match t {
        y::Type::Named(n) => match (n.path.as_slice(), n.args.as_slice()) {
            ([u], []) if u == "Unit" => Type::None,
            (path, args) => Type::Named {
                path: path.to_vec(),
                args: args
                    .iter()
                    .map(|a| {
                        Ok(match a {
                            y::TArg::Type(t) => TArg::Type(ty(t)?),
                            y::TArg::Nat(n) => TArg::Int(n.clone()),
                        })
                    })
                    .collect::<R<_>>()?,
            },
        },
        y::Type::Tuple(ts) => Type::Named {
            path: vec!["tuple".into()],
            args: ts
                .iter()
                .map(|t| Ok(TArg::Type(ty(t)?)))
                .collect::<R<_>>()?,
        },
        y::Type::Fn(f) => Type::Callable {
            par: false,
            params: f.params.iter().map(|(_, t)| ty(t)).collect::<R<_>>()?,
            ret: Box::new(ty(&f.ret)?),
            fail: Vec::new(),
            uses: match &f.uses {
                Some(y::CapSet::Names(ns)) => ns.clone(),
                _ => Vec::new(),
            },
        },
        y::Type::Dyn(_) => return no("`dyn` types"),
        y::Type::Refined { .. } => return no("refined types"),
        y::Type::Union(_) => return no("error unions"),
    })
}

fn fail_paths(t: &y::Type) -> R<Vec<Path>> {
    match t {
        y::Type::Named(n) if n.args.is_empty() => Ok(vec![n.path.clone()]),
        y::Type::Union(ts) => ts
            .iter()
            .map(fail_paths)
            .map(|p| Ok(p?.remove(0)))
            .collect(),
        _ => no("this failure type"),
    }
}

fn func(f: &y::FnDef, std: &dyn Std, marks: &Marks) -> R<FuncDef> {
    let mut params = Vec::new();
    let mut caps = Vec::new();
    let mut locals = HashMap::new();
    for p in &f.params {
        match p {
            y::Param::Value {
                comptime: false,
                conv,
                name,
                ty: t,
            } => {
                locals.insert(name.clone(), t.clone());
                params.push(Param::Value {
                    conv: match conv {
                        y::Conv::Let => None,
                        y::Conv::Inout => Some(Conv::Var),
                        y::Conv::Sink => Some(Conv::Take),
                    },
                    name: name.clone(),
                    span: AT,
                    ty: ty(t)?,
                    default: None,
                })
            }
            y::Param::Value { .. } => return no("comptime parameters"),
            y::Param::Cap { name, .. } => caps.push(CapRef {
                path: vec![name.clone()],
                alias: None,
            }),
        }
    }
    let unit = matches!(&f.ret, y::Type::Named(n) if n.path == ["Unit"]);
    let fail = f
        .fail
        .as_ref()
        .map(fail_paths)
        .transpose()?
        .unwrap_or_default();
    let ret = (!unit || !fail.is_empty()).then(|| -> R<RetType> {
        Ok(RetType {
            ty: if unit { None } else { Some(ty(&f.ret)?) },
            fail,
            from: f.from.clone(),
        })
    });
    let mut contracts = Vec::new();
    for c in &f.contracts {
        let mut b = Fold::new(std, marks, locals.clone(), &[]);
        contracts.push(match c {
            y::Contract::Requires(x) => Contract::Pre(b.pure(x)?),
            y::Contract::Ensures(x) => Contract::Post(b.pure(x)?),
            y::Contract::Example(x) => Contract::Ex {
                expr: b.pure(x)?,
                with: Vec::new(),
            },
            y::Contract::Decreases(_) => return no("`decreases`"),
        });
    }
    let body = match &f.body {
        None => None,
        Some(blk) => {
            let mut b = Fold::new(std, marks, locals, &blk.stmts);
            let mut stmts = b.stmts(&blk.stmts)?;
            contracts.extend(b.pre.drain(..).map(Contract::Pre));
            if let Some(t) = b.pending.keys().find(|t| !b.pure.contains(*t)) {
                return no(format!("a temporary used out of order: `{t}`"));
            }
            let r = if unit { None } else { Some(ty(&f.ret)?) };
            if !marks.sourced {
                drop_inferred(&mut stmts, r.as_ref(), &b.types);
            }
            if unit && matches!(stmts.last().map(|s| &s.kind), Some(StmtKind::Return(None))) {
                stmts.pop();
            }
            if stmts.is_empty() {
                stmts.push(stmt(StmtKind::Pass));
            }
            Some(Body {
                doc: None,
                contracts,
                stmts,
            })
        }
    };
    Ok(FuncDef {
        name: last(&f.path),
        name_span: f.name_span,
        tparams: tparams(&f.tparams),
        params,
        ret: ret.transpose()?,
        uses: (!caps.is_empty()).then_some(Uses::Caps(caps)),
        body,
    })
}

/// A nullary variant's name, which Yan Core spells without parentheses: `CamelCase`, not `None`.
fn variant(n: &str) -> bool {
    n != "None"
        && n.starts_with(|c: char| c.is_ascii_uppercase())
        && n.contains(|c: char| c.is_ascii_lowercase())
}

/// A name elaboration generates: `_`, a letter or two, digits.
fn generated(n: &str) -> bool {
    let Some(rest) = n.strip_prefix('_') else {
        return false;
    };
    let digits = rest.trim_start_matches(|c: char| c.is_ascii_lowercase());
    digits.len() < rest.len() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// A temporary waiting to be folded into the expression that uses it.
enum Pending {
    Expr(Expr),
    List(Vec<Expr>),
    Set(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
}

struct Fold<'a> {
    std: &'a dyn Std,
    marks: &'a Marks<'a>,
    types: HashMap<String, y::Type>,
    uses: HashMap<String, usize>,
    pending: HashMap<String, Pending>,
    /// Pending temporaries bound by a std call without effects, which may go unused.
    pure: std::collections::HashSet<String>,
    /// Labels of the enclosing loops, innermost last.
    loops: Vec<String>,
    /// Leading `assert`s elaborated from `pre`, moved back to the contract list.
    pre: Vec<Expr>,
}

const BUILDERS: [&str; 3] = ["std.list.append", "std.set.add", "std.dict.insert"];

impl<'a> Fold<'a> {
    fn new(
        std: &'a dyn Std,
        marks: &'a Marks<'a>,
        types: HashMap<String, y::Type>,
        stmts: &[y::Stmt],
    ) -> Fold<'a> {
        let mut uses = HashMap::new();
        count_stmts(stmts, &mut uses);
        Fold {
            std,
            marks,
            types,
            uses,
            pending: HashMap::new(),
            pure: Default::default(),
            loops: Vec::new(),
            pre: Vec::new(),
        }
    }

    fn uses(&self, n: &str) -> usize {
        self.uses.get(n).copied().unwrap_or(0)
    }

    fn atom(&mut self, a: &y::Atom) -> R<Expr> {
        Ok(match a {
            y::Atom::Name(n, _) => match self.pending.remove(n) {
                Some(Pending::Expr(x)) => x,
                Some(Pending::List(xs)) => e(ExprKind::List(xs)),
                Some(Pending::Set(xs)) if xs.is_empty() => call(name("set"), Vec::new()),
                Some(Pending::Set(xs)) => e(ExprKind::Set(xs)),
                Some(Pending::Dict(kvs)) => e(ExprKind::Dict(kvs)),
                None if n == "None" => e(ExprKind::Lit(Lit::None)),
                None if variant(n) => call(name(n), Vec::new()),
                None => name(n),
            },
            y::Atom::Lit(l, _) => e(ExprKind::Lit(match l {
                y::Literal::Int(s) => Lit::Int(s.clone()),
                y::Literal::Float(s) => Lit::Float(s.clone()),
                y::Literal::Str(s) => Lit::Str {
                    prefix: String::new(),
                    value: s.clone(),
                },
                y::Literal::Char(c) => Lit::Char(*c),
                y::Literal::Bool(b) => Lit::Bool(*b),
                y::Literal::Unit => Lit::None,
            })),
        })
    }

    fn atom_ty(&self, a: &y::Atom) -> Option<&y::Type> {
        match a {
            y::Atom::Name(n, _) => self.types.get(n),
            y::Atom::Lit(..) => None,
        }
    }

    fn atom_is_str(&self, a: &y::Atom) -> bool {
        match a {
            y::Atom::Lit(y::Literal::Str(_), _) => true,
            a => matches!(self.atom_ty(a), Some(y::Type::Named(n)) if n.path == ["Str"]),
        }
    }

    fn pure(&mut self, x: &y::Expr) -> R<Expr> {
        match x {
            y::Expr::Atom(a) => self.atom(a),
            y::Expr::Call(c) => self.call(c),
        }
    }

    fn place(&mut self, p: &y::Place) -> R<Expr> {
        let mut x = self.atom(&y::Atom::Name(p.root.clone(), p.root_span))?;
        for pr in &p.projs {
            x = match pr {
                // A tuple's field `_0` is written `t[0]`.
                y::Proj::Field(f, _) => match f.strip_prefix('_') {
                    Some(i) if !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit()) => {
                        e(ExprKind::Index(
                            Box::new(x),
                            Box::new(e(ExprKind::Lit(Lit::Int(i.into())))),
                        ))
                    }
                    _ => e(ExprKind::Attr(Box::new(x), f.clone(), AT)),
                },
                y::Proj::Index(a) => {
                    let i = negative_index(&x, self.atom(a)?);
                    e(ExprKind::Index(Box::new(x), Box::new(i)))
                }
            };
        }
        Ok(x)
    }

    fn rhs(&mut self, r: &y::Rhs) -> R<Expr> {
        match r {
            y::Rhs::Atom(a) => self.atom(a),
            y::Rhs::Call(c) | y::Rhs::Try(c) => self.call(c),
            y::Rhs::Propagate { call, via: None } => {
                Ok(e(ExprKind::Try(Box::new(self.call(call)?))))
            }
            y::Rhs::Propagate { .. } => no("`?` with a conversion"),
            y::Rhs::Proj(p) | y::Rhs::Move(p) => self.place(p),
            y::Rhs::Copy(p) => {
                let x = self.place(p)?;
                Ok(call(attr(x, "copy"), Vec::new()))
            }
            y::Rhs::Ctor(c) => self.ctor(c),
            y::Rhs::Closure(c) => self.closure(c),
        }
    }

    fn ctor(&mut self, c: &y::Ctor) -> R<Expr> {
        match c {
            y::Ctor::Tuple(xs) => Ok(e(ExprKind::Tuple(
                xs.iter().map(|a| self.atom(a)).collect::<R<_>>()?,
            ))),
            y::Ctor::Named { path, fields, .. } => match (path.as_slice(), fields.as_slice()) {
                ([s], [(v, a)]) if s == "Some" && v == "value" => {
                    self.marks.consume(&[self.marks.field(c, 0)]);
                    self.atom(a)
                }
                ([n], []) if n == "None" => Ok(e(ExprKind::Lit(Lit::None))),
                _ => {
                    let f = path_expr(path);
                    let args = fields
                        .iter()
                        .enumerate()
                        .map(|(k, (n, a))| {
                            let mark = self.marks.field(c, k);
                            self.marks.consume(std::slice::from_ref(&mark));
                            let positional = matches!(mark, Some((Mark::Positional, _)));
                            Ok(Arg {
                                name: (!positional).then(|| n.clone()),
                                kind: ArgKind::Expr(self.atom(a)?),
                                span: AT,
                            })
                        })
                        .collect::<R<_>>()?;
                    Ok(e(ExprKind::Call(Box::new(f), args)))
                }
            },
        }
    }

    fn closure(&mut self, c: &y::Closure) -> R<Expr> {
        let mut names = Vec::new();
        for p in &c.params {
            let y::Param::Value { name, ty: t, .. } = p else {
                return no("a closure with a capability parameter");
            };
            self.types.insert(name.clone(), t.clone());
            names.push(name.clone());
        }
        count_stmts(&c.body.stmts, &mut self.uses);
        let mut ss = self.stmts(&c.body.stmts)?;
        match (ss.pop(), ss.is_empty()) {
            (
                Some(Stmt {
                    kind: StmtKind::Return(Some(x)),
                    ..
                }),
                true,
            ) => Ok(e(ExprKind::Lambda(names, Box::new(x)))),
            _ => no("a closure whose body is not one expression"),
        }
    }

    fn args(&mut self, args: &[y::Arg]) -> R<Vec<Arg>> {
        let mut out = Vec::new();
        for a in args {
            if let y::Arg::Value(conv, a, _) = a {
                let x = self.atom(a)?;
                out.push(Arg {
                    name: None,
                    kind: if *conv == y::Conv::Inout {
                        ArgKind::Ref(x)
                    } else {
                        ArgKind::Expr(x)
                    },
                    span: AT,
                });
            }
        }
        Ok(out)
    }

    fn values<'c>(&self, c: &'c y::Call) -> Vec<&'c y::Atom> {
        c.args
            .iter()
            .filter_map(|a| match a {
                y::Arg::Value(_, a, _) => Some(a),
                y::Arg::Cap(..) => None,
            })
            .collect()
    }

    fn call(&mut self, c: &y::Call) -> R<Expr> {
        let x = self.call_marked(c)?;
        self.marks.consume(&self.marks.args(c));
        Ok(x)
    }

    fn call_marked(&mut self, c: &y::Call) -> R<Expr> {
        let p = c.path.join(".");
        let vals = self.values(c);
        if let ([a, b], Some(op)) = (vals.as_slice(), binary(&p)) {
            let (a, b) = (self.atom(a)?, self.atom(b)?);
            return Ok(e(if COMPARE.contains(&op) {
                ExprKind::Compare(op, Box::new(a), Box::new(b))
            } else {
                ExprKind::Binary(op, Box::new(a), Box::new(b))
            }));
        }
        if let Some((f, ty)) = p.rsplit_once('_')
            && ty != "bool"
            && !p.contains('.')
        {
            if f == "not" && vals.len() == 1 {
                let x = self.atom(vals[0])?;
                return Ok(e(ExprKind::Unary("~", Box::new(x))));
            }
            if yan_std::intrinsics::BITS.contains(&f) {
                let args = vals
                    .iter()
                    .map(|v| self.atom(v).map(arg))
                    .collect::<R<Vec<_>>>()?;
                return Ok(call(attr(name("bits"), f), args));
            }
        }
        if p == "not_bool" && vals.len() == 1 {
            let x = self.atom(vals[0])?;
            return Ok(match x.kind {
                ExprKind::Compare("in", a, b) => e(ExprKind::Compare("not in", a, b)),
                _ => e(ExprKind::Not(Box::new(x))),
            });
        }
        if let Some(op) = p.strip_prefix("neg_").map(|_| "-")
            && vals.len() == 1
        {
            let x = self.atom(vals[0])?;
            return Ok(e(ExprKind::Unary(op, Box::new(x))));
        }
        if p == "std.text.concat" {
            return self.concat(c);
        }
        // `str(x)` lowers the same way; the f-string is the more common spelling.
        if p == "std.fmt.show" && vals.len() == 1 {
            let x = self.atom(vals[0])?;
            return Ok(e(ExprKind::FStr {
                template: false,
                pieces: vec![FPiece::Expr(x, None)],
            }));
        }
        if let Some((m, f)) = p.strip_prefix("std.").and_then(|s| s.rsplit_once('.')) {
            // A float `sum` is its own function only below Yan Agent.
            let f = if f == "sum_float" { "sum" } else { f };
            if let ([a], true) = (vals.as_slice(), m.starts_with("time.") && f == "neg") {
                let a = self.atom(a)?;
                return Ok(e(ExprKind::Unary("-", Box::new(a))));
            }
            if let ([a, b], Some(op)) = (vals.as_slice(), time_operator(m, f)) {
                let (a, b) = (self.atom(a)?, self.atom(b)?);
                return Ok(e(ExprKind::Binary(op, Box::new(a), Box::new(b))));
            }
            return self.std_call(c, m, f);
        }
        let mut args = self.args(&c.args)?;
        let marks = self.marks.args(c);
        let names = self.marks.fns.get(&p);
        for (i, a) in args.iter_mut().enumerate() {
            match (marks.get(i), names.and_then(|n| n.get(i))) {
                (Some(Some((Mark::Keyword, _))), Some(n)) => a.name = Some(n.clone()),
                (Some(m @ Some((Mark::Keyword, _))), None) => self.marks.reject(m),
                _ => {}
            }
        }
        Ok(call(path_expr(&c.path), args))
    }

    /// A left-to-right concatenation chain (R478) as an f-string when it shows a value or
    /// has literal pieces, and as `+` otherwise.
    fn concat(&mut self, c: &y::Call) -> R<Expr> {
        let vals = self.values(c);
        let [a, b] = vals.as_slice() else {
            return no("`std.text.concat` with other than two arguments");
        };
        let mut parts = Vec::new();
        let mut shown = false;
        for x in [a, b] {
            match self.atom(x)? {
                Expr {
                    kind: ExprKind::FStr { pieces, .. },
                    ..
                } if parts.is_empty() || pieces.len() == 1 => {
                    shown = true;
                    parts.extend(pieces)
                }
                Expr {
                    kind: ExprKind::Lit(Lit::Str { prefix, value }),
                    ..
                } if prefix.is_empty() => parts.push(FPiece::Lit(value)),
                x => parts.push(FPiece::Expr(x, None)),
            }
        }
        let lits = parts.iter().filter(|p| matches!(p, FPiece::Lit(_))).count();
        let fmt = shown || (lits > 0 && lits < parts.len());
        if fmt {
            let mut merged: Vec<FPiece> = Vec::new();
            for p in parts {
                match (merged.last_mut(), p) {
                    (Some(FPiece::Lit(a)), FPiece::Lit(b)) => a.push_str(&b),
                    (_, p) => merged.push(p),
                }
            }
            return Ok(e(ExprKind::FStr {
                template: false,
                pieces: merged,
            }));
        }
        let mut xs = parts.into_iter().map(|p| match p {
            FPiece::Expr(x, _) => x,
            FPiece::Lit(value) => e(ExprKind::Lit(Lit::Str {
                prefix: String::new(),
                value,
            })),
        });
        let first = xs.next().expect("two parts");
        Ok(xs.fold(first, |a, b| {
            e(ExprKind::Binary("+", Box::new(a), Box::new(b)))
        }))
    }

    fn std_call(&mut self, c: &y::Call, module: &str, f: &str) -> R<Expr> {
        let vals = self.values(c);
        // Collection constructors and builtins (13.3).
        match (module, f, vals.len()) {
            ("list", "new", 0) => return Ok(e(ExprKind::List(Vec::new()))),
            ("dict", "new", 0) => return Ok(e(ExprKind::Dict(Vec::new()))),
            ("set", "new", 0) => return Ok(call(name("set"), Vec::new())),
            (m, "len", 1) if TYPE_MODULES.contains(&m) => {
                let x = self.atom(vals[0])?;
                return Ok(match self.marks.spelling(c) {
                    Some(Mark::Method) => call(attr(x, "len"), Vec::new()),
                    _ => call(name("len"), vec![arg(x)]),
                });
            }
            // `len(s)` on a `str` elaborates to `char_count` (N21).
            ("text", "char_count", 1) if self.marks.spelling(c) == Some(Mark::Function) => {
                let x = self.atom(vals[0])?;
                return Ok(call(name("len"), vec![arg(x)]));
            }
            ("prelude", f, _) => {
                let args = self.std_args(c, &format!("std.prelude.{f}"), 0)?;
                return Ok(call(name(f), args));
            }
            ("cmp", f, 2) if binary(&format!("{f}_")).is_some_and(|o| COMPARE.contains(&o)) => {
                let op = binary(&format!("{f}_")).expect("matched");
                let (a, b) = (self.atom(vals[0])?, self.atom(vals[1])?);
                return Ok(e(ExprKind::Compare(op, Box::new(a), Box::new(b))));
            }
            // A pattern with a backslash is written raw.
            ("regex", "compile", 1) => {
                let mut x = self.atom(vals[0])?;
                if let ExprKind::Lit(Lit::Str { prefix, value }) = &mut x.kind
                    && value.contains('\\')
                    && !value.ends_with('\\')
                    && !value.contains(['"', '\n', '\r'])
                {
                    *prefix = "r".into();
                }
                return Ok(call(attr(name("regex"), f), vec![arg(x)]));
            }
            ("error", "context", 2) => {
                let recv = self.atom(vals[0])?;
                let args = self.args(&c.args[1..])?;
                return Ok(call(attr(recv, f), args));
            }
            (m, "contains" | "contains_key", 2) if TYPE_MODULES.contains(&m) => {
                let (coll, x) = (self.atom(vals[0])?, self.atom(vals[1])?);
                return Ok(e(ExprKind::Compare("in", Box::new(x), Box::new(coll))));
            }
            _ => {}
        }
        // A capability operation is a method of the capability.
        if let Some(y::Arg::Cap(cap, _)) = c.args.first() {
            let args = self.args(&c.args[1..])?;
            return Ok(call(attr(name(cap), f), args));
        }
        // A method of the type its module declares (17.2): its first parameter is `self`.
        let path = format!("std.{module}.{f}");
        if let Some(recv) = vals.first()
            && let Some(tm) = self.recv_module(recv)
            && (module == tm || module.starts_with(&format!("{tm}.")))
            && self
                .std
                .params(&path)
                .is_none_or(|ps| ps.first().is_some_and(|p| p.name == "self"))
        {
            let recv = self.atom(recv)?;
            let args = self.std_args(c, &format!("std.{module}.{f}"), 1)?;
            return Ok(call(attr(recv, f), args));
        }
        let args = self.std_args(c, &format!("std.{module}.{f}"), 0)?;
        let callee = if module == "test" {
            name(f)
        } else if let Some(t) = static_type(module, f) {
            attr(name(t), f)
        } else {
            attr(
                path_expr(&module.split('.').map(str::to_owned).collect::<Vec<_>>()),
                f,
            )
        };
        Ok(call(callee, args))
    }

    fn recv_module(&self, a: &y::Atom) -> Option<String> {
        if self.atom_is_str(a) || matches!(a, y::Atom::Lit(y::Literal::Char(_), _)) {
            return Some("text".into());
        }
        let t = self.atom_ty(a)?;
        if let Some(m) = type_module(t) {
            return Some(m.into());
        }
        match t {
            y::Type::Named(n) if n.path.len() == 1 => self.std.type_module(&n.path[0]),
            _ => None,
        }
    }

    /// A std call's arguments from the `skip`th on: those equal to the parameter's default
    /// are dropped, and later ones are named, as Yan Agent writes them (13.3).
    fn std_args(&mut self, c: &y::Call, path: &str, skip: usize) -> R<Vec<Arg>> {
        let mut args = self.args(&c.args)?;
        let marks = self.marks.args(c);
        let Some(params) = self.std.params(path) else {
            return Ok(args.split_off(skip.min(args.len())));
        };
        let mut out = Vec::new();
        // Yan Agent passes an optional parameter by keyword, except the first argument after the
        // receiver or a positional-only one, and leaves it out when it has its default.
        for (i, a) in args.into_iter().enumerate().skip(skip) {
            let (Some(p), ArgKind::Expr(x)) = (params.get(i), &a.kind) else {
                out.push(a);
                continue;
            };
            // yan-elab passes a list or a `str` to an `iter` parameter as `x.iter()`.
            let a = match &x.kind {
                ExprKind::Call(f, xs) if p.iter && xs.is_empty() => match &f.kind {
                    ExprKind::Attr(r, m, _) if m == "iter" => Arg {
                        kind: ArgKind::Expr((**r).clone()),
                        ..a
                    },
                    _ => a,
                },
                _ => a,
            };
            let ArgKind::Expr(x) = &a.kind else {
                unreachable!("an expression argument")
            };
            let mark = marks.get(i).and_then(Option::as_ref).map(|(m, _)| m);
            match (mark, &p.default) {
                // A marked argument was written, even if it equals the default.
                (Some(Mark::Keyword), _) if !p.positional => out.push(Arg {
                    name: Some(p.name.clone()),
                    ..a
                }),
                (Some(Mark::Keyword), _) => {
                    self.marks.reject(&marks[i]);
                    out.push(a)
                }
                (Some(_), _) => out.push(a),
                (None, Some(d)) if same(x, d) => {}
                (None, Some(_)) if i > skip && !p.positional => out.push(Arg {
                    name: Some(p.name.clone()),
                    ..a
                }),
                _ => out.push(a),
            }
        }
        Ok(out)
    }

    fn bind_pending(&mut self, n: &str, x: Pending) {
        self.pending.insert(n.to_owned(), x);
    }

    fn stmts(&mut self, ss: &[y::Stmt]) -> R<Vec<Stmt>> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < ss.len() {
            i += self.stmt(&ss[i..], &mut out)?;
        }
        Ok(out)
    }

    fn block(&mut self, b: &y::Block) -> R<Block> {
        Ok(Block {
            inline: false,
            stmts: self.stmts(&b.stmts)?,
        })
    }

    /// `var t = copy p` and stores to its fields as `evolve(p, f=v, ...)` (14), pending at
    /// `t`'s one use after them; returns how many statements after the copy it consumed.
    fn evolve(
        &mut self,
        t: &str,
        from: &y::Place,
        rest: &[y::Stmt],
        out: &mut Vec<Stmt>,
    ) -> R<Option<usize>> {
        use y::StmtKind as K;
        let mut stores = 0;
        let mut end = 0;
        for (k, s) in rest.iter().enumerate() {
            match &s.kind {
                K::Let {
                    pat: y::Pat::Bind(x),
                    ..
                } if generated(x) => {}
                K::Assign { place, .. }
                    if place.root == t
                        && matches!(place.projs.as_slice(), [y::Proj::Field(..)]) =>
                {
                    stores += 1;
                    end = k + 1;
                }
                _ => break,
            }
        }
        if stores == 0 || self.uses(t) != stores + 1 {
            return Ok(None);
        }
        let mut args = Vec::new();
        let mut k = 0;
        while k < end {
            match &rest[k].kind {
                K::Assign { place, rhs, .. } => {
                    let [y::Proj::Field(f, _)] = place.projs.as_slice() else {
                        unreachable!("matched")
                    };
                    args.push(Arg {
                        name: Some(f.clone()),
                        kind: ArgKind::Expr(self.rhs(rhs)?),
                        span: AT,
                    });
                    k += 1;
                }
                _ => k += self.stmt(&rest[k..], out)?,
            }
        }
        let mut all = vec![arg(self.place(from)?)];
        all.extend(args);
        self.bind_pending(t, Pending::Expr(call(name("evolve"), all)));
        Ok(Some(end))
    }

    /// Raises the statement at the head of `ss` into `out`; returns how many it consumed.
    fn stmt(&mut self, ss: &[y::Stmt], out: &mut Vec<Stmt>) -> R<usize> {
        use y::StmtKind as K;
        let s = &ss[0];
        if let K::Let { rhs, .. } | K::Assign { rhs, .. } | K::Rhs(rhs) = &s.kind
            && let y::Rhs::Call(c) | y::Rhs::Try(c) | y::Rhs::Propagate { call: c, .. } = rhs
        {
            self.marks.consume(&self.marks.args(c));
        }
        match &s.kind {
            K::Let {
                pat: y::Pat::Bind(n),
                ty: t,
                rhs,
                mutable,
                ..
            } => {
                self.types.insert(n.clone(), t.clone());
                if *mutable
                    && generated(n)
                    && let Some(k) = self.for_loop(n, rhs, &ss[1..], out)?
                {
                    return Ok(1 + k);
                }
                if *mutable
                    && generated(n)
                    && let y::Rhs::Copy(from) = rhs
                    && let Some(k) = self.evolve(n, from, &ss[1..], out)?
                {
                    return Ok(1 + k);
                }
                if generated(n) {
                    let (names, k) = destructured(n, &ss[1..]);
                    if !names.is_empty() && self.uses(n) == names.len() {
                        let value = self.rhs(rhs)?;
                        for (i, b) in names.iter().enumerate() {
                            if let K::Let { ty, .. } = &ss[1 + names.len() + i].kind {
                                self.types.insert(b.clone(), ty.clone());
                            }
                        }
                        out.push(stmt(StmtKind::Assign {
                            target: e(ExprKind::Tuple(names.iter().map(|b| name(b)).collect())),
                            ty: None,
                            value,
                        }));
                        return Ok(1 + k);
                    }
                    let builder = match rhs {
                        y::Rhs::Call(c) if c.args.is_empty() => match c.path.join(".").as_str() {
                            "std.list.new" => Some(Pending::List(Vec::new())),
                            "std.set.new" => Some(Pending::Set(Vec::new())),
                            "std.dict.new" => Some(Pending::Dict(Vec::new())),
                            _ => None,
                        },
                        _ => None,
                    };
                    let builds = count_builds(n, &ss[1..]);
                    // A `for` over the list also takes its length and indexes it.
                    let loops = usize::from(for_over(n, &ss[1..]));
                    if let Some(b) = builder
                        && self.uses(n) == builds + 1 + loops
                    {
                        self.bind_pending(n, b);
                        return Ok(1);
                    }
                    // yan-elab leaves unused temporaries where it lowers an expression twice;
                    // a std call without capabilities or `inout` arguments has no effect.
                    let pure = matches!(rhs, y::Rhs::Closure(_))
                        || matches!(rhs, y::Rhs::Call(c)
                            if c.path.first().is_some_and(|p| p == "std")
                                && c.args.iter().all(|a| matches!(a, y::Arg::Value(y::Conv::Let, ..))));
                    if self.uses(n) == 0 && pure {
                        return Ok(1);
                    }
                    if pure {
                        self.pure.insert(n.clone());
                    }
                    if self.uses(n) == 1 {
                        let x = self.rhs(rhs)?;
                        self.bind_pending(n, Pending::Expr(x));
                        return Ok(1);
                    }
                    if self.uses(n) == 0 {
                        let x = self.rhs(rhs)?;
                        out.push(stmt(StmtKind::Expr(x)));
                        return Ok(1);
                    }
                }
                let value = self.rhs(rhs)?;
                let ty = match self.marks.annotated(s) {
                    Some(true) => match ty(t) {
                        Ok(t) => Some(t),
                        Err(_) => {
                            self.marks.reject_stmt(s);
                            annotation(&value, t)?
                        }
                    },
                    Some(false) => None,
                    None => annotation(&value, t)?,
                };
                out.push(stmt(StmtKind::Assign {
                    target: name(n),
                    ty,
                    value,
                }));
            }
            K::Let {
                pat: y::Pat::Tuple(ps),
                rhs,
                ..
            } => {
                let mut names = Vec::new();
                for p in ps {
                    let y::Pat::Bind(n) = p else {
                        return no("a nested `let` pattern");
                    };
                    names.push(name(n));
                }
                let value = self.rhs(rhs)?;
                out.push(stmt(StmtKind::Assign {
                    target: e(ExprKind::Tuple(names)),
                    ty: None,
                    value,
                }));
            }
            K::Let { .. } => return no("this `let` pattern"),
            K::Assign { place, rhs, .. } => {
                let target = self.place(place)?;
                let concat = |c: &y::Call| (c.path.join(".") == "std.text.concat").then_some("+");
                if place.projs.is_empty()
                    && let y::Rhs::Call(c) = rhs
                    && let Some(op) = binary(&c.path.join(".")).or_else(|| concat(c))
                    && !COMPARE.contains(&op)
                    && let [
                        y::Arg::Value(_, y::Atom::Name(a, _), _),
                        y::Arg::Value(_, b, _),
                    ] = c.args.as_slice()
                    && *a == place.root
                {
                    let value = self.atom(b)?;
                    out.push(stmt(StmtKind::AugAssign {
                        target,
                        op: aug(op),
                        value,
                    }));
                } else {
                    let value = self.rhs(rhs)?;
                    out.push(stmt(StmtKind::Assign {
                        target,
                        ty: None,
                        value,
                    }));
                }
            }
            K::Rhs(y::Rhs::Call(c))
                if BUILDERS.contains(&c.path.join(".").as_str())
                    && let Some(y::Arg::Value(y::Conv::Inout, y::Atom::Name(t, _), _)) =
                        c.args.first()
                    && self.pending.contains_key(t) =>
            {
                let vals: Vec<y::Atom> = self.values(c).into_iter().cloned().collect();
                let xs = vals[1..]
                    .iter()
                    .map(|a| self.atom(a))
                    .collect::<R<Vec<_>>>()?;
                match (self.pending.get_mut(t), xs.as_slice()) {
                    (Some(Pending::List(v) | Pending::Set(v)), [x]) => v.push(x.clone()),
                    (Some(Pending::Dict(v)), [k, x]) => v.push((k.clone(), x.clone())),
                    _ => return no("a collection built in pieces"),
                }
            }
            K::Rhs(y::Rhs::Call(c))
                if matches!(
                    c.path.join(".").as_str(),
                    "std.dict.insert" | "std.collections.sorted_dict.insert"
                ) && let [
                    y::Arg::Value(y::Conv::Inout, d, _),
                    y::Arg::Value(_, k, _),
                    y::Arg::Value(_, v, _),
                ] = c.args.as_slice() =>
            {
                let d = self.atom(d)?;
                let k = self.atom(k)?;
                let value = self.atom(v)?;
                out.push(stmt(StmtKind::Assign {
                    target: e(ExprKind::Index(Box::new(d), Box::new(k))),
                    ty: None,
                    value,
                }));
            }
            K::Rhs(r) => {
                let x = self.rhs(r)?;
                out.push(stmt(StmtKind::Expr(x)));
            }
            K::If { cond, then, els } => {
                let c = self.atom(cond)?;
                let then = self.block(then)?;
                let els = els.as_ref().map(|b| self.block(b)).transpose()?;
                let mut arms = vec![(c, then)];
                let els = match els {
                    Some(b)
                        if b.stmts.len() == 1 && matches!(b.stmts[0].kind, StmtKind::If { .. }) =>
                    {
                        let StmtKind::If { arms: more, els } =
                            b.stmts.into_iter().next().expect("one").kind
                        else {
                            unreachable!("matched")
                        };
                        arms.extend(more);
                        els
                    }
                    els => els,
                };
                out.push(stmt(StmtKind::If { arms, els }));
            }
            K::Ret(a) => {
                let x = self.atom(a)?;
                let r = match x.kind {
                    ExprKind::Lit(Lit::None) if matches!(a, y::Atom::Lit(y::Literal::Unit, _)) => {
                        None
                    }
                    _ => Some(x),
                };
                out.push(stmt(StmtKind::Return(r)));
            }
            K::Fail(a) => {
                let x = self.atom(a)?;
                out.push(stmt(StmtKind::Fail(x)));
            }
            K::Loop { label, body, .. }
                if let Some(k) = self.iter_loop(label, &body.stmts, out)? =>
            {
                return Ok(k);
            }
            K::Loop { label, body, .. } => {
                let ss = &body.stmts;
                // `while c` is a loop whose first statement after `c`'s temporaries is
                // `if c {} else { break }`.
                let j = ss.iter().position(|s| {
                    !matches!(&s.kind,
                    K::Let { pat: y::Pat::Bind(n), .. } if generated(n))
                });
                let test = j.and_then(|j| match &ss[j].kind {
                    K::If { cond, then, els: Some(b) }
                        if then.stmts.is_empty()
                            && matches!(b.stmts.as_slice(),
                                [y::Stmt { kind: K::Break { label: l, value: None }, .. }] if l == label) =>
                    {
                        Some((j, cond.clone()))
                    }
                    _ => None,
                });
                let (cond, rest) = match test {
                    Some((j, c)) => {
                        let pre = self.stmts(&ss[..j])?;
                        if !pre.is_empty() {
                            return no("a loop condition with statements");
                        }
                        (self.atom(&c)?, &ss[j + 1..])
                    }
                    None => (e(ExprKind::Lit(Lit::Bool(true))), &ss[..]),
                };
                let b = self.loop_body(label, rest)?;
                out.push(stmt(StmtKind::While {
                    label: None,
                    cond,
                    contracts: Vec::new(),
                    body: b,
                }));
            }
            K::Break { label, value: None } if self.loops.last() == Some(label) => {
                out.push(stmt(StmtKind::Break))
            }
            K::Continue { label } if self.loops.last() == Some(label) => {
                out.push(stmt(StmtKind::Continue))
            }
            K::Break { .. } | K::Continue { .. } => {
                return no("a `break` or `continue` to an outer loop");
            }
            K::Jp {
                name: k,
                params,
                body,
            } => {
                if let Some(next) = ss.get(1)
                    && let Some(x) = self.fallback(k, &next.kind)?
                {
                    let [y::Param::Value { name: v, ty: t, .. }] = params.as_slice() else {
                        return no("a join point with other than one parameter");
                    };
                    self.types.insert(v.clone(), t.clone());
                    let (names, k) = destructured(v, &body.stmts);
                    if names.is_empty() {
                        self.bind_pending(v, Pending::Expr(x));
                    } else {
                        out.push(stmt(StmtKind::Assign {
                            target: e(ExprKind::Tuple(names.iter().map(|n| name(n)).collect())),
                            ty: None,
                            value: x,
                        }));
                    }
                    out.extend(self.stmts(&body.stmts[k..])?);
                    return Ok(2);
                }
                return no("join points");
            }
            K::Match { scrutinee, arms } => {
                let scrutinee = self.atom(scrutinee)?;
                let mut cases = Vec::new();
                for (p, b) in arms {
                    cases.push(Case {
                        pat: pat(p)?,
                        span: AT,
                        guard: None,
                        body: self.block(b)?,
                    });
                }
                out.push(stmt(StmtKind::Match { scrutinee, cases }));
            }
            K::With {
                name: n,
                dyn_: false,
                handler,
                body,
            } => {
                let h = self.atom(handler)?;
                let body = self.block(body)?;
                out.push(stmt(StmtKind::With {
                    kind: WithKind::Bind(n.clone(), h),
                    body,
                }));
            }
            K::Deadline { at, body } => {
                let at = self.atom(at)?;
                let body = self.block(body)?;
                out.push(stmt(StmtKind::With {
                    kind: WithKind::Deadline(at),
                    body,
                }));
            }
            K::Assert(x) => {
                let x = self.pure(x)?;
                if out.is_empty() && self.marks.marked_pre(s) {
                    self.pre.push(x);
                } else {
                    out.push(stmt(StmtKind::Assert(x)));
                }
            }
            K::Unreachable => out.push(stmt(StmtKind::Unreachable)),
            K::Drop(_) => {}
            other => return no(format!("this statement: {}", kind_name(other))),
        }
        Ok(1)
    }

    fn loop_body(&mut self, label: &str, body: &[y::Stmt]) -> R<Vec<Stmt>> {
        self.loops.push(label.to_owned());
        let r = self.stmts(body);
        self.loops.pop();
        r
    }

    /// A `for` over a list or a `range`, as elaboration expands it (`for_` in yan-elab): a
    /// counter `i` starting at `var` `n`, an optional length temporary, then a loop that
    /// breaks when `i >= stop`, binds the target, and steps `i`.
    fn for_loop(
        &mut self,
        i: &str,
        start: &y::Rhs,
        rest: &[y::Stmt],
        out: &mut Vec<Stmt>,
    ) -> R<Option<usize>> {
        use y::StmtKind as K;
        let y::Rhs::Atom(start) = start else {
            return Ok(None);
        };
        // The list case binds its length first.
        let (len, k) = match rest.first().map(|s| &s.kind) {
            Some(K::Let {
                pat: y::Pat::Bind(n),
                rhs: y::Rhs::Call(c),
                ..
            }) if generated(n) && c.path.last().is_some_and(|f| f == "len") => {
                (Some((n.clone(), c.clone())), 1)
            }
            _ => (None, 0),
        };
        let Some(y::Stmt {
            kind: K::Loop { label, body, .. },
            ..
        }) = rest.get(k)
        else {
            return Ok(None);
        };
        let [check, exit, bind, tail @ ..] = body.stmts.as_slice() else {
            return Ok(None);
        };
        let K::Let {
            pat: y::Pat::Bind(done),
            rhs: y::Rhs::Call(ge),
            ..
        } = &check.kind
        else {
            return Ok(None);
        };
        let [
            y::Arg::Value(_, y::Atom::Name(ci, _), _),
            y::Arg::Value(_, stop, _),
        ] = ge.args.as_slice()
        else {
            return Ok(None);
        };
        if ge.path != ["ge_i64"] || ci != i {
            return Ok(None);
        }
        match &exit.kind {
            K::If {
                cond: y::Atom::Name(c, _),
                then,
                els: None,
            } if c == done
                && matches!(then.stmts.as_slice(),
                    [y::Stmt { kind: K::Break { label: l, value: None }, .. }] if l == label) => {}
            _ => return Ok(None),
        }
        // The target: one name, or a tuple through a temporary and its fields.
        let K::Let {
            pat: y::Pat::Bind(t),
            ty: tt,
            rhs: cur,
            ..
        } = &bind.kind
        else {
            return Ok(None);
        };
        let mut targets = vec![t.clone()];
        let mut tail = tail;
        let fields: Vec<(String, y::Type)> = tail
            .iter()
            .map_while(|s| match &s.kind {
                K::Let {
                    pat: y::Pat::Bind(n),
                    ty,
                    rhs: y::Rhs::Proj(p),
                    ..
                } if p.root == *t
                    && matches!(p.projs.as_slice(), [y::Proj::Field(f, _)] if f.starts_with('_')) =>
                {
                    Some((n.clone(), ty.clone()))
                }
                _ => None,
            })
            .collect();
        if generated(t) && !fields.is_empty() {
            targets = fields.iter().map(|(n, _)| n.clone()).collect();
            for (n, ty) in &fields {
                self.types.insert(n.clone(), ty.clone());
            }
            tail = &tail[fields.len()..];
        } else {
            self.types.insert(t.clone(), tt.clone());
        }
        let [step_stmt, body @ ..] = tail else {
            return Ok(None);
        };
        let K::Assign {
            place,
            rhs: y::Rhs::Call(add),
            ..
        } = &step_stmt.kind
        else {
            return Ok(None);
        };
        let [
            y::Arg::Value(_, y::Atom::Name(ai, _), _),
            y::Arg::Value(_, step, _),
        ] = add.args.as_slice()
        else {
            return Ok(None);
        };
        if place.root != i || !place.projs.is_empty() || add.path != ["add_checked_i64"] || ai != i
        {
            return Ok(None);
        }
        let iter = match (cur, &len) {
            (y::Rhs::Proj(p), Some((n, c)))
                if matches!(stop, y::Atom::Name(s, _) if s == n)
                    && matches!(start, y::Atom::Lit(y::Literal::Int(z), _) if z == "0")
                    && matches!(p.projs.as_slice(), [y::Proj::Index(y::Atom::Name(x, _))] if x == i) =>
            {
                let [y::Arg::Value(_, y::Atom::Name(xs, _), _)] = c.args.as_slice() else {
                    return Ok(None);
                };
                if *xs != p.root {
                    return Ok(None);
                }
                // The list was bound to a temporary used only by the loop.
                match out.last() {
                    Some(Stmt {
                        kind:
                            StmtKind::Assign {
                                target:
                                    Expr {
                                        kind: ExprKind::Name(n),
                                        ..
                                    },
                                ..
                            },
                        ..
                    }) if n == xs && generated(xs) && self.uses(xs) == 2 => {
                        let Some(Stmt {
                            kind: StmtKind::Assign { value, .. },
                            ..
                        }) = out.pop()
                        else {
                            unreachable!("matched")
                        };
                        value
                    }
                    _ => self.atom(&y::Atom::Name(xs.clone(), AT))?,
                }
            }
            (y::Rhs::Atom(y::Atom::Name(x, _)), None) if x == i => {
                let mut args = Vec::new();
                let zero = matches!(start, y::Atom::Lit(y::Literal::Int(z), _) if z == "0");
                let one = matches!(step, y::Atom::Lit(y::Literal::Int(o), _) if o == "1");
                if !zero || !one {
                    args.push(arg(self.atom(start)?));
                }
                args.push(arg(self.atom(stop)?));
                if !one {
                    args.push(arg(self.atom(step)?));
                }
                call(name("range"), args)
            }
            _ => return Ok(None),
        };
        let body = self.loop_body(label, body)?;
        out.push(stmt(StmtKind::For {
            label: None,
            target: targets,
            iter,
            contracts: Vec::new(),
            body,
        }));
        Ok(Some(k + 1))
    }

    /// A `for` over an iterator, as elaboration expands it (`for_iter` in yan-elab): a loop
    /// that breaks when `done(it)`, then binds the target from `current(it)`.
    fn iter_loop(
        &mut self,
        label: &str,
        body: &[y::Stmt],
        out: &mut Vec<Stmt>,
    ) -> R<Option<usize>> {
        use y::StmtKind as K;
        let [check, exit, bind, tail @ ..] = body else {
            return Ok(None);
        };
        let (
            K::Let {
                pat: y::Pat::Bind(done),
                rhs: y::Rhs::Call(d),
                ..
            },
            K::Let {
                pat: y::Pat::Bind(t),
                ty: tt,
                rhs: y::Rhs::Call(cur),
                ..
            },
        ) = (&check.kind, &bind.kind)
        else {
            return Ok(None);
        };
        let it = match (
            d.path.join(".").as_str(),
            d.args.as_slice(),
            cur.path.join(".").as_str(),
            cur.args.as_slice(),
        ) {
            (
                "std.iter.ops.done",
                [y::Arg::Value(_, a @ y::Atom::Name(i, _), _)],
                "std.iter.ops.current",
                [y::Arg::Value(_, y::Atom::Name(j, _), _)],
            ) if i == j => a.clone(),
            _ => return Ok(None),
        };
        if !matches!(&exit.kind, K::If { cond: y::Atom::Name(c, _), then, els: None }
            if c == done && matches!(then.stmts.as_slice(),
                [y::Stmt { kind: K::Break { label: l, value: None }, .. }] if l == label))
        {
            return Ok(None);
        }
        let (names, k) = if generated(t) {
            let (ns, k) = destructured_from(t, tail);
            (ns, k)
        } else {
            (Vec::new(), 0)
        };
        let targets = if names.is_empty() {
            self.types.insert(t.clone(), tt.clone());
            vec![t.clone()]
        } else {
            for s in &tail[..k] {
                if let K::Let {
                    pat: y::Pat::Bind(n),
                    ty,
                    ..
                } = &s.kind
                {
                    self.types.insert(n.clone(), ty.clone());
                }
            }
            names
        };
        // The iterator was bound to a temporary used only by the loop.
        let iter = match (&it, out.last()) {
            (
                y::Atom::Name(n, _),
                Some(Stmt {
                    kind:
                        StmtKind::Assign {
                            target:
                                Expr {
                                    kind: ExprKind::Name(m),
                                    ..
                                },
                            ..
                        },
                    ..
                }),
            ) if n == m && generated(n) && self.uses(n) == 2 => {
                let Some(Stmt {
                    kind: StmtKind::Assign { value, .. },
                    ..
                }) = out.pop()
                else {
                    unreachable!("matched")
                };
                value
            }
            _ => self.atom(&it)?,
        };
        let body = self.loop_body(label, &tail[k..])?;
        out.push(stmt(StmtKind::For {
            label: None,
            target: targets,
            iter,
            contracts: Vec::new(),
            body,
        }));
        Ok(Some(1))
    }

    /// The value an arm passes to its join point, with the temporaries computing it folded in.
    fn arm_value(&mut self, pre: &[y::Stmt], v: &y::Atom) -> R<Expr> {
        if !self.stmts(pre)?.is_empty() {
            return no("a fallback value with statements");
        }
        self.atom(v)
    }

    /// `x else v` as elaboration expands it: a join point `k` followed by a match on an
    /// optional or a result whose arms each jump to `k`.
    fn fallback(&mut self, k: &str, next: &y::StmtKind) -> R<Option<Expr>> {
        let y::StmtKind::Match {
            scrutinee: y::Atom::Name(s, _),
            arms,
        } = next
        else {
            return Ok(None);
        };
        // An arm that jumps to `k`, after temporaries for the argument.
        fn jump<'b>(k: &str, b: &'b y::Block) -> Option<(&'b [y::Stmt], y::Atom)> {
            let (last, pre) = b.stmts.split_last()?;
            match &last.kind {
                y::StmtKind::Jump { name, args }
                    if name == k
                        && args.len() == 1
                        && pre.iter().all(|s| {
                            matches!(&s.kind,
                            y::StmtKind::Let { pat: y::Pat::Bind(t), .. } if generated(t))
                        }) =>
                {
                    Some((pre, args[0].clone()))
                }
                _ => None,
            }
        }
        let mut other = None;
        let mut bound = None;
        let mut some = false;
        for (p, b) in arms {
            match p {
                y::Pat::Ctor { path, .. } if path == &["Some"] || path == &["Ok"] => {
                    if !jump(k, b).is_some_and(|(pre, _)| pre.is_empty()) {
                        return Ok(None);
                    }
                    some = true;
                }
                y::Pat::Ctor { path, fields: None } if path == &["None"] => other = Some(b),
                y::Pat::Bind(n) if n == "None" => other = Some(b),
                y::Pat::Ctor {
                    path,
                    fields: Some((fs, _)),
                } if path == &["Err"] => {
                    other = Some(b);
                    if let [(_, y::Pat::Bind(n))] = fs.as_slice() {
                        bound = Some(n.clone());
                    }
                }
                _ => return Ok(None),
            }
        }
        let (true, Some(b)) = (some, other) else {
            return Ok(None);
        };
        if let Some(n) = &bound
            && let Some((pre, v)) = jump(k, b)
        {
            let x = self.atom(&y::Atom::Name(s.clone(), AT))?;
            let v = self.arm_value(pre, &v)?;
            return Ok(Some(e(ExprKind::Fallback(
                Box::new(x),
                Box::new(Fallback::Bind(n.clone(), v)),
            ))));
        }
        let x = self.atom(&y::Atom::Name(s.clone(), AT))?;
        let fb = match jump(k, b) {
            Some((pre, v)) => Fallback::Value(self.arm_value(pre, &v)?),
            None => {
                let mut ss = self.stmts(&b.stmts)?;
                let (Some(last), true) = (ss.pop(), ss.is_empty()) else {
                    return no("a fallback block");
                };
                match last.kind {
                    StmtKind::Fail(v) => Fallback::Fail(v),
                    StmtKind::Return(v) => Fallback::Return(v),
                    StmtKind::Break => Fallback::Break,
                    StmtKind::Continue => Fallback::Continue,
                    _ => return no("a fallback block"),
                }
            }
        };
        Ok(Some(e(ExprKind::Fallback(Box::new(x), Box::new(fb)))))
    }
}

/// `a, b = v` as elaboration expands it: temporaries for `v._0`, `v._1`, ..., then one
/// binding per name. Returns the names and how many statements that took.
fn destructured(v: &str, ss: &[y::Stmt]) -> (Vec<String>, usize) {
    let field_of = |s: &y::Stmt| match &s.kind {
        y::StmtKind::Let {
            pat: y::Pat::Bind(t),
            rhs: y::Rhs::Proj(p),
            ..
        } if generated(t)
            && p.root == v
            && matches!(p.projs.as_slice(), [y::Proj::Field(f, _)] if f.starts_with('_')) =>
        {
            Some(t.clone())
        }
        _ => None,
    };
    let temps: Vec<String> = ss.iter().map_while(field_of).collect();
    let n = temps.len();
    let names: Option<Vec<String>> = temps
        .iter()
        .zip(ss.get(n..2 * n).unwrap_or_default())
        .map(|(t, s)| match &s.kind {
            y::StmtKind::Let {
                pat: y::Pat::Bind(b),
                rhs: y::Rhs::Atom(y::Atom::Name(a, _)),
                ..
            } if a == t => Some(b.clone()),
            _ => None,
        })
        .collect();
    match names {
        Some(ns) if n > 0 && ns.len() == n => (ns, 2 * n),
        _ => (Vec::new(), 0),
    }
}

/// `x[len(x) + -k]` is how yan-elab normalizes `x[-k]`.
fn negative_index(x: &Expr, i: Expr) -> Expr {
    if let ExprKind::Binary("+", a, b) = &i.kind
        && let ExprKind::Call(f, args) = &a.kind
        && matches!(&f.kind, ExprKind::Name(n) if n == "len")
        && let [
            Arg {
                kind: ArgKind::Expr(y),
                name: None,
                ..
            },
        ] = args.as_slice()
        && same(x, y)
        && matches!(&b.kind, ExprKind::Lit(Lit::Int(k)) if k.starts_with('-'))
    {
        return (**b).clone();
    }
    i
}

/// Two simple expressions, a literal or a name, are the same up to spans.
fn same(a: &Expr, b: &Expr) -> bool {
    match (&a.kind, &b.kind) {
        (ExprKind::Lit(x), ExprKind::Lit(y)) => x == y,
        (ExprKind::Name(x), ExprKind::Name(y)) => x == y,
        (ExprKind::List(x), ExprKind::List(y)) => x.is_empty() && y.is_empty(),
        (ExprKind::Dict(x), ExprKind::Dict(y)) => x.is_empty() && y.is_empty(),
        (ExprKind::Attr(x, f, _), ExprKind::Attr(y, g, _)) => f == g && same(x, y),
        (ExprKind::Call(f, xs), ExprKind::Call(g, ys)) => {
            xs.is_empty() && ys.is_empty() && same(f, g)
        }
        (ExprKind::Call(f, xs), _) if xs.is_empty() => same(f, b),
        (_, ExprKind::Call(g, ys)) if ys.is_empty() => same(a, g),
        _ => false,
    }
}

/// Tuple targets bound straight from `t`'s fields: `let a = t._0`, `let b = t._1`, ...
fn destructured_from(t: &str, ss: &[y::Stmt]) -> (Vec<String>, usize) {
    let names: Vec<String> = ss
        .iter()
        .enumerate()
        .map_while(|(i, s)| match &s.kind {
            y::StmtKind::Let {
                pat: y::Pat::Bind(n),
                rhs: y::Rhs::Proj(p),
                ..
            } if p.root == t
                && matches!(p.projs.as_slice(), [y::Proj::Field(f, _)] if *f == format!("_{i}")) =>
            {
                Some(n.clone())
            }
            _ => None,
        })
        .collect();
    let k = names.len();
    (names, k)
}

/// Yan Agent writes a type on an assignment where inference needs it: an empty collection.
/// Drops the annotation of an empty `[]` or `{}` whose type R808 infers from the first later
/// statement that fixes it, as yan-elab's `learn` does.
fn drop_inferred(ss: &mut [Stmt], ret: Option<&Type>, types: &HashMap<String, y::Type>) {
    for i in 0..ss.len() {
        let (head, rest) = ss.split_at_mut(i + 1);
        if let StmtKind::Assign {
            target: Expr {
                kind: ExprKind::Name(n),
                ..
            },
            ty: t @ Some(_),
            value,
        } = &mut head[i].kind
            && matches!(&value.kind, ExprKind::List(xs) if xs.is_empty())
                | matches!(&value.kind, ExprKind::Dict(kvs) if kvs.is_empty())
            && first_fix(n, rest, ret, types).flatten().as_ref() == t.as_ref()
        {
            *t = None;
        }
        for b in blocks(&mut head[i].kind) {
            drop_inferred(b, ret, types);
        }
    }
}

fn blocks(k: &mut StmtKind) -> Vec<&mut Vec<Stmt>> {
    match k {
        StmtKind::If { arms, els } => arms
            .iter_mut()
            .map(|(_, b)| &mut b.stmts)
            .chain(els.as_mut().map(|b| &mut b.stmts))
            .collect(),
        StmtKind::While { body, .. } | StmtKind::For { body, .. } => vec![body],
        StmtKind::Match { cases, .. } => cases.iter_mut().map(|c| &mut c.body.stmts).collect(),
        StmtKind::With { body, .. } => vec![&mut body.stmts],
        _ => Vec::new(),
    }
}

/// The type the first statement of `ss` (in order, nested ones after their parent) that fixes
/// `n` gives it: `Some(None)` when that type is not known here.
fn first_fix(
    n: &str,
    ss: &[Stmt],
    ret: Option<&Type>,
    types: &HashMap<String, y::Type>,
) -> Option<Option<Type>> {
    let is_n = |e: &Expr| matches!(&e.kind, ExprKind::Name(x) if x == n);
    let probe = |e: &Expr| -> Option<Type> {
        let core = |p: &str| Type::Named {
            path: vec![p.into()],
            args: Vec::new(),
        };
        match &e.kind {
            ExprKind::Lit(Lit::Int(_)) => Some(core("i64")),
            ExprKind::Lit(Lit::Float(_)) => Some(core("f64")),
            ExprKind::Lit(Lit::Bool(_)) => Some(core("bool")),
            ExprKind::Lit(Lit::Str { prefix, .. }) if prefix.is_empty() => Some(core("Str")),
            ExprKind::FStr { .. } => Some(core("Str")),
            ExprKind::Name(x) => types.get(x).and_then(|t| ty(t).ok()),
            _ => None,
        }
    };
    let coll = |p: &str, ts: Vec<Option<Type>>| -> Option<Type> {
        Some(Type::Named {
            path: vec![p.into()],
            args: ts
                .into_iter()
                .map(|t| t.map(TArg::Type))
                .collect::<Option<_>>()?,
        })
    };
    for s in ss {
        match &s.kind {
            StmtKind::Expr(Expr {
                kind: ExprKind::Call(f, args),
                ..
            }) if let ExprKind::Attr(r, m, _) = &f.kind
                && is_n(r)
                && let [
                    Arg {
                        kind: ArgKind::Expr(a),
                        ..
                    },
                ] = args.as_slice() =>
            {
                match m.as_str() {
                    "append" => return Some(coll("List", vec![probe(a)])),
                    "extend" => {
                        let t = probe(a);
                        if t.is_none()
                            || matches!(&t, Some(Type::Named { path, .. }) if path == &["List"])
                        {
                            return Some(t);
                        }
                    }
                    _ => {}
                }
            }
            StmtKind::Assign {
                target:
                    Expr {
                        kind: ExprKind::Index(d, k),
                        ..
                    },
                ty: None,
                value,
            } if is_n(d) => return Some(coll("Map", vec![probe(k), probe(value)])),
            StmtKind::Return(Some(e)) if is_n(e) => return Some(ret.cloned()),
            _ => {}
        }
        let mut k = s.kind.clone();
        for b in blocks(&mut k) {
            if let Some(t) = first_fix(n, b, ret, types) {
                return Some(t);
            }
        }
    }
    None
}

fn annotation(value: &Expr, t: &y::Type) -> R<Option<Type>> {
    let empty = match &value.kind {
        ExprKind::List(xs) | ExprKind::Set(xs) => xs.is_empty(),
        ExprKind::Dict(kvs) => kvs.is_empty(),
        ExprKind::Lit(Lit::None) => true,
        ExprKind::Call(f, args) => {
            args.is_empty()
                && matches!(&f.kind, ExprKind::Name(n) if COLLECTIONS.contains(&n.as_str()))
        }
        // A literal of a fixed-width type keeps its annotation.
        ExprKind::Lit(Lit::Int(_) | Lit::Float(_)) | ExprKind::Unary("-", _) => matches!(
            t,
            y::Type::Named(n) if n.path.len() == 1
                && !matches!(n.path[0].as_str(), "i64" | "f64" | "BigInt")
                && yan_std::intrinsics::INTS.iter().chain(&yan_std::intrinsics::FLOATS)
                    .any(|s| *s == n.path[0])
        ),
        _ => false,
    };
    empty.then(|| ty(t)).transpose()
}

fn aug(op: &str) -> &'static str {
    match op {
        "+" => "+=",
        "-" => "-=",
        "*" => "*=",
        "//" => "//=",
        "%" => "%=",
        "/" => "/=",
        "&" => "&=",
        "|" => "|=",
        _ => "^=",
    }
}

const COLLECTIONS: [&str; 5] = ["set", "deque", "heap", "sorted_dict", "sorted_set"];

const COMPARE: [&str; 6] = ["==", "!=", "<", "<=", ">", ">="];

/// The operator an intrinsic implements, from its name (`add_checked_i64` is `+`).
fn binary(path: &str) -> Option<&'static str> {
    let (op, _ty) = path.rsplit_once('_')?;
    Some(match op.strip_suffix("_checked").unwrap_or(op) {
        "add" => "+",
        "sub" => "-",
        "mul" => "*",
        "floordiv" => "//",
        "mod" => "%",
        "div" => "/",
        "and" => "&",
        "or" => "|",
        "xor" => "^",
        "shl" => "<<",
        "shr" => ">>",
        "eq" => "==",
        "ne" => "!=",
        "lt" => "<",
        "le" => "<=",
        "gt" => ">",
        "ge" => ">=",
        _ => return None,
    })
}

/// The operator that elaborates to `std.<module>.<f>` on time values (17.3).
fn time_operator(module: &str, f: &str) -> Option<&'static str> {
    if module == "time" {
        return match f {
            "zoned_add" | "zoned_add_period" => Some("+"),
            "zoned_sub" => Some("-"),
            _ => None,
        };
    }
    module.strip_prefix("time.")?;
    Some(match f {
        "add" => "+",
        "sub" | "since" => "-",
        "mul" => "*",
        "div" | "ratio" => "/",
        "floordiv" | "quotient" => "//",
        "rem" => "%",
        _ => return None,
    })
}

/// The std types Yan Agent names for their module's functions, as in `Duration.seconds(5)`.
pub const TYPE_STATICS: [(&str, &str); 10] = [
    ("bytes", "encoding"),
    ("bytearray", "encoding.bytearray"),
    ("Duration", "time.duration.of"),
    ("Instant", "time.instant"),
    ("PlainDate", "time.date"),
    ("PlainTime", "time.plain_time"),
    ("PlainDateTime", "time.plain_date_time"),
    ("Period", "time.period"),
    ("TimeZone", "time.zone"),
    ("ZonedDateTime", "time"),
];

/// The type that spells `module.f` when it is not a method: `encoding` keeps its codecs.
fn static_type(module: &str, f: &str) -> Option<&'static str> {
    if module == "encoding" && f != "from_list" {
        return None;
    }
    TYPE_STATICS
        .iter()
        .find(|(_, m)| *m == module)
        .map(|(t, _)| *t)
}

const TYPE_MODULES: [&str; 11] = [
    "list",
    "encoding",
    "encoding.bytearray",
    "dict",
    "set",
    "text",
    "iter",
    "collections.deque",
    "collections.heap",
    "collections.sorted_dict",
    "collections.sorted_set",
];

/// The std module that declares a type, for method calls (17.2).
fn type_module(t: &y::Type) -> Option<&'static str> {
    let y::Type::Named(n) = t else {
        return None;
    };
    Some(match n.path.as_slice() {
        [p] => match p.as_str() {
            "List" => "list",
            "Map" => "dict",
            "Set" => "set",
            "Str" | "char" => "text",
            "Iter" => "iter",
            "Deque" => "collections.deque",
            "Heap" => "collections.heap",
            "SortedMap" => "collections.sorted_dict",
            "SortedSet" => "collections.sorted_set",
            "Regex" => "regex",
            _ => return None,
        },
        _ => return None,
    })
}

fn pat(p: &y::Pat) -> R<Pat> {
    Ok(match p {
        y::Pat::Wild => Pat::Wild,
        // A nullary variant is a class pattern, so it does not read as a capture.
        y::Pat::Bind(n) => Pat::Path {
            path: vec![n.clone()],
            args: variant(n).then(Vec::new),
        },
        y::Pat::Ctor { path, fields: None } => Pat::Path {
            path: path.clone(),
            args: path.last().is_some_and(|n| variant(n)).then(Vec::new),
        },
        y::Pat::Ctor {
            path,
            fields: Some((fs, false)),
        } => Pat::Path {
            path: path.clone(),
            // Yan Agent's canonical class patterns are positional.
            args: Some(
                fs.iter()
                    .map(|(_, p)| Ok((None, pat(p)?)))
                    .collect::<R<_>>()?,
            ),
        },
        y::Pat::Tuple(ps) => Pat::Tuple(ps.iter().map(pat).collect::<R<_>>()?),
        _ => return no("this pattern"),
    })
}

fn kind_name(k: &y::StmtKind) -> &'static str {
    use y::StmtKind as K;
    match k {
        K::Yield(_) => "yield",
        K::Become(_) => "become",
        K::With { .. } => "`with dyn`",
        K::Par { .. } => "par",
        K::ComptimeFor { .. } | K::ComptimeIf { .. } => "comptime",
        K::Unsafe(_) => "unsafe",
        K::Asm { .. } => "asm",
        K::Reuse { .. } => "reuse",
        K::Jump { .. } => "jump",
        _ => "this statement",
    }
}

fn call(f: Expr, args: Vec<Arg>) -> Expr {
    e(ExprKind::Call(Box::new(f), args))
}

fn attr(x: Expr, f: &str) -> Expr {
    e(ExprKind::Attr(Box::new(x), f.to_owned(), AT))
}

fn arg(x: Expr) -> Arg {
    Arg {
        name: None,
        kind: ArgKind::Expr(x),
        span: AT,
    }
}

fn path_expr(p: &[y::Name]) -> Expr {
    let mut x = name(&p[0]);
    for n in &p[1..] {
        x = attr(x, n);
    }
    x
}

/// Whether the statements after `n`'s builds start a `for` over `n` as yan-elab lowers it:
/// the index from 0, then the length.
fn for_over(n: &str, ss: &[y::Stmt]) -> bool {
    use y::StmtKind as K;
    let starts = |w: &[y::Stmt]| {
        matches!(
            (&w[0].kind, &w[1].kind, &w[2].kind),
            (
                K::Let { pat: y::Pat::Bind(i), rhs: y::Rhs::Atom(y::Atom::Lit(y::Literal::Int(z), _)), .. },
                K::Let { rhs: y::Rhs::Call(c), .. },
                K::Loop { .. },
            ) if generated(i) && z == "0"
                && c.path.join(".") == "std.list.len"
                && matches!(c.args.as_slice(), [y::Arg::Value(_, y::Atom::Name(x, _), _)] if x == n)
        )
    };
    for (k, w) in ss.windows(3).enumerate() {
        if starts(w) {
            return true;
        }
        let built = match &ss[k].kind {
            K::Let {
                pat: y::Pat::Bind(t),
                ..
            } => generated(t),
            K::Rhs(y::Rhs::Call(c)) => BUILDERS.contains(&c.path.join(".").as_str()),
            _ => false,
        };
        if !built {
            return false;
        }
    }
    false
}

fn count_builds(n: &str, ss: &[y::Stmt]) -> usize {
    ss.iter()
        .take_while(|s| match &s.kind {
            y::StmtKind::Let { pat: y::Pat::Bind(t), .. } => generated(t),
            y::StmtKind::Rhs(y::Rhs::Call(c)) => BUILDERS.contains(&c.path.join(".").as_str()),
            _ => false,
        })
        .filter(|s| {
            matches!(&s.kind, y::StmtKind::Rhs(y::Rhs::Call(c))
                if matches!(c.args.first(), Some(y::Arg::Value(y::Conv::Inout, y::Atom::Name(t, _), _)) if t == n))
        })
        .count()
}

fn count_atom(a: &y::Atom, uses: &mut HashMap<String, usize>) {
    if let y::Atom::Name(n, _) = a {
        *uses.entry(n.clone()).or_default() += 1;
    }
}

fn count_name(n: &str, uses: &mut HashMap<String, usize>) {
    *uses.entry(n.to_owned()).or_default() += 1;
}

fn count_call(c: &y::Call, uses: &mut HashMap<String, usize>) {
    for a in &c.args {
        if let y::Arg::Value(_, a, _) = a {
            count_atom(a, uses);
        }
    }
}

fn count_place(p: &y::Place, uses: &mut HashMap<String, usize>) {
    count_name(&p.root, uses);
    for pr in &p.projs {
        if let y::Proj::Index(a) = pr {
            count_atom(a, uses);
        }
    }
}

fn count_rhs(r: &y::Rhs, uses: &mut HashMap<String, usize>) {
    match r {
        y::Rhs::Atom(a) => count_atom(a, uses),
        y::Rhs::Call(c) | y::Rhs::Try(c) | y::Rhs::Propagate { call: c, .. } => count_call(c, uses),
        y::Rhs::Ctor(y::Ctor::Named { fields, .. }) => {
            fields.iter().for_each(|(_, a)| count_atom(a, uses))
        }
        y::Rhs::Ctor(y::Ctor::Tuple(xs)) => xs.iter().for_each(|a| count_atom(a, uses)),
        y::Rhs::Proj(p) | y::Rhs::Copy(p) | y::Rhs::Move(p) => count_place(p, uses),
        // A closure's body is counted when it is raised.
        y::Rhs::Closure(c) => c.captures.iter().for_each(|(_, n, _)| count_name(n, uses)),
    }
}

fn count_expr(x: &y::Expr, uses: &mut HashMap<String, usize>) {
    match x {
        y::Expr::Atom(a) => count_atom(a, uses),
        y::Expr::Call(c) => count_call(c, uses),
    }
}

fn count_stmts(ss: &[y::Stmt], uses: &mut HashMap<String, usize>) {
    use y::StmtKind as K;
    for s in ss {
        match &s.kind {
            K::Let { rhs, .. } | K::Rhs(rhs) => count_rhs(rhs, uses),
            K::Assign { place, rhs, .. } => {
                count_place(place, uses);
                count_rhs(rhs, uses);
            }
            K::If { cond, then, els } | K::ComptimeIf { cond, then, els } => {
                count_atom(cond, uses);
                count_stmts(&then.stmts, uses);
                if let Some(b) = els {
                    count_stmts(&b.stmts, uses);
                }
            }
            K::Match { scrutinee, arms } => {
                count_atom(scrutinee, uses);
                arms.iter().for_each(|(_, b)| count_stmts(&b.stmts, uses));
            }
            K::Loop {
                invariants,
                decreases,
                body,
                ..
            } => {
                invariants
                    .iter()
                    .chain(decreases)
                    .for_each(|x| count_expr(x, uses));
                count_stmts(&body.stmts, uses);
            }
            K::Break { value: Some(a), .. } | K::Ret(a) | K::Fail(a) | K::Yield(a) => {
                count_atom(a, uses)
            }
            K::Jp { body, .. } => count_stmts(&body.stmts, uses),
            K::Jump { args, .. } => args.iter().for_each(|a| count_atom(a, uses)),
            K::Become(c) => count_call(c, uses),
            K::With {
                handler: at, body, ..
            }
            | K::Deadline { at, body } => {
                count_atom(at, uses);
                count_stmts(&body.stmts, uses);
            }
            K::Par { blocks, .. } => blocks.iter().for_each(|b| count_stmts(&b.stmts, uses)),
            K::ComptimeFor { iter, body, .. } => {
                count_atom(iter, uses);
                count_stmts(&body.stmts, uses);
            }
            K::Assert(x) => count_expr(x, uses),
            K::Unsafe(b) => count_stmts(&b.stmts, uses),
            K::Drop(n) => count_name(n, uses),
            K::Reuse { name, .. } => count_name(name, uses),
            K::Break { value: None, .. } | K::Continue { .. } | K::Unreachable | K::Asm { .. } => {}
        }
    }
}
