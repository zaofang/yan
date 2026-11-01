//! The register VM that runs compiled Yong IR.

use std::mem;
use std::rc::Rc;

use crate::bind::{self, Bind};
use crate::compile::*;
use crate::{Interp, R, Value, data, intrinsic, panic, unsupported};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clo {
    code: usize,
    captured: Vec<Value>,
}

pub(crate) struct Vm<'o> {
    codes: Vec<Rc<Code>>,
    stack: Vec<Value>,
    it: Interp<'o>,
}

/// The type name a value's `impl` is keyed by, where the value tells it.
fn type_key(v: &Value) -> Option<&str> {
    Some(match v {
        Value::Data { name, .. } => name,
        Value::Int(_) => "i64",
        Value::Float(_) => "f64",
        Value::Bool(_) => "bool",
        Value::Str(_) => "Str",
        Value::Char(_) => "char",
        _ => return None,
    })
}

/// `v.clone()`, inline for scalars.
#[inline(always)]
fn dup(v: &Value) -> Value {
    match v {
        Value::Int(n) => Value::Int(*n),
        Value::Bool(b) => Value::Bool(*b),
        Value::Float(f) => Value::Float(*f),
        Value::Unit => Value::Unit,
        v => v.clone(),
    }
}

/// `stack.truncate(len)`, skipping drop glue for scalars.
#[inline(always)]
fn release(stack: &mut Vec<Value>, len: usize) {
    while stack.len() > len {
        match stack.pop() {
            Some(v @ (Value::Int(_) | Value::Bool(_) | Value::Float(_) | Value::Unit)) => {
                mem::forget(v)
            }
            v => drop(v),
        }
    }
}

/// A scalar op result, written into a register without drop glue when it held a scalar.
enum Scalar {
    I(i128),
    F(f64),
    B(bool),
}

#[inline(always)]
fn set(slot: &mut Value, v: Scalar) {
    match (&mut *slot, v) {
        (Value::Int(x), Scalar::I(y)) => *x = y,
        (Value::Bool(x), Scalar::B(y)) => *x = y,
        (Value::Float(x), Scalar::F(y)) => *x = y,
        (_, Scalar::I(y)) => *slot = Value::Int(y),
        (_, Scalar::B(y)) => *slot = Value::Bool(y),
        (_, Scalar::F(y)) => *slot = Value::Float(y),
    }
}

fn checked(v: Option<i128>, ty: u8) -> Result<Scalar, &'static str> {
    let (lo, hi) = INT_RANGES[ty as usize];
    match v {
        Some(v) if (lo..=hi).contains(&v) => Ok(Scalar::I(v)),
        _ => Err("overflow"),
    }
}

#[inline(always)]
fn int_op(op: IntOp, ty: u8, a: i128, b: i128) -> Result<Scalar, &'static str> {
    Ok(match op {
        IntOp::Add => return checked(a.checked_add(b), ty),
        IntOp::Sub => return checked(a.checked_sub(b), ty),
        IntOp::Mul => return checked(a.checked_mul(b), ty),
        IntOp::FloorDiv | IntOp::Mod if b == 0 => return Err("div_zero"),
        // i128 division is a libcall; i64 operands take the hardware path.
        IntOp::FloorDiv | IntOp::Mod
            if let (Ok(x), Ok(y)) = (i64::try_from(a), i64::try_from(b))
                && let Some(r) = x.checked_rem_euclid(y) =>
        {
            let v = if matches!(op, IntOp::Mod) {
                i128::from(if r != 0 && y < 0 { r + y } else { r })
            } else {
                i128::from(x.div_euclid(y)) - i128::from(r != 0 && y < 0)
            };
            return checked(Some(v), ty);
        }
        IntOp::FloorDiv => {
            return checked(
                Some(a.div_euclid(b) - i128::from(a.rem_euclid(b) != 0 && b < 0)),
                ty,
            );
        }
        IntOp::Mod => {
            let r = a.rem_euclid(b);
            return checked(Some(if r != 0 && b < 0 { r + b } else { r }), ty);
        }
        IntOp::Div => Scalar::F(a as f64 / b as f64),
        IntOp::And => Scalar::I(a & b),
        IntOp::Or => Scalar::I(a | b),
        IntOp::Xor => Scalar::I(a ^ b),
        IntOp::Cmp(c) => Scalar::B(cmp(c, &a, &b)),
    })
}

