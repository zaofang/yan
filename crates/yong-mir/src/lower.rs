//! Checked Yong to MIR.

use std::collections::HashMap;

use yong_core::ast as y;
use yong_std::registry::{self, id};

use crate::*;

type R<T> = Result<T, String>;

fn unsupported<T>(what: impl std::fmt::Display) -> R<T> {
    Err(format!("{what} in native code"))
}

const I64: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

type Fields = Vec<(String, Shape)>;

/// A Yong type as scalar slots.
#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Scalar(Ty),
    Str,
    List(Box<Shape>),
    Rec(String, Vec<(String, Shape)>),
    Tuple(Vec<Shape>),
    /// A tag slot, then each variant's fields in order.
    Sum(String, Vec<(String, Vec<(String, Shape)>)>),
}

impl Shape {
    fn slots(&self, out: &mut Vec<Ty>) {
        match self {
            Shape::Scalar(t) => out.push(*t),
            Shape::Str | Shape::List(_) => out.push(Ty::Ptr),
            Shape::Rec(_, fs) => fs.iter().for_each(|(_, s)| s.slots(out)),
            Shape::Tuple(ss) => ss.iter().for_each(|s| s.slots(out)),
            Shape::Sum(_, vs) => {
                out.push(I64);
                vs.iter()
                    .flat_map(|(_, fs)| fs)
                    .for_each(|(_, s)| s.slots(out));
            }
        }
    }

    fn tys(&self) -> Vec<Ty> {
        let mut v = Vec::new();
        self.slots(&mut v);
        v
    }

    fn width(&self) -> usize {
        self.tys().len()
    }

    fn has_list(&self) -> bool {
        match self {
            Shape::List(_) => true,
            Shape::Scalar(_) | Shape::Str => false,
            Shape::Rec(_, fs) => fs.iter().any(|(_, s)| s.has_list()),
            Shape::Tuple(ss) => ss.iter().any(Shape::has_list),
            Shape::Sum(_, vs) => vs.iter().flat_map(|(_, fs)| fs).any(|(_, s)| s.has_list()),
        }
    }

    /// The fields of a record or tuple, by name, with their first slot.
    fn fields(&self) -> Option<Vec<(String, usize, &Shape)>> {
        let named: Vec<(String, &Shape)> = match self {
            Shape::Rec(_, fs) => fs.iter().map(|(n, s)| (n.clone(), s)).collect(),
            Shape::Tuple(ss) => ss
                .iter()
                .enumerate()
                .map(|(i, s)| (i.to_string(), s))
                .collect(),
            _ => return None,
        };
        let mut at = 0;
        Some(
            named
                .into_iter()
                .map(|(n, s)| {
                    let r = (n, at, s);
                    at += s.width();
                    r
                })
                .collect(),
        )
    }

    /// A sum's variant `name`: its tag, first slot and fields.
    fn variant(&self, name: &str) -> Option<(i128, usize, &Fields)> {
        let Shape::Sum(_, vs) = self else {
            return None;
        };
        let mut at = 1;
        for (tag, (n, fs)) in vs.iter().enumerate() {
            if n == name {
                return Some((tag as i128, at, fs));
            }
            at += fs.iter().map(|(_, s)| s.width()).sum::<usize>();
        }
        None
    }
}

fn zero(t: Ty) -> Operand {
    match t {
        Ty::Int(_) | Ty::Ptr => Operand::Int(0),
        Ty::F64 => Operand::F64(0.0),
        Ty::Bool => Operand::Bool(false),
        Ty::Unit => Operand::Unit,
    }
}

struct Types<'m> {
    recs: HashMap<&'m str, &'m y::Record>,
    sums: HashMap<&'m str, &'m y::Sum>,
}

impl Types<'_> {
    fn shape(&self, t: &y::Type) -> R<Shape> {
        self.shape_in(t, &mut Vec::new())
    }

    fn shape_in(&self, t: &y::Type, stack: &mut Vec<String>) -> R<Shape> {
        match t {
            y::Type::Tuple(ts) => Ok(Shape::Tuple(
                ts.iter()
                    .map(|t| self.shape_in(t, stack))
                    .collect::<R<_>>()?,
            )),
            y::Type::Named(n) if n.path.len() == 1 => {
                let name = n.path[0].as_str();
                match (name, n.args.as_slice()) {
                    ("Str", []) => return Ok(Shape::Str),
                    ("List", [y::TArg::Type(e)]) => {
                        return Ok(Shape::List(Box::new(self.shape_in(e, stack)?)));
                    }
                    ("Option", [y::TArg::Type(t)]) => {
                        let t = self.shape_in(t, stack)?;
                        return Ok(Shape::Sum(
                            "Option".into(),
                            vec![
                                ("Some".into(), vec![("value".into(), t)]),
                                ("None".into(), vec![]),
                            ],
                        ));
                    }
                    ("Result", [y::TArg::Type(t), y::TArg::Type(e)]) => {
                        return Ok(result(self.shape_in(t, stack)?, self.shape_in(e, stack)?));
                    }
                    (_, []) => {}
                    _ => return unsupported(format!("type `{name}[..]`")),
                }
                if let Some(t) = scalar_ty(name) {
                    return Ok(Shape::Scalar(t));
                }
                if stack.iter().any(|s| s == name) {
                    return unsupported(format!("recursive type `{name}`"));
                }
                stack.push(name.to_owned());
                let fields = |fs: &[y::Field], stack: &mut Vec<String>| {
                    fs.iter()
                        .map(|f| Ok((f.name.clone(), self.shape_in(&f.ty, stack)?)))
                        .collect::<R<Vec<_>>>()
                };
                let s = if let Some(r) = self.recs.get(name).filter(|r| r.tparams.is_empty()) {
                    Shape::Rec(name.to_owned(), fields(&r.fields, stack)?)
                } else if let Some(s) = self.sums.get(name).filter(|s| s.tparams.is_empty()) {
                    let vs = s
                        .variants
                        .iter()
                        .map(|v| {
                            let fs = v.fields.as_deref().unwrap_or(&[]);
                            Ok((v.name.clone(), fields(fs, stack)?))
                        })
                        .collect::<R<_>>()?;
                    Shape::Sum(name.to_owned(), vs)
                } else {
                    return unsupported(format!("type `{name}`"));
                };
                stack.pop();
                Ok(s)
            }
            _ => unsupported("a compound type"),
        }
    }
}

