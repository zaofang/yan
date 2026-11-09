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
use yan_surface::{Attach, Inserted, Mark, Trivia, TriviaTable};

use crate::*;

mod inner;
mod loops;
mod marked;
mod std_call;
mod stmt;

pub(crate) const AT: Span = Span {
    line: 0,
    col: 0,
    end_line: 0,
    end_col: 0,
};

/// A Yan Core form raise does not handle yet.
#[derive(Debug, PartialEq)]
pub struct Unraised(pub String, pub Option<Span>);

impl Unraised {
    /// Locates the failure at `span` unless a nearer form already did.
    fn at(mut self, span: Span) -> Self {
        self.1 = self.1.or(Some(span));
        self
    }
}

impl std::fmt::Display for Unraised {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.1 {
            Some(s) => write!(f, "{} at {}:{}", self.0, s.line, s.col),
            None => f.write_str(&self.0),
        }
    }
}

pub(crate) type R<T> = Result<T, Unraised>;

pub(crate) fn no<T>(what: impl Into<String>) -> R<T> {
    Err(Unraised(what.into(), None))
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
    /// Whether a std function returns a value, so calling it as a statement discards one (E312).
    fn returns_value(&self, path: &str) -> bool;
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
    let mut attrs = Vec::new();
    let mut doc = None;
    for (it, h) in m.items.iter().zip(yan_core::hash::hash_module(m)) {
        let marks = Marks::new(trivia, h.identity.to_string(), it, &fns);
        let lemmas = marks.root(|m| match m {
            Mark::Lemma(t) => Some(t.clone()),
            _ => None,
        });
        if let Some(mut i) = item(it, std, &marks).map_err(|e| e.at(it.span))? {
            for text in lemmas {
                items.push(Item {
                    decorators: Vec::new(),
                    is_pub: false,
                    kind: ItemKind::Verbatim(text),
                    span: AT,
                    name_span: AT,
                });
            }
            let (module_doc, item_doc) = marks.docs();
            doc = doc.or(module_doc);
            set_doc(&mut i, item_doc);
            items.push(i);
        }
        if !marks
            .root(|m| matches!(m, Mark::Prove).then_some(()))
            .is_empty()
            && attrs.is_empty()
        {
            attrs.push(Decorator {
                path: vec!["prove".into()],
                args: None,
            });
        }
        consumed.extend(marks.finish());
    }
    Ok(Raised {
        module: Module {
            header: Vec::new(),
            attrs,
            doc,
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

thread_local! {
    /// Atoms that were `out` in a `post`: they are not uses of the local they name, so
    /// the use counts leave them out.
    static OUT_ATOMS: RefCell<HashSet<*const y::Atom>> = RefCell::default();
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
    lits: HashMap<*const y::Literal, Vec<u32>>,
    atoms: HashMap<*const y::Atom, Vec<u32>>,
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
        let mut lits = HashMap::new();
        let mut atoms = HashMap::new();
        yan_core::path::walk(it, &mut |p, n| match n {
            yan_core::path::Node::Stmt(s) => {
                stmts.insert(s as *const y::Stmt, p.to_vec());
            }
            yan_core::path::Node::Call(c) => {
                paths.insert(c as *const y::Call, p.to_vec());
            }
            yan_core::path::Node::Field(c, k) => {
                fields.insert((c as *const y::Ctor, k), p.to_vec());
                if let y::Ctor::Named { fields, .. } = c {
                    atoms.insert(&fields[k].1 as *const y::Atom, p.to_vec());
                }
            }
            yan_core::path::Node::Arg(y::Arg::Value(_, a, _)) => {
                atoms.insert(a as *const y::Atom, p.to_vec());
            }
            yan_core::path::Node::Literal(l, _) => {
                lits.insert(l as *const y::Literal, p.to_vec());
            }
            _ => {}
        });
        let sourced = trivia.has_names(&key);
        OUT_ATOMS.with(|o| {
            *o.borrow_mut() = atoms
                .iter()
                .filter(|(_, p)| {
                    sourced
                        && trivia
                            .at(&key, p)
                            .iter()
                            .any(|t| matches!(t, Trivia::Mark(Mark::Out)))
                })
                .map(|(a, _)| *a)
                .collect();
        });
        Marks {
            sourced: trivia.has_names(&key),
            trivia,
            key,
            paths,
            fields,
            stmts,
            lits,
            atoms,
            fns,
            seen: RefCell::default(),
            rejected: RefCell::default(),
        }
    }

    /// The marks at the item's root that `pick` accepts, consumed. None when the item has no
    /// marks to go by (D3).
    fn root<T>(&self, pick: impl Fn(&Mark) -> Option<T>) -> Vec<T> {
        if !self.sourced {
            return Vec::new();
        }
        let found: Vec<T> = self
            .trivia
            .at(&self.key, &[])
            .iter()
            .filter_map(|t| match t {
                Trivia::Mark(m) => pick(m),
                _ => None,
            })
            .collect();
        if !found.is_empty() {
            self.seen.borrow_mut().insert(Vec::new());
        }
        found
    }

    /// The docstring trivia at the item's root: the module's, which rides on the first item
    /// (leading), and the item's own (dangling).
    fn docs(&self) -> (Option<String>, Option<String>) {
        let at = |want: Attach| {
            self.trivia.at(&self.key, &[]).iter().find_map(|t| match t {
                Trivia::Comment { text, attach } if *attach == want => Some(text.clone()),
                _ => None,
            })
        };
        if !self.sourced {
            return (None, None);
        }
        (at(Attach::Leading), at(Attach::Dangling))
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

    /// Whether atom `a` was `out` in a `post`, consumed.
    fn is_out(&self, a: &y::Atom) -> bool {
        let Some(p) = self
            .atoms
            .get(&(a as *const y::Atom))
            .filter(|_| self.sourced)
        else {
            return false;
        };
        let here = self
            .trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Mark(Mark::Out)));
        if here {
            self.seen.borrow_mut().insert(p.clone());
        }
        here
    }

    /// Whether `s` carries `FromPost`, consumed.
    fn marked_post(&self, s: &y::Stmt) -> bool {
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
            .any(|t| matches!(t, Trivia::Mark(Mark::FromPost)));
        if here {
            self.seen.borrow_mut().insert(p.clone());
        }
        here
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

    /// The spelling of integer literal `l` (canonical text `canon`) as the source wrote it,
    /// consumed when it denotes the same value. A note that disagrees with the value is rejected
    /// and the decimal form prints (16.5).
    fn spelled(&self, l: &y::Literal, canon: &str) -> Option<String> {
        let p = self
            .lits
            .get(&(l as *const y::Literal))
            .filter(|_| self.sourced)?;
        let text = self.trivia.at(&self.key, p).iter().find_map(|t| match t {
            Trivia::Literal(s) => Some(s),
            _ => None,
        })?;
        if int_value(text).is_some_and(|v| v.to_string() == canon) {
            self.seen.borrow_mut().insert(p.clone());
            Some(text.clone())
        } else {
            self.rejected.borrow_mut().insert(p.clone());
            None
        }
    }

    /// Whether string literal `l` was written raw, consumed when the note matches its value.
    fn raw(&self, l: &y::Literal, value: &str) -> bool {
        let Some(p) = self
            .lits
            .get(&(l as *const y::Literal))
            .filter(|_| self.sourced)
        else {
            return false;
        };
        let raw = self
            .trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Literal(s) if s.strip_prefix('r') == Some(value)));
        if raw {
            self.seen.borrow_mut().insert(p.clone());
        } else if self
            .trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Literal(_)))
        {
            self.rejected.borrow_mut().insert(p.clone());
        }
        raw
    }

    /// The path of statement `s` when it carries mark `want`.
    fn stmt_mark(&self, s: &y::Stmt, want: &Mark) -> Option<Vec<u32>> {
        let p = self
            .stmts
            .get(&(s as *const y::Stmt))
            .filter(|_| self.sourced)?;
        self.trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Mark(m) if m == want))
            .then(|| p.clone())
    }

    /// Whether the node at `p` carries `want`, consumed.
    fn has_mark(&self, p: &[u32], want: &Mark) -> bool {
        let hit = self.sourced
            && self
                .trivia
                .at(&self.key, p)
                .iter()
                .any(|t| matches!(t, Trivia::Mark(m) if m == want));
        if hit {
            self.seen.borrow_mut().insert(p.to_vec());
        }
        hit
    }

    /// `t` as written when the node at `p` is marked `View`: the owned list prints as `View[T]`.
    fn view(&self, p: &[u32], t: &y::Type, printed: Type) -> Type {
        match (&printed, t) {
            (Type::Named { path, args }, y::Type::Named(n))
                if n.path == ["List"] && args.len() == 1 && self.has_mark(p, &Mark::View) =>
            {
                Type::Named {
                    path: vec!["View".into()],
                    args: args.clone(),
                }
            }
            _ => printed,
        }
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

    /// The path of call `c` when it carries mark `want`.
    fn call_mark(&self, c: &y::Call, want: &Mark) -> Option<Vec<u32>> {
        let p = self
            .paths
            .get(&(c as *const y::Call))
            .filter(|_| self.sourced)?;
        self.trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Mark(m) if m == want))
            .then(|| p.clone())
    }

    /// The path of call `c` and the name of its `Schema` mark.
    fn call_schema(&self, c: &y::Call) -> Option<(Vec<u32>, String)> {
        let p = self
            .paths
            .get(&(c as *const y::Call))
            .filter(|_| self.sourced)?;
        self.trivia.at(&self.key, p).iter().find_map(|t| match t {
            Trivia::Mark(Mark::Schema(n)) => Some((p.clone(), n.clone())),
            _ => None,
        })
    }

    /// The origin elaboration recorded at call `c`.
    fn call_origin(&self, c: &y::Call) -> Option<Inserted> {
        let p = self
            .paths
            .get(&(c as *const y::Call))
            .filter(|_| self.sourced)?;
        self.trivia.origin(&self.key, p)
    }

    /// The path of literal `l` when it carries mark `want`.
    fn lit_mark(&self, l: &y::Literal, want: &Mark) -> Option<Vec<u32>> {
        let p = self
            .lits
            .get(&(l as *const y::Literal))
            .filter(|_| self.sourced)?;
        self.trivia
            .at(&self.key, p)
            .iter()
            .any(|t| matches!(t, Trivia::Mark(m) if m == want))
            .then(|| p.clone())
    }

    /// The `Qualified` mark path of each value argument of `c`, in order.
    fn qualified(&self, c: &y::Call) -> Vec<Option<Vec<u32>>> {
        let at = self.paths.get(&(c as *const y::Call));
        let mut out = Vec::new();
        for (k, a) in c.args.iter().enumerate() {
            if !matches!(a, y::Arg::Value(..)) {
                continue;
            }
            out.push(at.filter(|_| self.sourced).and_then(|p| {
                let mut p = p.clone();
                p.push(k as u32);
                self.trivia
                    .at(&self.key, &p)
                    .iter()
                    .any(|t| matches!(t, Trivia::Mark(Mark::Qualified)))
                    .then_some(p)
            }));
        }
        out
    }

    fn accept(&self, p: Vec<u32>) {
        self.seen.borrow_mut().insert(p);
    }

    fn refuse(&self, p: Vec<u32>) {
        self.rejected.borrow_mut().insert(p);
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

/// A spelling of `trivia` that `raised` did not consume: its item, and what it is with its path.
#[derive(Clone, Debug, PartialEq)]
pub struct Unused {
    pub item: String,
    pub what: String,
}

/// The spellings of `trivia` that `raised` did not consume.
pub fn unconsumed(trivia: &TriviaTable, raised: &Raised) -> Vec<Unused> {
    trivia
        .entries()
        .filter_map(|(i, p, t)| {
            let used = raised.consumed.iter().any(|(ci, cp)| ci == i && cp == p);
            let what = match t {
                Trivia::Literal(_) if !used => format!("literal spelling at {i} {p:?}"),
                Trivia::Mark(mk) if !used => format!("{mk:?} at {i} {p:?}"),
                _ => return None,
            };
            Some(Unused {
                item: i.to_owned(),
                what,
            })
        })
        .collect()
}

/// The value of an integer literal written in decimal, hex, octal or binary, with underscores.
fn int_value(s: &str) -> Option<i128> {
    let s = s.replace('_', "");
    let (radix, digits) = match s.get(..2) {
        Some("0x" | "0X") => (16, &s[2..]),
        Some("0o" | "0O") => (8, &s[2..]),
        Some("0b" | "0B") => (2, &s[2..]),
        _ => (10, &s[..]),
    };
    i128::from_str_radix(digits, radix).ok()
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

fn set_doc(i: &mut Item, doc: Option<String>) {
    match &mut i.kind {
        ItemKind::Func(f) => {
            if let Some(b) = &mut f.body {
                b.doc = doc;
            }
        }
        ItemKind::Record(r) => r.doc = doc,
        _ => {}
    }
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
                                decorators: Vec::new(),
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
        y::ItemKind::Extern(x) => (false, ItemKind::Extern(extern_block(x)?)),
        y::ItemKind::Inst(_) => return no("instances, which only optimized Yan Core has"),
    };
    let traits = marks.root(|m| match m {
        Mark::Derived(t) => Some(t.clone()),
        _ => None,
    });
    let derive = (!traits.is_empty()).then(|| Decorator {
        path: vec!["derive".into()],
        args: Some(
            traits
                .iter()
                .map(|t| Arg {
                    name: None,
                    kind: ArgKind::Expr(name(t)),
                    span: AT,
                })
                .collect(),
        ),
    });
    let decorators = derive
        .into_iter()
        .map(Ok)
        .chain(
            it.attrs
                .iter()
                .filter(|a| a.name != "doc")
                .map(|a| match a.args {
                    None => Ok(Decorator {
                        path: vec![a.name.clone()],
                        args: None,
                    }),
                    Some(_) => no("attributes with arguments"),
                }),
        )
        .collect::<R<_>>()?;
    Ok(Some(Item {
        decorators,
        is_pub,
        kind,
        span: it.span,
        name_span: it.span,
    }))
}

fn extern_block(x: &y::Extern) -> R<Extern> {
    let items = x
        .items
        .iter()
        .map(|i| {
            Ok(match i {
                y::XItem::Fn {
                    attrs,
                    safe,
                    name,
                    params,
                    variadic,
                    ret,
                    uses_none,
                    nonblocking,
                    ..
                } => XItem::Fn {
                    attrs: attrs
                        .iter()
                        .map(|a| {
                            Ok(Decorator {
                                path: vec![a.name.clone()],
                                args: a.args.as_deref().map(crate::claim::args).transpose()?,
                            })
                        })
                        .collect::<R<_>>()?,
                    safe: *safe,
                    name: name.clone(),
                    name_span: AT,
                    params: params
                        .iter()
                        .map(|p| {
                            Ok(match p {
                                y::XParam::Value { name, ty: t } => XParam {
                                    name: name.clone(),
                                    callback: None,
                                    ty: ty(t)?,
                                    ty_span: AT,
                                },
                                y::XParam::Callback {
                                    name,
                                    escaping,
                                    ty: f,
                                } => XParam {
                                    name: name.clone(),
                                    callback: Some(if *escaping {
                                        Callback::Escaping
                                    } else {
                                        Callback::Scoped
                                    }),
                                    ty: ty(&y::Type::Fn(f.clone()))?,
                                    ty_span: AT,
                                },
                            })
                        })
                        .collect::<R<_>>()?,
                    variadic: *variadic,
                    ret: match ty(ret)? {
                        Type::None => None,
                        t => Some((t, AT)),
                    },
                    uses_none: *uses_none,
                    nonblocking: *nonblocking,
                },
                y::XItem::Static { safe, name, ty: t } => XItem::Static {
                    safe: *safe,
                    name: name.clone(),
                    ty: ty(t)?,
                    ty_span: AT,
                },
                y::XItem::Type { name, drop } => XItem::Type {
                    name: name.clone(),
                    drop: drop.last().cloned().unwrap_or_default(),
                },
            })
        })
        .collect::<R<_>>()?;
    Ok(Extern {
        abi: x.abi.clone(),
        lib: x.lib.clone(),
        link: x.link.clone(),
        items,
    })
}

fn field(f: &y::Field) -> R<Field> {
    if f.default.is_some() {
        return no("field defaults");
    }
    let decorators = f
        .attrs
        .iter()
        .map(|a| match a.args {
            None => Ok(Decorator {
                path: vec![a.name.clone()],
                args: None,
            }),
            Some(_) => no("attributes with arguments"),
        })
        .collect::<R<_>>()?;
    Ok(Field {
        decorators,
        name: f.name.clone(),
        ty: ty(&f.ty)?,
        ty_span: Span::default(),
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
    for (k, p) in f.params.iter().enumerate() {
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
                    ty: marks.view(&[0, k as u32], t, ty(t)?),
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
    // Ghost parameters were erased (R870).
    for (k, text) in marks.root(|m| match m {
        Mark::GhostParam(k, t) => Some((*k, t.clone())),
        _ => None,
    }) {
        if let Some((n, t)) = text.strip_prefix("ghost ").and_then(|r| r.split_once(':')) {
            params.insert(
                k.min(params.len()),
                Param::Value {
                    conv: Some(Conv::Ghost),
                    name: n.trim().to_owned(),
                    span: AT,
                    ty: Type::Named {
                        path: vec![t.trim().to_owned()],
                        args: Vec::new(),
                    },
                    default: None,
                },
            );
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
            ty: if unit {
                None
            } else {
                Some(marks.view(&[], &f.ret, ty(&f.ret)?))
            },
            fail,
            from: f.from.clone(),
        })
    });
    let mut contracts = Vec::new();
    // The printed name stands for the `ex` text as written: Yan Agent prints a name verbatim.
    for text in marks.root(|m| match m {
        Mark::Example(t) => Some(t.clone()),
        _ => None,
    }) {
        contracts.push(Contract::Ex {
            expr: name(&text),
            with: Vec::new(),
        });
    }
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
            y::Contract::Cost { steps, alloc } => match marks
                .root(|m| match m {
                    Mark::Cost(s, a) => Some((s.clone(), a.clone())),
                    _ => None,
                })
                .pop()
            {
                Some((s, a)) => Contract::Cost {
                    steps: s.as_deref().map(name),
                    alloc: a.as_deref().map(name),
                },
                None if steps
                    .iter()
                    .chain(alloc)
                    .any(|x| matches!(x, y::Expr::Call(c) if c.path[0].starts_with("__cost_"))) =>
                {
                    return no("a `cost` bound without its source text");
                }
                None => Contract::Cost {
                    steps: steps.as_ref().map(|x| b.pure(x)).transpose()?,
                    alloc: alloc.as_ref().map(|x| b.pure(x)).transpose()?,
                },
            },
        });
    }
    let examples = contracts.len();
    for (k, text) in marks.root(|m| match m {
        Mark::GhostContract(k, t) => Some((*k, t.clone())),
        _ => None,
    }) {
        let c = match text.split_once(' ') {
            Some(("pre", t)) => Contract::Pre(name(t)),
            Some(("post", t)) => Contract::Post(name(t)),
            _ => continue,
        };
        contracts.insert((examples + k).min(contracts.len()), c);
    }
    let body = match &f.body {
        None => None,
        Some(blk) => {
            let mut b = Fold::new(std, marks, locals, &blk.stmts);
            let mut stmts = b.stmts(&blk.stmts)?;
            contracts.extend(b.pre.drain(..).map(Contract::Pre));
            contracts.extend(b.post.drain(..).map(Contract::Post));
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
            for (k, text) in marks.root(|m| match m {
                Mark::GhostStmt(k, t) => Some((*k, t.clone())),
                _ => None,
            }) {
                stmts.insert(k.min(stmts.len()), stmt(StmtKind::Expr(name(&text))));
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
/// The `math` constant whose value `text` is.
fn std_const(text: &str) -> Option<&'static str> {
    match text.replace('_', "").parse::<f64>().ok()? {
        v if v == std::f64::consts::PI => Some("pi"),
        v if v == std::f64::consts::E => Some("e"),
        v if v == f64::INFINITY => Some("inf"),
        _ => None,
    }
}

/// Whether std module `module` declares a sum type with variant `v`.
fn std_sum_variant(module: &str, v: &str) -> bool {
    yan_std::SOURCES
        .iter()
        .filter(|(m, _)| *m == module)
        .flat_map(|(_, src)| src.lines())
        .any(|l| {
            l.starts_with("pub type ")
                && (l.contains(&format!("= {v}(")) || l.contains(&format!("| {v}(")))
        })
}

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
    /// The next tuple raised was written in parentheses (R868).
    paren_tuple: bool,
    pending: HashMap<String, Pending>,
    /// Pending temporaries bound by a std call without effects, which may go unused.
    pure: std::collections::HashSet<String>,
    /// Labels of the enclosing loops, innermost last.
    loops: Vec<String>,
    /// Leading `assert`s elaborated from `pre`, moved back to the contract list.
    pre: Vec<Expr>,
    /// The `post`s of the `assert`s elaborated from them, once each.
    post: Vec<Expr>,
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
            paren_tuple: false,
            pending: HashMap::new(),
            pure: Default::default(),
            loops: Vec::new(),
            pre: Vec::new(),
            post: Vec::new(),
        }
    }

    fn uses(&self, n: &str) -> usize {
        self.uses.get(n).copied().unwrap_or(0)
    }

    fn atom(&mut self, a: &y::Atom) -> R<Expr> {
        if self.marks.is_out(a) {
            return Ok(name("out"));
        }
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
            y::Atom::Lit(l @ y::Literal::Float(s), _)
                if let Some(p) = self.marks.lit_mark(l, &Mark::Const) =>
            {
                match std_const(s) {
                    Some(k) => {
                        self.marks.accept(p);
                        attr(name("math"), k)
                    }
                    None => {
                        self.marks.refuse(p);
                        e(ExprKind::Lit(Lit::Float(s.clone())))
                    }
                }
            }
            y::Atom::Lit(l @ y::Literal::Str(s), _)
                if let Some(p) = self.marks.lit_mark(l, &Mark::Lenient("N36".into())) =>
            {
                let mut cs = s.chars();
                match (cs.next(), cs.next()) {
                    (Some(c), None) => {
                        self.marks.accept(p);
                        e(ExprKind::Lit(Lit::Char(c)))
                    }
                    _ => {
                        self.marks.refuse(p);
                        e(ExprKind::Lit(Lit::Str {
                            prefix: String::new(),
                            value: s.clone(),
                        }))
                    }
                }
            }
            y::Atom::Lit(l, _) => e(ExprKind::Lit(match l {
                y::Literal::Int(s) => {
                    Lit::Int(self.marks.spelled(l, s).unwrap_or_else(|| s.clone()))
                }
                y::Literal::Float(s) => Lit::Float(s.clone()),
                y::Literal::Str(s) => Lit::Str {
                    prefix: if self.marks.raw(l, s) { "r" } else { "" }.into(),
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
            y::Rhs::Propagate {
                call, via: None, ..
            } => Ok(e(ExprKind::Try(Box::new(self.call(call)?)))),
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
            y::Ctor::Tuple(xs) => {
                let paren = std::mem::take(&mut self.paren_tuple);
                let mut t = e(ExprKind::Tuple(
                    xs.iter().map(|a| self.atom(a)).collect::<R<_>>()?,
                ));
                t.paren = paren;
                Ok(t)
            }
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
                            // `Ok`, `Err` and `Some` take their one payload positionally only.
                            let positional = matches!(mark, Some((Mark::Positional, _)))
                                || matches!(path.as_slice(), [s] if matches!(s.as_str(), "Ok" | "Err" | "Some"));
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

    /// The printed expression bound to `a`, if it is a pending temporary.
    fn peek(&self, a: &y::Atom) -> Option<&Expr> {
        let y::Atom::Name(n, _) = a else { return None };
        match self.pending.get(n) {
            Some(Pending::Expr(x)) => Some(x),
            _ => None,
        }
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
                rhs,
                ..
            } => match rhs {
                y::Rhs::Atom(y::Atom::Name(a, _)) if a == t => Some(b.clone()),
                y::Rhs::Move(p) if p.root == *t && p.projs.is_empty() => Some(b.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    match names {
        Some(ns) if n > 0 && ns.len() == n => (ns, 2 * n),
        _ => (Vec::new(), 0),
    }
}

/// `for x in t` over a tuple `t`, as yan-elab expands it: a list built from copies of each
/// field of the tuple temporary `t`. Returns the list's temporary and the statements it took.
fn tuple_iterated(t: &str, ss: &[y::Stmt]) -> Option<(String, usize)> {
    use y::StmtKind as K;
    let [
        K::Let {
            pat: y::Pat::Bind(list),
            rhs: y::Rhs::Call(new),
            ..
        },
        ..,
    ] = ss.iter().map(|s| &s.kind).collect::<Vec<_>>().as_slice()
    else {
        return None;
    };
    if !generated(list) || new.path.join(".") != "std.list.new" {
        return None;
    }
    let mut k = 1;
    let mut field = 0;
    while let [
        K::Let {
            pat: y::Pat::Bind(x),
            rhs: y::Rhs::Copy(p),
            ..
        },
        K::Rhs(y::Rhs::Call(add)),
        ..,
    ] = &ss[k..].iter().map(|s| &s.kind).collect::<Vec<_>>()[..]
    {
        let copies_field = p.root == t
            && matches!(p.projs.as_slice(), [y::Proj::Field(f, _)] if *f == format!("_{field}"));
        let appends = add.path.join(".") == "std.list.append"
            && matches!(add.args.as_slice(), [
                y::Arg::Value(y::Conv::Inout, y::Atom::Name(l, _), _),
                y::Arg::Value(_, y::Atom::Name(v, _), _),
            ] if l == list && v == x);
        if !(copies_field && appends) {
            break;
        }
        k += 2;
        field += 1;
    }
    (field > 0).then(|| (list.clone(), k))
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

/// `for k in m` iterates the keys of a dict, which elaboration spells `m.keys()`.
fn keys_iterated(x: Expr) -> Expr {
    match x.kind {
        ExprKind::Call(f, args) if args.is_empty() => match f.kind {
            ExprKind::Attr(m, k, _) if k == "keys" => *m,
            kind => Expr {
                kind: ExprKind::Call(Box::new(Expr { kind, ..*f }), args),
                ..x
            },
        },
        _ => x,
    }
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
    // A `Result` binding of a call needs its type, or the failure is unhandled (E315).
    let reified = matches!(&value.kind, ExprKind::Call(..))
        && matches!(t, y::Type::Named(n) if n.path == ["Result"]);
    let empty = reified
        || match &value.kind {
            ExprKind::List(xs) | ExprKind::Set(xs) => xs.is_empty(),
            ExprKind::Dict(kvs) => kvs.is_empty(),
            ExprKind::Lit(Lit::None) => true,
            ExprKind::Call(f, args) => {
                args.is_empty()
                    && (matches!(&f.kind, ExprKind::Name(n) if COLLECTIONS.contains(&n.as_str()))
                        // The element type only the annotation names.
                        || matches!(&f.kind, ExprKind::Attr(m, e, _) if e == "empty"
                            && matches!(&m.kind, ExprKind::Name(n) if n == "iter")))
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

/// A pattern with a backslash is written raw.
fn raw_if_backslash(x: &mut Expr) {
    if let ExprKind::Lit(Lit::Str { prefix, value }) = &mut x.kind
        && value.contains('\\')
        && !value.ends_with('\\')
        && !value.contains(['"', '\n', '\r'])
    {
        *prefix = "r".into();
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
    if let y::Atom::Name(n, _) = a
        && !OUT_ATOMS.with(|o| o.borrow().contains(&(a as *const y::Atom)))
    {
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

/// The comprehension that `acc = <empty>` followed by a `for` loop that fills `acc` stands for.
fn comprehension(acc: &str, raised: &[Stmt]) -> Option<Expr> {
    let [first, second] = raised else { return None };
    let StmtKind::Assign { target, value, .. } = &first.kind else {
        return None;
    };
    let StmtKind::For {
        label: None,
        target: vars,
        iter,
        contracts,
        body,
    } = &second.kind
    else {
        return None;
    };
    let is_acc = |x: &Expr| matches!(&x.kind, ExprKind::Name(t) if t == acc);
    let kind = match &value.kind {
        ExprKind::List(xs) if xs.is_empty() => CompKind::List,
        ExprKind::Dict(kvs) if kvs.is_empty() => CompKind::Dict,
        ExprKind::Call(f, xs)
            if xs.is_empty() && matches!(&f.kind, ExprKind::Name(n) if n == "set") =>
        {
            CompKind::Set
        }
        _ => return None,
    };
    if !is_acc(target) || !contracts.is_empty() {
        return None;
    }
    let (cond, inner) = match body.as_slice() {
        [
            Stmt {
                kind: StmtKind::If { arms, els: None },
                ..
            },
        ] if let [(c, b)] = arms.as_slice() => (Some(c), b.stmts.as_slice()),
        b => (None, b),
    };
    let [one] = inner else { return None };
    let (elem, val) = match (&one.kind, kind) {
        (
            StmtKind::Expr(Expr {
                kind: ExprKind::Call(f, args),
                ..
            }),
            CompKind::List | CompKind::Set,
        ) if let ExprKind::Attr(recv, m, _) = &f.kind
            && is_acc(recv)
            && m == if kind == CompKind::List {
                "append"
            } else {
                "add"
            }
            && let [
                Arg {
                    name: None,
                    kind: ArgKind::Expr(a),
                    ..
                },
            ] = args.as_slice() =>
        {
            (a.clone(), None)
        }
        (
            StmtKind::Assign {
                target:
                    Expr {
                        kind: ExprKind::Index(recv, k),
                        ..
                    },
                value,
                ..
            },
            CompKind::Dict,
        ) if is_acc(recv) => ((**k).clone(), Some(Box::new(value.clone()))),
        _ => return None,
    };
    Some(e(ExprKind::Comp {
        kind,
        par: None,
        elem: Box::new(elem),
        value: val,
        target: vars.clone(),
        iter: Box::new(iter.clone()),
        cond: cond.cloned().map(Box::new),
    }))
}