fn cmp<T: PartialOrd>(c: Cmp, a: &T, b: &T) -> bool {
    match c {
        Cmp::Eq => a == b,
        Cmp::Ne => a != b,
        Cmp::Lt => a < b,
        Cmp::Le => a <= b,
        Cmp::Gt => a > b,
        Cmp::Ge => a >= b,
    }
}

fn float_op(op: FloatOp, a: f64, b: f64) -> Scalar {
    match op {
        FloatOp::Add => Scalar::F(a + b),
        FloatOp::Sub => Scalar::F(a - b),
        FloatOp::Mul => Scalar::F(a * b),
        FloatOp::Div => Scalar::F(a / b),
        FloatOp::Cmp(c) => Scalar::B(cmp(c, &a, &b)),
    }
}

fn field<'v>(v: &'v Value, f: &str) -> Option<&'v Value> {
    match v {
        Value::Data { fields, .. } => fields.iter().find(|(n, _)| n == f).map(|(_, v)| v),
        Value::Tuple(xs) => f
            .strip_prefix('_')
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| xs.get(i)),
        _ => None,
    }
}

fn matches(p: &CPat, v: &Value) -> bool {
    match (p, v) {
        (CPat::Wild | CPat::Bind(_), _) => true,
        (CPat::Nullary(n), Value::Data { name, fields }) => name == n && fields.is_empty(),
        (CPat::Lit(l), v) => l.as_ref() == Some(v),
        (CPat::Range(Some((lo, hi))), Value::Int(x)) => (lo..=hi).contains(&x),
        (CPat::Or(ps), v) => ps.iter().any(|p| matches(p, v)),
        (CPat::Tuple(ps), Value::Tuple(xs)) => {
            ps.len() == xs.len() && ps.iter().zip(xs).all(|(p, x)| matches(p, x))
        }
        (
            CPat::Ctor {
                name: n,
                fields: pf,
            },
            Value::Data { name, fields },
        ) => {
            n == name
                && pf.as_ref().is_none_or(|ps| {
                    ps.iter()
                        .all(|(n, p)| fields.iter().any(|(m, x)| m == n && matches(p, x)))
                })
        }
        (CPat::Slice { elems, rest }, Value::List(xs)) => {
            let fits = if rest.is_some() {
                xs.len() >= elems.len()
            } else {
                xs.len() == elems.len()
            };
            fits && elems.iter().zip(xs).all(|(p, x)| matches(p, x))
        }
        _ => false,
    }
}

/// The bindings of `p`, which matched `v`, in binding order.
fn bindings(p: &CPat, v: &Value, out: &mut Vec<(Reg, Value)>) {
    match (p, v) {
        (CPat::Bind(r), _) => out.push((*r, dup(v))),
        (CPat::Tuple(ps), Value::Tuple(xs)) => {
            for (p, x) in ps.iter().zip(xs) {
                bindings(p, x, out);
            }
        }
        (
            CPat::Ctor {
                fields: Some(ps), ..
            },
            Value::Data { fields, .. },
        ) => {
            for (n, p) in ps {
                if let Some((_, x)) = fields.iter().find(|(m, _)| m == n) {
                    bindings(p, x, out);
                }
            }
        }
        (CPat::Or(ps), v) => {
            if let Some(p) = ps.iter().find(|p| matches(p, v)) {
                bindings(p, v, out);
            }
        }
        (CPat::Slice { elems, rest }, Value::List(xs)) => {
            for (p, x) in elems.iter().zip(xs) {
                bindings(p, x, out);
            }
            if let Some(Some(r)) = rest {
                out.push((*r, Value::List(xs[elems.len()..].to_vec())));
            }
        }
        _ => {}
    }
}

enum Target {
    Code(usize, Option<Rc<Clo>>),
    Native,
}

impl<'o> Vm<'o> {
    pub(crate) fn new(p: Program, it: Interp<'o>) -> Self {
        Vm {
            codes: p.codes,
            stack: Vec::with_capacity(1 << 12),
            it,
        }
    }