fn result(ok: Shape, err: Shape) -> Shape {
    Shape::Sum(
        "Result".into(),
        vec![
            ("Ok".into(), vec![("value".into(), ok)]),
            ("Err".into(), vec![("error".into(), err)]),
        ],
    )
}

struct Sig {
    params: Vec<(y::Conv, Shape)>,
    /// The declared result.
    ok: Shape,
    /// What the function returns: `ok`, or `Result[ok, E]` for `fail E`.
    ret: Shape,
    fails: bool,
}

impl Sig {
    fn inout(&self) -> Vec<Ty> {
        self.params
            .iter()
            .filter(|(c, _)| *c == y::Conv::Inout)
            .flat_map(|(_, s)| s.tys())
            .collect()
    }
}

/// The MIR of `m`, or what it uses that native code cannot run yet.
pub fn lower(m: &y::Module) -> R<Program> {
    let mut types = Types {
        recs: HashMap::new(),
        sums: HashMap::new(),
    };
    let mut defs = Vec::new();
    for i in &m.items {
        match &i.kind {
            y::ItemKind::Fn(f) if f.body.is_some() => defs.push(f),
            y::ItemKind::Fn(_) => {}
            y::ItemKind::Record(r) => {
                types.recs.insert(r.path.last().expect("named"), r);
            }
            y::ItemKind::Sum(s) => {
                types.sums.insert(s.path.last().expect("named"), s);
            }
            _ => return unsupported(format!("item at line {}", i.span.line)),
        }
    }
    let ids: HashMap<String, FuncId> = defs
        .iter()
        .enumerate()
        .map(|(i, f)| (f.path.join("."), i as FuncId))
        .collect();
    let mut prog = Program {
        funcs: Vec::new(),
        main: *ids.get("main").ok_or("no `fn main` in the module")?,
        main_fails: false,
        strings: Vec::new(),
    };
    let sigs: Vec<Sig> = defs
        .iter()
        .map(|f| {
            if !f.tparams.is_empty() {
                return unsupported("a generic function");
            }
            Ok(Sig {
                params: params(f)?
                    .into_iter()
                    .map(|(_, c, t)| Ok((c, types.shape(t)?)))
                    .collect::<R<_>>()?,
                ok: types.shape(&f.ret)?,
                ret: match &f.fail {
                    Some(e) => result(types.shape(&f.ret)?, types.shape(e)?),
                    None => types.shape(&f.ret)?,
                },
                fails: f.fail.is_some(),
            })
        })
        .collect::<R<_>>()?;
    prog.main_fails = sigs[prog.main as usize].fails;
    for (i, f) in defs.iter().enumerate() {
        let func = Fn::new(&ids, &sigs, &types, &mut prog.strings, i).func(f)?;
        prog.funcs.push(func);
    }
    Ok(prog)
}

fn params(f: &y::FnDef) -> R<Vec<(&str, y::Conv, &y::Type)>> {
    f.params
        .iter()
        .filter_map(|p| match p {
            y::Param::Value { comptime: true, .. } => Some(unsupported("a comptime parameter")),
            y::Param::Value { name, ty, conv, .. } => Some(Ok((name.as_str(), *conv, ty))),
            y::Param::Cap { .. } => None,
        })
        .collect()
}

fn scalar_ty(name: &str) -> Option<Ty> {
    let int = |bits, signed| Some(Ty::Int(IntTy { bits, signed }));
    match name {
        "i8" => int(8, true),
        "i16" => int(16, true),
        "i32" => int(32, true),
        "i64" => int(64, true),
        "u8" => int(8, false),
        "u16" => int(16, false),
        "u32" => int(32, false),
        "u64" | "Addr" => int(64, false),
        "f64" => Some(Ty::F64),
        "bool" => Some(Ty::Bool),
        "Unit" => Some(Ty::Unit),
        _ => None,
    }
}

/// `<op>_<ty>` as an op and its operand type.
fn scalar(path: &str) -> Option<(ScalarOp, Option<Ty>)> {
    use ScalarOp::*;
    if path == "exclusive_check" {
        return Some((Exclusive, None));
    }
    let (op, ty) = path.rsplit_once('_')?;
    let op = match op {
        "add_checked" => AddChecked,
        "sub_checked" => SubChecked,
        "mul_checked" => MulChecked,
        "floordiv" => FloorDiv,
        "mod" => Mod,
        "div" => Div,
        "and" => And,
        "or" => Or,
        "xor" => Xor,
        "not" => Not,
        "add" => Add,
        "sub" => Sub,
        "mul" => Mul,
        "eq" => Eq,
        "ne" => Ne,
        "lt" => Lt,
        "le" => Le,
        "gt" => Gt,
        "ge" => Ge,
        _ => return None,
    };
    Some((op, Some(scalar_ty(ty)?)))
}

fn parse_int(s: &str) -> R<i128> {
    let t = s.replace('_', "");
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t.as_str()),
    };
    let (radix, digits) = match t.get(..2) {
        Some("0x") => (16, &t[2..]),
        Some("0o") => (8, &t[2..]),
        Some("0b") => (2, &t[2..]),
        _ => (10, t),
    };
    let v = i128::from_str_radix(digits, radix).map_err(|_| format!("integer literal `{s}`"))?;
    Ok(if neg { -v } else { v })
}

#[derive(Clone)]
struct Var {
    locals: Vec<Local>,
    shape: Shape,
}

/// Where a place's slots are.
enum At {
    Reg(Vec<Local>),
    /// Slots from `word` on in the block at `ptr`.
    Mem(Operand, u32),
}

enum Scope {
    Loop {
        label: String,
        head: BlockId,
        exit: BlockId,
    },
    /// A block that declares join points. A jump from its statement `at` sets `cont` to `at`,
    /// and the end of a join point's body resumes after that statement.
    Jps {
        jps: Vec<(String, BlockId, Vec<Local>)>,
        cont: Local,
        at: Option<i128>,
    },
}

struct Fn<'a> {
    ids: &'a HashMap<String, FuncId>,
    sigs: &'a [Sig],
    types: &'a Types<'a>,
    strings: &'a mut Vec<String>,
    me: usize,
    locals: Vec<Ty>,
    names: HashMap<String, Var>,
    /// The `inout` parameters' slots, returned with every result.
    inout: Vec<Local>,
    blocks: Vec<Block>,
    done: Vec<bool>,
    cur: BlockId,
    scopes: Vec<Scope>,
}