    /// Call code `f` with `args`. The outcome, plus the final values of the parameters.
    pub(crate) fn call(
        &mut self,
        f: usize,
        args: Vec<Value>,
    ) -> R<(Result<Value, Value>, Vec<Value>)> {
        let base = self.stack.len();
        let n = args.len();
        self.stack.extend(args);
        self.enter(f, None, base, n);
        let r = self.exec(f, base)?;
        let finals = self.stack.drain(base..).take(n).collect();
        Ok((r, finals))
    }

    /// Call closure `c`; a closure cannot fail, since function types have no `fail` yet.
    pub(crate) fn apply(&mut self, c: &Rc<Clo>, args: Vec<Value>) -> R<Value> {
        let base = self.stack.len();
        let n = args.len();
        self.stack.extend(args);
        self.enter(c.code, Some(c), base, n);
        let r = self.exec(c.code, base)?;
        release(&mut self.stack, base);
        match r {
            Ok(v) => Ok(v),
            Err(_) => unsupported("a closure failed inside std.iter"),
        }
    }

    /// Lay out the frame at `base`, whose first `nargs` slots hold the arguments.
    fn enter(&mut self, f: usize, clo: Option<&Clo>, base: usize, nargs: usize) {
        let code = &self.codes[f];
        let np = code.nparams as usize;
        release(&mut self.stack, base + nargs.min(np));
        self.stack
            .resize_with(base + code.nregs as usize, || Value::Unit);
        if let Some(c) = clo {
            for (r, v) in code.capture_regs.iter().zip(&c.captured) {
                // A parameter of the same name wins over the capture.
                if (*r as usize) >= nargs.min(np) {
                    self.stack[base + *r as usize] = v.clone();
                }
            }
        }
    }

    fn val<'a>(&'a self, code: &'a Code, base: usize, o: Opnd) -> &'a Value {
        match o.konst() {
            Some(k) => &code.consts[k],
            None => &self.stack[base + o.idx()],
        }
    }

    fn exec(&mut self, f: usize, base: usize) -> R<Result<Value, Value>> {
        let mut code = self.codes[f].clone();
        let mut pc = 0usize;
        loop {
            let op = code.ops[pc];
            pc += 1;
            match op {
                Op::Move { dst, src } => {
                    let v = match *self.val(&code, base, src) {
                        Value::Int(n) => Scalar::I(n),
                        Value::Bool(b) => Scalar::B(b),
                        Value::Float(f) => Scalar::F(f),
                        ref v => {
                            let v = v.clone();
                            self.stack[base + dst as usize] = v;
                            continue;
                        }
                    };
                    set(&mut self.stack[base + dst as usize], v);
                }
                Op::Read { dst, place } => {
                    let v = self.read(&code, base, &code.places[place as usize])?;
                    self.stack[base + dst as usize] = v;
                }
                Op::Write { place, src } => {
                    let v = mem::take(&mut self.stack[base + src as usize]);
                    self.write(&code, base, &code.places[place as usize], v)?;
                }
                Op::Restore { dst, src } => {
                    let v = mem::take(&mut self.stack[base + src as usize]);
                    self.stack[base + dst as usize] = v;
                }
                Op::Clear(r) => self.stack[base + r as usize] = Value::Unit,
                Op::Jmp(to) => pc = to as usize,
                Op::JmpIf { cond, to } => match self.val(&code, base, cond) {
                    Value::Bool(true) => pc = to as usize,
                    Value::Bool(false) => {}
                    v => return unsupported(format!("condition is not a bool: {v:?}")),
                },
                Op::JmpIfNot { cond, to } => match self.val(&code, base, cond) {
                    Value::Bool(true) => {}
                    Value::Bool(false) => pc = to as usize,
                    v => return unsupported(format!("condition is not a bool: {v:?}")),
                },
                Op::CmpJmp {
                    when,
                    c,
                    dst,
                    a,
                    b,
                    to,
                    site,
                } => {
                    let t = match (self.val(&code, base, a), self.val(&code, base, b)) {
                        (Value::Int(a), Value::Int(b)) => {
                            let t = cmp(c, a, b);
                            set(&mut self.stack[base + dst as usize], Scalar::B(t));
                            t
                        }
                        _ => {
                            let v = self.slow(&code, base, site)?;
                            self.stack[base + dst as usize] = v;
                            match &self.stack[base + dst as usize] {
                                Value::Bool(t) => *t,
                                v => return unsupported(format!("condition is not a bool: {v:?}")),
                            }
                        }
                    };
                    if t == when {
                        pc = to as usize;
                    }
                }
                Op::SetCont { reg, to } => {
                    self.stack[base + reg as usize] = Value::Int(i128::from(to));
                }
                Op::JmpCont(reg) => match self.stack[base + reg as usize] {
                    Value::Int(to) => pc = to as usize,
                    _ => unreachable!("join point continuation is set before the jump"),
                },
                Op::Ret(o) => return Ok(Ok(dup(self.val(&code, base, o)))),
                Op::Fail(o) => return Ok(Err(dup(self.val(&code, base, o)))),
                Op::Call { site, dst } => {
                    let s = &code.sites[site as usize];
                    let r = self.call_site(&code, base, s)?;
                    let v = match (r, &s.mode) {
                        (Ok(v), Mode::Try) => data("Ok", "value", v),
                        (Err(e), Mode::Try) => data("Err", "error", e),
                        (Ok(v), _) => v,
                        (Err(e), Mode::Plain) => return Ok(Err(e)),
                        (Err(e), Mode::Propagate(via)) => return self.via(via, e).map(Err),
                        (Err(_), Mode::Contract) => {
                            return unsupported("a failing call in a contract or assertion");
                        }
                        (Err(_), Mode::Become) => unreachable!("become is its own op"),
                    };
                    self.stack[base + dst as usize] = v;
                }
                Op::Become { site } => {
                    let s = &code.sites[site as usize];
                    let new_base = self.stack.len();
                    self.push_args(&code, base, s);
                    let (g, clo) = match self.target_at(s, base, new_base) {
                        Target::Code(g, clo) => (g, clo),
                        Target::Native => {
                            return unsupported("`become` of a std function or intrinsic");
                        }
                    };
                    let nargs = self.stack.len() - new_base;
                    // Move the arguments down over the old frame.
                    if nargs <= new_base - base {
                        for i in 0..nargs {
                            self.stack.swap(base + i, new_base + i);
                        }
                        release(&mut self.stack, base + nargs);
                    } else {
                        self.stack.drain(base..new_base);
                    }
                    self.enter(g, clo.as_deref(), base, nargs);
                    code = self.codes[g].clone();
                    pc = 0;
                }
                Op::Int {
                    op,
                    ty,
                    dst,
                    a,
                    b,
                    site,
                } => {
                    if let (Value::Int(a), Value::Int(b)) =
                        (self.val(&code, base, a), self.val(&code, base, b))
                    {
                        match int_op(op, ty, *a, *b) {
                            Ok(v) => set(&mut self.stack[base + dst as usize], v),
                            Err(kind) => return panic(kind),
                        }
                    } else {
                        let v = self.slow(&code, base, site)?;
                        self.stack[base + dst as usize] = v;
                    }
                }
                Op::Float {
                    op,
                    dst,
                    a,
                    b,
                    site,
                } => {
                    if let (Value::Float(a), Value::Float(b)) =
                        (self.val(&code, base, a), self.val(&code, base, b))
                    {
                        let v = float_op(op, *a, *b);
                        set(&mut self.stack[base + dst as usize], v);
                    } else {
                        let v = self.slow(&code, base, site)?;
                        self.stack[base + dst as usize] = v;
                    }
                }
                Op::Bool {
                    op,
                    dst,
                    a,
                    b,
                    site,
                } => {
                    let v = match (self.val(&code, base, a), self.val(&code, base, b)) {
                        (Value::Bool(a), Value::Bool(b)) => Value::Bool(match op {
                            BoolOp::And => *a && *b,
                            BoolOp::Or => *a || *b,
                            BoolOp::Eq => a == b,
                            BoolOp::Ne => a != b,
                        }),
                        _ => self.slow(&code, base, site)?,
                    };
                    self.stack[base + dst as usize] = v;
                }
                Op::Not { dst, a, site } => {
                    let v = match self.val(&code, base, a) {
                        Value::Bool(a) => Value::Bool(!a),
                        _ => self.slow(&code, base, site)?,
                    };
                    self.stack[base + dst as usize] = v;
                }
                Op::Check { src, kind } => {
                    if !matches!(self.stack[base + src as usize], Value::Bool(true)) {
                        return panic(kind);
                    }
                }
                Op::Panic(kind) => return panic(kind),
                Op::Unsupported(m) => return unsupported(code.msgs[m as usize].clone()),
                Op::Match { pat, src, to } => {
                    let p = &code.pats[pat as usize];
                    let v = &self.stack[base + src as usize];
                    if matches(p, v) {
                        let mut out = Vec::new();
                        bindings(p, v, &mut out);
                        for (r, v) in out {
                            self.stack[base + r as usize] = v;
                        }
                    } else {
                        pc = to as usize;
                    }
                }
                Op::Tuple { dst, list } => {
                    let v = Value::Tuple(
                        code.lists[list as usize]
                            .iter()
                            .map(|o| self.val(&code, base, *o).clone())
                            .collect(),
                    );
                    self.stack[base + dst as usize] = v;
                }
                Op::Data { dst, ctor } => {
                    let c = &code.ctors[ctor as usize];
                    let v = Value::Data {
                        name: c.name.clone(),
                        fields: c
                            .fields
                            .iter()
                            .map(|(n, o)| (n.clone(), self.val(&code, base, *o).clone()))
                            .collect(),
                    };
                    self.stack[base + dst as usize] = v;
                }
                Op::Closure { dst, clo } => {
                    let c = &code.clos[clo as usize];
                    let v = Value::Closure(Rc::new(Clo {
                        code: c.code,
                        captured: c
                            .captures
                            .iter()
                            .map(|o| self.val(&code, base, *o).clone())
                            .collect(),
                    }));
                    self.stack[base + dst as usize] = v;
                }
            }
        }
    }