impl<'a> Fn<'a> {
    fn new(
        ids: &'a HashMap<String, FuncId>,
        sigs: &'a [Sig],
        types: &'a Types<'a>,
        strings: &'a mut Vec<String>,
        me: usize,
    ) -> Self {
        Fn {
            ids,
            sigs,
            types,
            strings,
            me,
            locals: Vec::new(),
            names: HashMap::new(),
            inout: Vec::new(),
            blocks: Vec::new(),
            done: Vec::new(),
            cur: 0,
            scopes: Vec::new(),
        }
    }

    fn func(mut self, f: &y::FnDef) -> R<Func> {
        let sig = &self.sigs[self.me];
        for ((name, ..), (conv, shape)) in params(f)?.iter().zip(&sig.params) {
            let v = self.var(shape.clone());
            if *conv == y::Conv::Inout {
                self.inout.extend(&v.locals);
            }
            self.names.insert((*name).to_owned(), v);
        }
        let n_params = self.locals.len() as u32;
        let mut ret = sig.ret.tys();
        ret.extend(sig.inout());
        self.cur = self.block_new();
        for c in &f.contracts {
            if let y::Contract::Requires(e) = c {
                let t = self.expr(e)?;
                self.check(t, "contract");
            }
        }
        self.block(f.body.as_ref().expect("filtered on body"))?;
        self.finish(Term::Panic("unreachable"));
        Ok(Func {
            name: f.path.join("."),
            params: n_params,
            ret,
            locals: self.locals,
            blocks: self.blocks,
        })
    }

    fn local(&mut self, t: Ty) -> Local {
        self.locals.push(t);
        self.locals.len() as Local - 1
    }

    fn var(&mut self, shape: Shape) -> Var {
        let locals = shape.tys().into_iter().map(|t| self.local(t)).collect();
        Var { locals, shape }
    }

    fn block_new(&mut self) -> BlockId {
        self.blocks.push(Block {
            insts: Vec::new(),
            term: Term::Panic("unreachable"),
        });
        self.done.push(false);
        self.blocks.len() as BlockId - 1
    }

    fn emit(&mut self, i: Inst) {
        self.blocks[self.cur as usize].insts.push(i);
    }

    /// End the current block with `t` unless it already ended.
    fn finish(&mut self, t: Term) {
        let c = self.cur as usize;
        if !self.done[c] {
            self.blocks[c].term = t;
            self.done[c] = true;
        }
    }

    /// End the current block with `t` and continue in a new, unreachable one.
    fn end(&mut self, t: Term) {
        self.finish(t);
        self.cur = self.block_new();
    }

    fn goto(&mut self, b: BlockId) {
        self.finish(Term::Jump(b));
        self.cur = b;
    }

    fn branch(&mut self, cond: Operand) -> (BlockId, BlockId) {
        let (t, e) = (self.block_new(), self.block_new());
        self.finish(Term::Branch {
            cond,
            then: t,
            els: e,
        });
        (t, e)
    }

    fn check(&mut self, cond: Operand, kind: &'static str) {
        let (ok, bad) = self.branch(cond);
        self.cur = bad;
        self.finish(Term::Panic(kind));
        self.cur = ok;
    }

    fn copy_to(&mut self, dst: &[Local], src: Vec<Operand>) {
        for (d, s) in dst.iter().zip(src) {
            self.emit(Inst::Copy { dst: *d, src: s });
        }
    }

    fn rt(&mut self, f: RtFn, ty: Ty, args: Vec<Operand>) -> Operand {
        let d = self.local(ty);
        self.emit(Inst::Rt {
            dst: Some(d),
            f,
            args,
        });
        Operand::Local(d)
    }

    /// Return `v` with the final `inout` slots.
    fn ret(&mut self, mut v: Vec<Operand>) {
        v.extend(self.inout.iter().map(|l| Operand::Local(*l)));
        self.end(Term::Return(v));
    }

    /// The slots of sum `s` holding variant `name` with fields `v`.
    fn variant_value(s: &Shape, name: &str, v: Vec<Operand>) -> Vec<Operand> {
        let (tag, at, _) = s.variant(name).expect("a variant of the sum");
        let mut out: Vec<Operand> = s.tys().into_iter().map(zero).collect();
        out[0] = Operand::Int(tag);
        let w = v.len();
        out.splice(at..at + w, v);
        out
    }

    fn lit(&mut self, s: &str) -> Operand {
        self.strings.push(s.to_owned());
        Operand::Str(self.strings.len() as u32 - 1)
    }

    fn scalar_op(&mut self, op: ScalarOp, ty: Ty, args: Vec<Operand>) -> Operand {
        let name = match op {
            ScalarOp::Eq => "eq_i64",
            ScalarOp::Ge => "ge_i64",
            ScalarOp::Le => "le_i64",
            ScalarOp::AddChecked => "add_checked_i64",
            _ => unreachable!("only used for these"),
        };
        let d = self.local(if matches!(op, ScalarOp::AddChecked) {
            I64
        } else {
            Ty::Bool
        });
        self.emit(Inst::Scalar {
            dst: Some(d),
            id: registry::id(name).expect("scalar ops are registered"),
            op,
            ty,
            args,
        });
        Operand::Local(d)
    }

    fn name(&self, n: &str) -> R<&Var> {
        self.names
            .get(n)
            .ok_or_else(|| format!("unbound name `{n}`"))
    }

    fn ops(v: &Var) -> Vec<Operand> {
        v.locals.iter().map(|l| Operand::Local(*l)).collect()
    }

    fn literal(&mut self, l: &y::Literal) -> R<(Operand, Shape)> {
        Ok(match l {
            y::Literal::Int(s) => (Operand::Int(parse_int(s)?), Shape::Scalar(I64)),
            y::Literal::Float(s) => (
                Operand::F64(
                    s.replace('_', "")
                        .parse()
                        .map_err(|_| format!("float `{s}`"))?,
                ),
                Shape::Scalar(Ty::F64),
            ),
            y::Literal::Bool(b) => (Operand::Bool(*b), Shape::Scalar(Ty::Bool)),
            y::Literal::Unit => (Operand::Unit, Shape::Scalar(Ty::Unit)),
            y::Literal::Str(s) => (self.lit(s), Shape::Str),
            y::Literal::Char(_) => return unsupported("a char"),
        })
    }

    /// An atom whose type is its own: a local or a literal.
    fn atom_any(&mut self, a: &y::Atom) -> R<(Vec<Operand>, Shape)> {
        match a {
            y::Atom::Name(n, _) => {
                let v = self.name(n)?;
                Ok((Self::ops(v), v.shape.clone()))
            }
            y::Atom::Lit(l, _) => {
                let (o, s) = self.literal(l)?;
                Ok((vec![o], s))
            }
        }
    }

    /// An atom of type `want`: also a fieldless variant of a sum.
    fn atom(&mut self, a: &y::Atom, want: &Shape) -> R<Vec<Operand>> {
        if let y::Atom::Name(n, _) = a
            && !self.names.contains_key(n)
            && let Some((tag, ..)) = want.variant(n)
        {
            let mut v: Vec<Operand> = want.tys().into_iter().map(zero).collect();
            v[0] = Operand::Int(tag);
            return Ok(v);
        }
        if let (y::Atom::Lit(y::Literal::Int(s), _), Shape::Scalar(Ty::F64)) = (a, want) {
            return Ok(vec![Operand::F64(parse_int(s)? as f64)]);
        }
        Ok(self.atom_any(a)?.0)
    }

    fn scalar1(&mut self, a: &y::Atom) -> R<Operand> {
        let (mut v, s) = self.atom_any(a)?;
        match (s, v.len()) {
            (Shape::Scalar(_) | Shape::Str | Shape::List(_), 1) => Ok(v.remove(0)),
            _ => unsupported("an aggregate operand"),
        }
    }

    fn expr(&mut self, e: &y::Expr) -> R<Operand> {
        match e {
            y::Expr::Atom(a) => self.scalar1(a),
            y::Expr::Call(c) => {
                let t = self.local(Ty::Bool);
                self.call(
                    c,
                    Some(Var {
                        locals: vec![t],
                        shape: Shape::Scalar(Ty::Bool),
                    }),
                )?;
                Ok(Operand::Local(t))
            }
        }
    }

    fn value_args(c: &y::Call) -> R<Vec<(y::Conv, &y::Atom)>> {
        c.args
            .iter()
            .filter_map(|a| match a {
                y::Arg::Value(conv, a, _) => Some(Ok((*conv, a))),
                y::Arg::Cap(..) => None,
            })
            .collect()
    }

    fn targ(&self, c: &y::Call) -> R<Option<Shape>> {
        match c.targs.as_slice() {
            [y::TArg::Type(t)] => Ok(Some(self.types.shape(t)?)),
            _ => Ok(None),
        }
    }

    fn user_args(&mut self, func: FuncId, c: &y::Call) -> R<(Vec<Operand>, Vec<Local>)> {
        let args = Self::value_args(c)?;
        let sig = &self.sigs[func as usize];
        if sig.params.len() != args.len() {
            return unsupported(format!(
                "call of `{}` with these arguments",
                c.path.join(".")
            ));
        }
        let params: Vec<(y::Conv, Shape)> = sig.params.clone();
        let mut ops = Vec::new();
        let mut back = Vec::new();
        for ((conv, a), (_, shape)) in args.into_iter().zip(&params) {
            if conv == y::Conv::Inout {
                let y::Atom::Name(n, _) = a else {
                    return unsupported("an `inout` literal");
                };
                back.extend(self.name(n)?.locals.clone());
            }
            ops.extend(self.atom(a, shape)?);
        }
        Ok((ops, back))
    }

    fn call(&mut self, c: &y::Call, dst: Option<Var>) -> R<()> {
        let name = c.path.join(".");
        if let Some(&func) = self.ids.get(&name) {
            let (args, back) = self.user_args(func, c)?;
            let mut d = match dst {
                Some(v) => v.locals,
                None => self.var(self.sigs[func as usize].ret.clone()).locals,
            };
            d.extend(back);
            self.emit(Inst::Call { dst: d, func, args });
            return Ok(());
        }
        let Some(id) = registry::id(&name) else {
            return unsupported(format!("call of `{name}`"));
        };
        let args = Self::value_args(c)?;
        let dst1 = dst.as_ref().and_then(|v| v.locals.first().copied());
        if let Some((op, ty)) = scalar(&name) {
            let args = args
                .iter()
                .map(|(_, a)| self.scalar1(a))
                .collect::<R<Vec<_>>>()?;
            let ty = match ty {
                Some(t) => t,
                None => match args.first() {
                    Some(Operand::Local(l)) => self.locals[*l as usize],
                    _ => I64,
                },
            };
            self.emit(Inst::Scalar {
                dst: dst1,
                id,
                op,
                ty,
                args,
            });
            return Ok(());
        }
        let set = |me: &mut Self, v: Operand| {
            if let Some(d) = dst1 {
                me.emit(Inst::Copy { dst: d, src: v });
            }
        };
        match (id, args.as_slice()) {
            (id::STD_IO_PRINT, [(_, a)]) => {
                let s = self.scalar1(a)?;
                self.emit(Inst::Rt {
                    dst: None,
                    f: RtFn::Print,
                    args: vec![s],
                });
            }
            (id::STD_TEXT_CONCAT, [(_, a), (_, b)]) => {
                let (a, b) = (self.scalar1(a)?, self.scalar1(b)?);
                let r = self.rt(RtFn::Concat, Ty::Ptr, vec![a, b]);
                set(self, r);
            }
            (id::STD_FMT_SHOW, [(_, a)]) => {
                let (v, s) = match self.targ(c)? {
                    Some(s) => (self.atom(a, &s)?, s),
                    None => self.atom_any(a)?,
                };
                let r = self.show(&v, &s, false)?;
                set(self, r);
            }
            (id::STD_LIST_NEW, []) => {
                let Some(e) = self.targ(c)? else {
                    return unsupported("`std.list.new` without its element type");
                };
                let r = self.rt(
                    RtFn::ListNew,
                    Ty::Ptr,
                    vec![Operand::Int(e.width() as i128)],
                );
                set(self, r);
            }
            (id::STD_LIST_PUSH | id::STD_LIST_APPEND, [(_, y::Atom::Name(l, _)), (_, x)]) => {
                let list = self.name(l)?.clone();
                let Shape::List(e) = &list.shape else {
                    return unsupported(format!("call of `{name}`"));
                };
                let v = self.atom(x, e)?;
                let at = self.rt(RtFn::ListPush, Ty::Ptr, Self::ops(&list));
                self.write(&At::Mem(at, 0), v);
            }
            (id::STD_LIST_LEN, [(_, a)]) => {
                let l = self.scalar1(a)?;
                let r = self.rt(RtFn::ListLen, I64, vec![l]);
                set(self, r);
            }
            _ => return unsupported(format!("call of `{name}`")),
        }
        Ok(())
    }

    fn concat(&mut self, parts: Vec<Operand>) -> Operand {
        let mut it = parts.into_iter();
        let first = it.next().expect("at least one part");
        it.fold(first, |acc, p| self.rt(RtFn::Concat, Ty::Ptr, vec![acc, p]))
    }