    /// A typed op's generic path, for argument values it does not handle.
    fn slow(&mut self, code: &Code, base: usize, site: u32) -> R<Value> {
        match self.call_site(code, base, &code.sites[site as usize])? {
            Ok(v) => Ok(v),
            Err(_) => unreachable!("intrinsics do not fail"),
        }
    }

    fn via(&mut self, via: &Via, e: Value) -> R<Value> {
        Ok(match via {
            Via::None => e,
            Via::Item(f) => match self.call(*f, vec![e])?.0 {
                Ok(v) | Err(v) => v,
            },
            Via::Native(id) => match id.and_then(bind::get) {
                Some(Bind::Fallible(f)) => match f(&[e])? {
                    Ok(v) | Err(v) => v,
                },
                _ => {
                    return unsupported("`via` of a std function that is not a fallible intrinsic");
                }
            },
        })
    }

    /// Push the arguments of `s` on the stack; `inout` ones are moved out of the frame.
    fn push_args(&mut self, code: &Code, base: usize, s: &Site) {
        for a in &s.args {
            let v = match a {
                ArgC::Inout(r) => mem::take(&mut self.stack[base + *r as usize]),
                ArgC::Val(o) => dup(self.val(code, base, *o)),
            };
            self.stack.push(v);
        }
    }

    /// What `s` calls, given the caller frame at `base` and the arguments pushed at `args`.
    fn target_at(&self, s: &Site, base: usize, args: usize) -> Target {
        match &s.callee {
            Callee::Item(f) => Target::Code(*f, None),
            Callee::Native => Target::Native,
            Callee::Dyn { methods, local } => {
                if let Some(tk) = self.stack[args..].first().and_then(type_key)
                    && let Some((_, f)) = methods.iter().find(|(t, _)| &**t == tk)
                {
                    return Target::Code(*f, None);
                }
                match local.map(|r| &self.stack[base + r as usize]) {
                    Some(Value::Closure(c)) => Target::Code(c.code, Some(c.clone())),
                    _ => Target::Native,
                }
            }
        }
    }