    /// CPython's `str` of a value, or its `repr` inside a container (SQ20).
    fn show(&mut self, v: &[Operand], s: &Shape, nested: bool) -> R<Operand> {
        Ok(match s {
            Shape::Scalar(Ty::Int(t)) => {
                let f = if t.signed {
                    RtFn::ShowI64
                } else {
                    RtFn::ShowU64
                };
                self.rt(f, Ty::Ptr, vec![v[0].clone()])
            }
            Shape::Scalar(Ty::F64) => self.rt(RtFn::ShowF64, Ty::Ptr, vec![v[0].clone()]),
            Shape::Scalar(Ty::Bool) => self.rt(RtFn::ShowBool, Ty::Ptr, vec![v[0].clone()]),
            Shape::Scalar(Ty::Unit) => self.lit("None"),
            Shape::Scalar(Ty::Ptr) => unreachable!("not a Yong type"),
            Shape::Str if nested => self.rt(RtFn::Repr, Ty::Ptr, vec![v[0].clone()]),
            Shape::Str => v[0].clone(),
            Shape::Rec(name, fs) => self.show_fields(name, v, fs)?,
            Shape::Tuple(ss) => {
                let mut parts = vec![self.lit("(")];
                let mut at = 0;
                for (i, e) in ss.iter().enumerate() {
                    if i > 0 {
                        parts.push(self.lit(", "));
                    }
                    let w = e.width();
                    parts.push(self.show(&v[at..at + w], e, true)?);
                    at += w;
                }
                parts.push(self.lit(if ss.len() == 1 { ",)" } else { ")" }));
                self.concat(parts)
            }
            Shape::Sum(name, vs) => {
                let option = name == "Option";
                let out = self.local(Ty::Ptr);
                let join = self.block_new();
                for (n, fs) in vs {
                    let (tag, at, _) = s.variant(n).expect("its own variant");
                    let w: usize = fs.iter().map(|(_, s)| s.width()).sum();
                    let is =
                        self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(tag)]);
                    let (t, e) = self.branch(is);
                    self.cur = t;
                    // `Option` shows as its value or `None`.
                    let r = match (option, fs.first()) {
                        (true, Some((_, inner))) => self.show(&v[at..at + w], inner, nested)?,
                        (true, None) => self.lit("None"),
                        _ => self.show_fields(n, &v[at..at + w], fs)?,
                    };
                    self.emit(Inst::Copy { dst: out, src: r });
                    self.finish(Term::Jump(join));
                    self.cur = e;
                }
                self.finish(Term::Panic("unreachable"));
                self.cur = join;
                Operand::Local(out)
            }
            Shape::List(e) => {
                let list = v[0].clone();
                let out = self.local(Ty::Ptr);
                let open = self.lit("[");
                self.emit(Inst::Copy {
                    dst: out,
                    src: open,
                });
                let n = self.rt(RtFn::ListLen, I64, vec![list.clone()]);
                let i = self.local(I64);
                self.emit(Inst::Copy {
                    dst: i,
                    src: Operand::Int(0),
                });
                let head = self.block_new();
                self.goto(head);
                let done = self.scalar_op(ScalarOp::Ge, I64, vec![Operand::Local(i), n]);
                let (exit, body) = self.branch(done);
                self.cur = body;
                let first =
                    self.scalar_op(ScalarOp::Eq, I64, vec![Operand::Local(i), Operand::Int(0)]);
                let (skip, sep) = self.branch(first);
                self.cur = sep;
                let comma = self.lit(", ");
                let r = self.rt(RtFn::Concat, Ty::Ptr, vec![Operand::Local(out), comma]);
                self.emit(Inst::Copy { dst: out, src: r });
                self.goto(skip);
                let at = self.rt(RtFn::ListAt, Ty::Ptr, vec![list, Operand::Local(i)]);
                let ev = self.read(&At::Mem(at, 0), e);
                let es = self.show(&ev, e, true)?;
                let r = self.rt(RtFn::Concat, Ty::Ptr, vec![Operand::Local(out), es]);
                self.emit(Inst::Copy { dst: out, src: r });
                let next = self.scalar_op(
                    ScalarOp::AddChecked,
                    I64,
                    vec![Operand::Local(i), Operand::Int(1)],
                );
                self.emit(Inst::Copy { dst: i, src: next });
                self.finish(Term::Jump(head));
                self.cur = exit;
                let close = self.lit("]");
                let r = self.rt(RtFn::Concat, Ty::Ptr, vec![Operand::Local(out), close]);
                self.emit(Inst::Copy { dst: out, src: r });
                Operand::Local(out)
            }
        })
    }

    fn show_fields(&mut self, name: &str, v: &[Operand], fs: &[(String, Shape)]) -> R<Operand> {
        let mut parts = vec![self.lit(&format!("{name}("))];
        let mut at = 0;
        for (i, (n, s)) in fs.iter().enumerate() {
            let sep = if i > 0 { ", " } else { "" };
            parts.push(self.lit(&format!("{sep}{n}=")));
            let w = s.width();
            parts.push(self.show(&v[at..at + w], s, true)?);
            at += w;
        }
        parts.push(self.lit(")"));
        Ok(self.concat(parts))
    }

    fn place(&mut self, p: &y::Place) -> R<(At, Shape)> {
        let root = self.name(&p.root)?.clone();
        let mut at = At::Reg(root.locals);
        let mut shape = root.shape;
        for proj in &p.projs {
            match proj {
                y::Proj::Field(f, _) => {
                    let Some((_, start, s)) = shape
                        .fields()
                        .and_then(|fs| fs.into_iter().find(|(n, ..)| n == f))
                    else {
                        return unsupported(format!("field `{f}`"));
                    };
                    let w = s.width();
                    let s = s.clone();
                    at = match at {
                        At::Reg(ls) => At::Reg(ls[start..start + w].to_vec()),
                        At::Mem(p, word) => At::Mem(p, word + start as u32),
                    };
                    shape = s;
                }
                y::Proj::Index(i) => {
                    let Shape::List(e) = shape else {
                        return unsupported("indexing a non-list");
                    };
                    let list = self.read(&at, &Shape::List(e.clone())).remove(0);
                    let i = self.scalar1(i)?;
                    at = At::Mem(self.rt(RtFn::ListAt, Ty::Ptr, vec![list, i]), 0);
                    shape = *e;
                }
            }
        }
        Ok((at, shape))
    }

    fn read(&mut self, at: &At, s: &Shape) -> Vec<Operand> {
        match at {
            At::Reg(ls) => ls.iter().map(|l| Operand::Local(*l)).collect(),
            At::Mem(p, word) => s
                .tys()
                .into_iter()
                .enumerate()
                .map(|(i, t)| {
                    let d = self.local(t);
                    self.emit(Inst::Load {
                        dst: d,
                        ptr: p.clone(),
                        word: word + i as u32,
                    });
                    Operand::Local(d)
                })
                .collect(),
        }
    }

    fn write(&mut self, at: &At, v: Vec<Operand>) {
        match at {
            At::Reg(ls) => self.copy_to(&ls.clone(), v),
            At::Mem(p, word) => {
                for (i, src) in v.into_iter().enumerate() {
                    self.emit(Inst::Store {
                        ptr: p.clone(),
                        word: word + i as u32,
                        src,
                    });
                }
            }
        }
    }

    /// The value of `r`, of type `want`.
    fn value(&mut self, r: &y::Rhs, want: &Shape) -> R<Vec<Operand>> {
        match r {
            y::Rhs::Atom(a) => self.atom(a, want),
            y::Rhs::Call(c) => {
                let v = self.var(want.clone());
                let ops = Self::ops(&v);
                self.call(c, Some(v))?;
                Ok(ops)
            }
            y::Rhs::Proj(p) | y::Rhs::Move(p) => {
                let (at, s) = self.place(p)?;
                Ok(self.read(&at, &s))
            }
            y::Rhs::Copy(p) => {
                let (at, s) = self.place(p)?;
                let v = self.read(&at, &s);
                match &s {
                    Shape::List(e) if !e.has_list() => {
                        Ok(vec![self.rt(RtFn::ListCopy, Ty::Ptr, v)])
                    }
                    s if s.has_list() => unsupported("a copy of nested lists"),
                    _ => Ok(v),
                }
            }
            y::Rhs::Ctor(c) => self.ctor(c, want),
            y::Rhs::Try(c) => self.value(&y::Rhs::Call(c.clone()), want),
            y::Rhs::Propagate { call, via: None } => self.propagate(call, want),
            y::Rhs::Propagate { .. } => unsupported("`?` through a conversion"),
            y::Rhs::Closure(_) => unsupported("a closure"),
        }
    }

    /// `c?`: its `Ok` value, or return its error from this function.
    fn propagate(&mut self, c: &y::Call, want: &Shape) -> R<Vec<Operand>> {
        let Some(&func) = self.ids.get(&c.path.join(".")) else {
            return unsupported(format!("`?` on `{}`", c.path.join(".")));
        };
        let theirs = self.sigs[func as usize].ret.clone();
        let mine = self.sigs[self.me].ret.clone();
        let (Some((_, ok_at, _)), Some((_, err_at, efs)), Some((_, _, mine_e))) = (
            theirs.variant("Ok"),
            theirs.variant("Err"),
            mine.variant("Err"),
        ) else {
            return unsupported("`?` outside a failing function");
        };
        if efs[0].1 != mine_e[0].1 {
            return unsupported("`?` across error types");
        }
        let e = efs[0].1.clone();
        let r = self.var(theirs);
        let v = Self::ops(&r);
        self.call(c, Some(r))?;
        let is_ok = self.scalar_op(ScalarOp::Eq, I64, vec![v[0].clone(), Operand::Int(0)]);
        let (ok, err) = self.branch(is_ok);
        self.cur = err;
        let ev = v[err_at..err_at + e.width()].to_vec();
        self.ret(Self::variant_value(&mine, "Err", ev));
        self.cur = ok;
        Ok(v[ok_at..ok_at + want.width()].to_vec())
    }

    fn ctor(&mut self, c: &y::Ctor, want: &Shape) -> R<Vec<Operand>> {
        match (c, want) {
            (y::Ctor::Tuple(xs), Shape::Tuple(ss)) if xs.len() == ss.len() => {
                let mut v = Vec::new();
                for (x, s) in xs.iter().zip(ss) {
                    v.extend(self.atom(x, s)?);
                }
                Ok(v)
            }
            (y::Ctor::Named { path, fields, .. }, _) => {
                let name = path.last().expect("named");
                let (tag, start, fs): (Option<i128>, usize, Vec<(String, Shape)>) = match want {
                    Shape::Rec(_, fs) => (None, 0, fs.clone()),
                    Shape::Sum(..) => match want.variant(name) {
                        Some((t, at, fs)) => (Some(t), at, fs.to_vec()),
                        None => return unsupported(format!("constructor `{name}`")),
                    },
                    _ => return unsupported(format!("constructor `{name}`")),
                };
                let mut v: Vec<Operand> = want.tys().into_iter().map(zero).collect();
                if let Some(t) = tag {
                    v[0] = Operand::Int(t);
                }
                let mut at = start;
                for (n, s) in &fs {
                    let Some((_, x)) = fields.iter().find(|(f, _)| f == n) else {
                        return unsupported(format!("constructor `{name}` without field `{n}`"));
                    };
                    let xv = self.atom(x, s)?;
                    let w = xv.len();
                    v.splice(at..at + w, xv);
                    at += w;
                }
                Ok(v)
            }
            _ => unsupported("a constructor"),
        }
    }

    /// Test `v` against `p`, binding names, and continue in `fail` when it does not match.
    fn pat(&mut self, p: &y::Pat, v: &[Operand], s: &Shape, fail: BlockId) -> R<()> {
        let test = |me: &mut Self, ok: Operand| {
            let next = me.block_new();
            me.finish(Term::Branch {
                cond: ok,
                then: next,
                els: fail,
            });
            me.cur = next;
        };
        match (p, s) {
            (y::Pat::Wild, _) => {}
            (y::Pat::Bind(n), _) => {
                let var = self.var(s.clone());
                self.copy_to(&var.locals.clone(), v.to_vec());
                self.names.insert(n.clone(), var);
            }
            (y::Pat::Lit(l), _) => {
                let (o, _) = self.literal(l)?;
                let ok = match s {
                    Shape::Str => self.rt(RtFn::StrEq, Ty::Bool, vec![v[0].clone(), o]),
                    Shape::Scalar(t @ (Ty::Int(_) | Ty::Bool)) => {
                        let d = self.local(Ty::Bool);
                        self.emit(Inst::Scalar {
                            dst: Some(d),
                            id: registry::id("eq_i64").expect("registered"),
                            op: ScalarOp::Eq,
                            ty: *t,
                            args: vec![v[0].clone(), o],
                        });
                        Operand::Local(d)
                    }
                    _ => return unsupported("this literal pattern"),
                };
                test(self, ok);
            }
            (y::Pat::Range(lo, hi), Shape::Scalar(t @ Ty::Int(_))) => {
                let (lo, _) = self.literal(lo)?;
                let (hi, _) = self.literal(hi)?;
                for (op, bound) in [(ScalarOp::Ge, lo), (ScalarOp::Le, hi)] {
                    let ok = self.scalar_op(op, *t, vec![v[0].clone(), bound]);
                    test(self, ok);
                }
            }
            (y::Pat::Tuple(ps), Shape::Tuple(ss)) if ps.len() == ss.len() => {
                let mut at = 0;
                for (p, e) in ps.iter().zip(ss) {
                    let w = e.width();
                    self.pat(p, &v[at..at + w], e, fail)?;
                    at += w;
                }
            }
            (y::Pat::Ctor { path, fields }, Shape::Rec(..) | Shape::Sum(..)) => {
                let name = path.last().expect("named");
                let (start, fs) = match s {
                    Shape::Sum(..) => {
                        let Some((tag, at, fs)) = s.variant(name) else {
                            return unsupported(format!("pattern `{name}`"));
                        };
                        let ok = self.scalar_op(
                            ScalarOp::Eq,
                            I64,
                            vec![v[0].clone(), Operand::Int(tag)],
                        );
                        test(self, ok);
                        (at, fs.to_vec())
                    }
                    Shape::Rec(_, fs) => (0, fs.clone()),
                    _ => unreachable!("matched"),
                };
                for (n, sub) in fields.iter().flat_map(|(fs, _)| fs) {
                    let mut at = start;
                    let Some((_, fsh)) = fs.iter().find(|(f, sh)| {
                        if f == n {
                            return true;
                        }
                        at += sh.width();
                        false
                    }) else {
                        return unsupported(format!("field `{n}`"));
                    };
                    let w = fsh.width();
                    self.pat(sub, &v[at..at + w], fsh, fail)?;
                }
            }
            (y::Pat::Or(ps), _) => {
                let ok = self.block_new();
                for p in ps {
                    let next = self.block_new();
                    let before = self.names.len();
                    self.pat(p, v, s, next)?;
                    if self.names.len() != before {
                        return unsupported("an or-pattern that binds");
                    }
                    self.finish(Term::Jump(ok));
                    self.cur = next;
                }
                self.finish(Term::Jump(fail));
                self.cur = ok;
            }
            _ => return unsupported("this pattern"),
        }
        Ok(())
    }

    /// Names bound in `b` go out of scope at its end.
    fn block(&mut self, b: &y::Block) -> R<()> {
        let outer = self.names.clone();
        let r = self.block_in(b);
        self.names = outer;
        r
    }

    fn block_in(&mut self, b: &y::Block) -> R<()> {
        let decls: Vec<_> = b
            .stmts
            .iter()
            .filter_map(|s| match &s.kind {
                y::StmtKind::Jp { name, params, body } => Some((name, params, body)),
                _ => None,
            })
            .collect();
        if decls.is_empty() {
            return b.stmts.iter().try_for_each(|s| self.stmt(s));
        }
        let cont = self.local(I64);
        let mut jps = Vec::new();
        let mut jp_vars = Vec::new();
        for (name, params, _) in &decls {
            let mut ls = Vec::new();
            let mut vs = Vec::new();
            for p in params.iter() {
                let y::Param::Value { name, ty, .. } = p else {
                    return unsupported("a capability join-point parameter");
                };
                let v = self.var(self.types.shape(ty)?);
                ls.extend(&v.locals);
                vs.push((name.clone(), v));
            }
            jps.push(((*name).clone(), self.block_new(), ls));
            jp_vars.push(vs);
        }
        self.scopes.push(Scope::Jps {
            jps: jps.clone(),
            cont,
            at: None,
        });
        let mut resume = Vec::new();
        // A join point's body sees the names declared before it, and its parameters.
        let mut seen = Vec::new();
        for s in &b.stmts {
            if matches!(s.kind, y::StmtKind::Jp { .. }) {
                seen.push(self.names.clone());
                continue;
            }
            if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
                *at = Some(resume.len() as i128);
            }
            self.stmt(s)?;
            let next = self.block_new();
            self.goto(next);
            resume.push(next);
        }
        let after = self.cur;
        if let Some(Scope::Jps { at, .. }) = self.scopes.last_mut() {
            *at = None;
        }
        let jps_in = decls.iter().zip(&jps).zip(jp_vars).zip(seen);
        for ((((_, _, body), (_, start, _)), vs), names) in jps_in {
            self.cur = *start;
            self.names = names;
            self.names.extend(vs);
            self.block(body)?;
            self.finish(Term::Switch {
                on: cont,
                targets: resume.clone(),
            });
        }
        self.scopes.pop();
        self.cur = after;
        Ok(())
    }

    fn jump(&mut self, name: &str, args: &[y::Atom]) -> R<()> {
        let found = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Jps { jps, cont, at } => jps
                .iter()
                .find(|(n, ..)| n == name)
                .map(|(_, b, ps)| (*b, ps.clone(), *cont, *at)),
            Scope::Loop { .. } => None,
        });
        let Some((target, params, cont, at)) = found else {
            return unsupported(format!("jump to `{name}`"));
        };
        let mut vals = Vec::new();
        for a in args {
            vals.extend(self.atom_any(a)?.0);
        }
        // Through temporaries, so `jump k(b, a)` reads before it writes.
        let temps: Vec<Local> = vals
            .into_iter()
            .zip(&params)
            .map(|(src, p)| {
                let t = self.local(self.locals[*p as usize]);
                self.emit(Inst::Copy { dst: t, src });
                t
            })
            .collect();
        for (p, t) in params.iter().zip(temps) {
            self.emit(Inst::Copy {
                dst: *p,
                src: Operand::Local(t),
            });
        }
        if let Some(at) = at {
            self.emit(Inst::Copy {
                dst: cont,
                src: Operand::Int(at),
            });
        }
        self.end(Term::Jump(target));
        Ok(())
    }

    fn leave(&mut self, label: &str, brk: bool) -> R<()> {
        let to = self.scopes.iter().rev().find_map(|s| match s {
            Scope::Loop {
                label: l,
                head,
                exit,
            } if l == label => Some(if brk { *exit } else { *head }),
            _ => None,
        });
        match to {
            Some(b) => {
                self.end(Term::Jump(b));
                Ok(())
            }
            None => unsupported(format!("loop label `{label}`")),
        }
    }

    fn stmt(&mut self, s: &y::Stmt) -> R<()> {
        use y::StmtKind as K;
        match &s.kind {
            K::Let { pat, ty, rhs, .. } => {
                let shape = self.types.shape(ty)?;
                let v = self.value(rhs, &shape)?;
                match pat {
                    y::Pat::Bind(n) => {
                        let var = self.var(shape);
                        self.copy_to(&var.locals.clone(), v);
                        self.names.insert(n.clone(), var);
                    }
                    y::Pat::Wild => {}
                    p => {
                        let fail = self.block_new();
                        self.pat(p, &v, &shape, fail)?;
                        let ok = self.cur;
                        self.cur = fail;
                        self.finish(Term::Panic("unreachable"));
                        self.cur = ok;
                    }
                }
                Ok(())
            }
            K::Assign { place, rhs, .. } => {
                let (at, shape) = self.place(place)?;
                let v = self.value(rhs, &shape)?;
                self.write(&at, v);
                Ok(())
            }
            K::If { cond, then, els } => {
                let cond = self.scalar1(cond)?;
                let (t, e) = self.branch(cond);
                let join = self.block_new();
                self.cur = t;
                self.block(then)?;
                self.goto(join);
                self.cur = e;
                if let Some(els) = els {
                    self.block(els)?;
                }
                self.goto(join);
                Ok(())
            }
            K::Match { scrutinee, arms } => {
                let (v, shape) = self.atom_any(scrutinee)?;
                let join = self.block_new();
                for (p, body) in arms {
                    let next = self.block_new();
                    self.pat(p, &v, &shape, next)?;
                    self.block(body)?;
                    self.finish(Term::Jump(join));
                    self.cur = next;
                }
                self.finish(Term::Panic("unreachable"));
                self.cur = join;
                Ok(())
            }
            K::Loop { label, body, .. } => {
                let (head, exit) = (self.block_new(), self.block_new());
                self.goto(head);
                self.scopes.push(Scope::Loop {
                    label: label.clone(),
                    head,
                    exit,
                });
                self.block(body)?;
                self.scopes.pop();
                self.goto(head);
                self.cur = exit;
                Ok(())
            }
            K::Break { value: Some(_), .. } => unsupported("`break` with a value"),
            K::Break { label, value: None } => self.leave(label, true),
            K::Continue { label } => self.leave(label, false),
            K::Jp { .. } => unreachable!("handled by `block`"),
            K::Jump { name, args } => self.jump(name, args),
            K::Ret(a) => {
                let sig = &self.sigs[self.me];
                let (ok, ret, fails) = (sig.ok.clone(), sig.ret.clone(), sig.fails);
                let v = self.atom(a, &ok)?;
                let v = if fails {
                    Self::variant_value(&ret, "Ok", v)
                } else {
                    v
                };
                self.ret(v);
                Ok(())
            }
            K::Fail(a) => {
                let ret = self.sigs[self.me].ret.clone();
                let Some((_, _, fs)) = ret.variant("Err") else {
                    return unsupported("`fail` outside a failing function");
                };
                let e = fs[0].1.clone();
                let v = self.atom(a, &e)?;
                self.ret(Self::variant_value(&ret, "Err", v));
                Ok(())
            }
            K::Become(c) => {
                let name = c.path.join(".");
                let Some(&func) = self.ids.get(&name) else {
                    return unsupported(format!("`become` of `{name}`"));
                };
                if !self.inout.is_empty() || !self.sigs[func as usize].inout().is_empty() {
                    return unsupported("`become` with `inout` parameters");
                }
                if self.sigs[func as usize].ret != self.sigs[self.me].ret {
                    return unsupported("`become` across failure types");
                }
                let (args, _) = self.user_args(func, c)?;
                self.end(Term::TailCall { func, args });
                Ok(())
            }
            // Names bound in a `par` child are read after the block.
            K::Par { blocks, .. } => blocks.iter().try_for_each(|b| self.block_in(b)),
            K::Assert(e) => {
                let t = self.expr(e)?;
                self.check(t, "assert");
                Ok(())
            }
            K::Unreachable => {
                self.end(Term::Panic("unreachable"));
                Ok(())
            }
            K::Unsafe(b) => self.block(b),
            K::Drop(_) => Ok(()),
            K::Rhs(y::Rhs::Call(c)) => self.call(c, None),
            K::Rhs(_) => Ok(()),
            K::Yield(_) => unsupported("generators"),
            K::With { .. } => unsupported("`with`"),
            K::ComptimeFor { .. } | K::ComptimeIf { .. } => unsupported("a comptime statement"),
            K::Asm { .. } => unsupported("`asm`"),
            K::Reuse { .. } => unsupported("`reuse`"),
        }
    }
}