    /// Call `s` and write its `inout` arguments back. `Err` is the callee's failure value.
    fn call_site(&mut self, code: &Code, base: usize, s: &Site) -> R<Result<Value, Value>> {
        let new_base = self.stack.len();
        self.push_args(code, base, s);
        let nargs = s.args.len();
        let outcome = match self.target_at(s, base, new_base) {
            Target::Code(f, clo) => {
                self.enter(f, clo.as_deref(), new_base, nargs);
                let r = self.exec(f, new_base)?;
                for (i, a) in s.args.iter().enumerate() {
                    if let ArgC::Inout(r) = a {
                        let v = self
                            .stack
                            .get_mut(new_base + i)
                            .map(mem::take)
                            .unwrap_or(Value::Unit);
                        self.stack[base + *r as usize] = v;
                    }
                }
                r
            }
            Target::Native if let Some(b) = s.id.and_then(bind::get) => {
                let args = &mut self.stack[new_base..];
                let r = match b {
                    Bind::Plain(f) => Ok(f(args)?),
                    Bind::Cap(f) => Ok(f(&mut self.it, args)?),
                    Bind::Fallible(f) => f(args)?,
                };
                for (i, a) in s.args.iter().enumerate() {
                    if let ArgC::Inout(r) = a {
                        let v = mem::take(&mut self.stack[new_base + i]);
                        self.stack[base + *r as usize] = v;
                    }
                }
                r
            }
            Target::Native
                if s.name.starts_with("std.iter.")
                    || s.name.ends_with(".iter")
                    || matches!(&*s.name, "std.prelude.set" | "std.prelude.dict") =>
            {
                let args = self.stack[new_base..].iter_mut().map(mem::take).collect();
                match self.iter_call(&s.name, args) {
                    Some(r) => Ok(r?),
                    None => return unsupported(format!("call of `{}`", s.name)),
                }
            }
            Target::Native => {
                let r = if super::keyed::KEYED.contains(&&*s.name) {
                    // Taken off the stack: the key closure runs on it.
                    let mut args: Vec<Value> =
                        self.stack[new_base..].iter_mut().map(mem::take).collect();
                    let r = self.keyed_call(&s.name, &mut args)?;
                    for (i, v) in args.into_iter().enumerate() {
                        self.stack[new_base + i] = v;
                    }
                    Ok(r)
                } else {
                    match intrinsic(&s.name, &self.stack[new_base..]) {
                        Some(r) => Ok(r?),
                        None => return unsupported(format!("call of `{}`", s.name)),
                    }
                };
                for (i, a) in s.args.iter().enumerate() {
                    if let ArgC::Inout(r) = a {
                        let v = mem::take(&mut self.stack[new_base + i]);
                        self.stack[base + *r as usize] = v;
                    }
                }
                r
            }
        };
        release(&mut self.stack, new_base);
        Ok(outcome)
    }

    fn read(&self, code: &Code, base: usize, p: &PlaceC) -> R<Value> {
        let mut v = &self.stack[base + p.root as usize];
        for (proj, src) in p.projs.iter().zip(&p.src) {
            v = match (proj, v) {
                (Proj_::Field(f), Value::Data { .. }) => match field(v, f) {
                    Some(v) => v,
                    None => return unsupported(format!("no field `{f}`")),
                },
                (Proj_::Field(f), Value::Tuple(_)) => match field(v, f) {
                    Some(v) => v,
                    None => return unsupported(format!("tuple field `{f}`")),
                },
                (
                    Proj_::Index(i),
                    xs @ (Value::List(_) | Value::Deque(..) | Value::SortedSet(_)),
                ) => {
                    let Value::Int(i) = self.val(code, base, *i) else {
                        return unsupported("non-integer index");
                    };
                    let i = usize::try_from(*i).ok();
                    let got = match xs {
                        Value::List(xs) | Value::SortedSet(xs) => i.and_then(|i| xs.get(i)),
                        Value::Deque(d, _) => i.and_then(|i| d.get(i)),
                        _ => unreachable!("matched"),
                    };
                    match got {
                        Some(v) => v,
                        None => return panic("bounds"),
                    }
                }
                (_, v) => return unsupported(format!("projection {src:?} on {v:?}")),
            };
        }
        Ok(dup(v))
    }