#[cfg(test)]
mod selftest {
    use super::*;

    #[test]
    fn lowers_join_points_and_reports_the_rest() {
        let src = "module t\nfn main() -> Unit {\n var n: i64 = 0\n\
                   jp k(let m: i64) { n = add_checked_i64(let n, let m) }\n\
                   loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n\
                   if big { jump k(10) }\n }\n assert eq_i64(let n, let 13)\n ret ()\n}\n";
        let (m, d) = yong_core::parse("t.yong", src);
        assert!(d.is_empty());
        let p = lower(&m.unwrap()).expect("lowers");
        let f = &p.funcs[p.main as usize];
        assert!(
            f.blocks
                .iter()
                .any(|b| matches!(b.term, Term::Switch { .. }))
        );
        let (m, _) = yong_core::parse(
            "t.yong",
            "module t\nsum T { Leaf, Node { pub l: T } }\nfn main() -> Unit {\n let t: T = Leaf\n ret ()\n}\n",
        );
        assert!(
            lower(&m.unwrap())
                .unwrap_err()
                .contains("recursive type `T`")
        );
    }

    #[test]
    fn splits_sums_into_a_tag_and_variant_slots() {
        let (m, d) = yong_core::parse(
            "t.yong",
            "module t\nsum S { A { pub x: i64 }, B { pub y: f64, pub z: bool }, C }\n\
             fn main() -> Unit {\n let s: S = B { y: 1.0, z: true }\n ret ()\n}\n",
        );
        assert!(d.is_empty(), "{d:?}");
        let p = lower(&m.unwrap()).expect("lowers");
        let f = &p.funcs[p.main as usize];
        assert_eq!(f.locals[..4], [I64, I64, Ty::F64, Ty::Bool]);
    }
}