    fn write(&mut self, code: &Code, base: usize, p: &PlaceC, v: Value) -> R<()> {
        for proj in &p.projs {
            if let Proj_::Index(i) = proj
                && !matches!(self.val(code, base, *i), Value::Int(_))
            {
                return unsupported("non-integer index");
            }
        }
        // The root moves out while the projections walk it, so index registers stay readable.
        let mut root = mem::take(&mut self.stack[base + p.root as usize]);
        let r = walk_mut(&mut root, &p.projs, |o| match o.konst() {
            Some(k) => &code.consts[k],
            None => &self.stack[base + o.idx()],
        })
        .map(|slot| *slot = v);
        self.stack[base + p.root as usize] = root;
        r
    }
}

fn walk_mut<'v, 's>(
    mut slot: &'v mut Value,
    projs: &[Proj_],
    index: impl Fn(&Opnd) -> &'s Value,
) -> R<&'v mut Value> {
    for proj in projs {
        slot = match (proj, slot) {
            (Proj_::Field(f), Value::Data { fields, .. }) => {
                match fields.iter_mut().find(|(n, _)| **n == **f) {
                    Some((_, s)) => s,
                    None => return unsupported(format!("no field `{f}`")),
                }
            }
            (Proj_::Index(i), xs @ (Value::List(_) | Value::Deque(..))) => {
                let Value::Int(i) = index(i) else {
                    unreachable!("indices are checked first")
                };
                let i = usize::try_from(*i).ok();
                let got = match xs {
                    Value::List(xs) => i.and_then(|i| xs.get_mut(i)),
                    Value::Deque(d, _) => i.and_then(|i| d.get_mut(i)),
                    _ => unreachable!("matched"),
                };
                match got {
                    Some(s) => s,
                    None => return panic("bounds"),
                }
            }
            _ => return unsupported("assignment through this projection"),
        };
    }
    Ok(slot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The std paths quoted in `src`.
    fn paths(src: &str) -> Vec<&str> {
        src.split('"')
            .skip(1)
            .step_by(2)
            .filter(|c| c.starts_with("std.") && !c.contains(' '))
            .collect()
    }

    #[test]
    fn std_paths_have_one_handler() {
        let stages = [
            ("iter", include_str!("iter.rs")),
            ("keyed", include_str!("keyed.rs")),
        ];
        let mut seen: BTreeMap<&str, &str> = BTreeMap::new();
        for (stage, src) in stages {
            for p in paths(src) {
                if let Some(other) = seen.insert(p, stage) {
                    assert_eq!(other, stage, "`{p}` is handled in two name-matched modules");
                }
            }
        }
        for p in crate::bind::bound() {
            assert!(
                !seen.contains_key(p),
                "`{p}` is bound by id and matched by name"
            );
        }
    }

    #[test]
    fn int_ops_agree_with_intrinsics() {
        let edge = [
            i64::MIN as i128,
            i64::MIN as i128 + 1,
            -7,
            -2,
            -1,
            0,
            1,
            2,
            3,
            7,
            i64::MAX as i128,
        ];
        let ops = [
            (IntOp::FloorDiv, "floordiv"),
            (IntOp::Mod, "mod"),
            (IntOp::Add, "add_checked"),
            (IntOp::Mul, "mul_checked"),
        ];
        for (ti, ty) in INT_TYS
            .iter()
            .enumerate()
            .filter(|(_, t)| ["i64", "i128"].contains(t))
        {
            for (op, name) in ops {
                for a in edge {
                    for b in edge {
                        let want =
                            intrinsic(&format!("{name}_{ty}"), &[Value::Int(a), Value::Int(b)]);
                        let got = match int_op(op, ti as u8, a, b) {
                            Ok(Scalar::I(v)) => Some(Value::Int(v)),
                            Ok(_) => unreachable!("integer op"),
                            Err(_) => None,
                        };
                        assert_eq!(got, want.and_then(Result::ok), "{name}_{ty}({a}, {b})");
                    }
                }
            }
        }
    }
}
