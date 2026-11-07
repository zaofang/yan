//! The register VM that runs compiled Yan IR.

use std::mem;
use std::rc::Rc;

use crate::bind::{self, Bind};
use crate::compile::*;
use crate::{
    Frame, Interp, Operand, R, Stop, Value, data, intrinsic, out_of_range, panic, panic_fields,
    unsupported,
};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Clo {
    code: usize,
    captured: Vec<Value>,
}

pub(crate) struct Vm<'o> {
    codes: Vec<Rc<Code>>,
    stack: Vec<Value>,
    it: Interp<'o>,
    /// Budgets, with the current depth, the steps taken and the live heap bytes at the
    /// start; none at run time.
    limits: Option<(crate::Limits, usize, u64, usize)>,
    /// The frame whose `par concurrent` tasks are running: only its own calls may park.
    task_base: Option<usize>,
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
        IntOp::Div => Scalar::F(yan_rt::num::div_i128(a, b)),
        IntOp::And => Scalar::I(a & b),
        IntOp::Or => Scalar::I(a | b),
        IntOp::Xor => Scalar::I(a ^ b),
        IntOp::Cmp(c) => Scalar::B(cmp(c, &a, &b)),
    })
}

fn int_panic<T>(kind: &str, op: IntOp, ty: u8, a: i128, b: i128) -> R<T> {
    let ty = INT_TYS[ty as usize];
    let what = match op {
        IntOp::Add => format!("{a} + {b}"),
        IntOp::Sub => format!("{a} - {b}"),
        IntOp::Mul => format!("{a} * {b}"),
        IntOp::FloorDiv if b == 0 => format!("floor division of {a} by zero"),
        IntOp::FloorDiv => format!("floor division of {a} by {b}"),
        _ => format!("{a} modulo zero"),
    };
    match kind {
        "overflow" => panic_fields(
            kind,
            format!("{what} overflows {ty}"),
            vec![
                ("type", Operand::Type(ty)),
                ("left", Operand::Int(a)),
                ("right", Operand::Int(b)),
            ],
        ),
        _ => panic_fields(
            kind,
            what,
            vec![("dividend", Operand::Int(a)), ("divisor", Operand::Int(b))],
        ),
    }
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
            limits: None,
            task_base: None,
        }
    }

    pub(crate) fn limit(&mut self, l: crate::Limits) {
        self.limits = Some((l, 0, 0, yan_rt::heap::live()));
    }

    pub(crate) fn steps(&self) -> u64 {
        self.limits.map_or(0, |(_, _, s, _)| s)
    }

    /// One step against the budgets; the clock is read every 4096 steps.
    fn tick(&mut self) -> R<()> {
        let Some((l, _, steps, base)) = &mut self.limits else {
            return Ok(());
        };
        *steps += 1;
        if *steps > l.steps {
            return Err(crate::Stop::Limit("steps"));
        }
        if yan_rt::heap::live().saturating_sub(*base) > l.heap {
            return Err(crate::Stop::Limit("heap"));
        }
        if *steps % 4096 == 0
            && let Some(d) = l.deadline
            && std::time::Instant::now() > d
        {
            return Err(crate::Stop::Limit("time"));
        }
        Ok(())
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

    /// Run the branches at `entries` of `code` as cooperative tasks over the frame at `base`.
    /// A task that parks yields to the lowest-index task that can progress; a task that fails
    /// or returns ends the block with that outcome and drops the rest (R390, R394).
    fn run_tasks(
        &mut self,
        code: &mut Rc<Code>,
        base: usize,
        entries: &[u32],
    ) -> R<Option<Result<Value, Value>>> {
        struct Task {
            pc: usize,
            done: bool,
            park: Option<(bool, usize)>,
            seen: Option<u64>,
        }
        let mut tasks: Vec<Task> = entries
            .iter()
            .map(|e| Task {
                pc: *e as usize,
                done: false,
                park: None,
                seen: None,
            })
            .collect();
        let outer = self.task_base.replace(base);
        self.it.in_par += 1;
        let out = loop {
            let progress = self.it.progress;
            let next = tasks
                .iter()
                .position(|t| !t.done && t.seen.is_none_or(|s| s != progress));
            let (i, forced) = match next {
                Some(i) => (i, false),
                None => match tasks.iter().position(|t| !t.done) {
                    Some(i) => (i, true),
                    None => break Ok(None),
                },
            };
            self.it.force = forced;
            self.it.park = None;
            self.it.task_done = false;
            let mut pc = tasks[i].pc;
            let r = self.exec_from(code, &mut pc, base);
            self.it.force = false;
            let t = &mut tasks[i];
            let prev = t.park.take();
            let r = match r {
                Ok(r) => r,
                Err(e) => break Err(e),
            };
            if let Some(now) = self.it.park.take() {
                if prev != Some(now) {
                    if let Some((true, c)) = prev
                        && let Some(k) = self.it.waiting_recv.iter().position(|w| *w == c)
                    {
                        self.it.waiting_recv.swap_remove(k);
                    }
                    if now.0 {
                        self.it.waiting_recv.push(now.1);
                    }
                    self.it.progress += 1;
                }
                t.pc = pc;
                t.park = Some(now);
                t.seen = Some(self.it.progress);
                continue;
            }
            if let Some((true, c)) = prev
                && let Some(k) = self.it.waiting_recv.iter().position(|w| *w == c)
            {
                self.it.waiting_recv.swap_remove(k);
                self.it.progress += 1;
            }
            if self.it.task_done {
                t.done = true;
            } else {
                break Ok(Some(r));
            }
        };
        for t in &tasks {
            if let Some((true, c)) = t.park
                && let Some(k) = self.it.waiting_recv.iter().position(|w| *w == c)
            {
                self.it.waiting_recv.swap_remove(k);
            }
        }
        self.task_base = outer;
        self.it.in_par -= 1;
        out
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
        // A deadline scope ends with its function, however that returns.
        let outer = self.it.deadline;
        let r = self.exec_from(&mut code, &mut pc, base);
        self.it.deadline = outer;
        match r {
            Err(Stop::Panic(mut p)) => {
                p.trace.push(Frame {
                    item: code.name.clone(),
                    at: code.spans[pc - 1],
                });
                Err(Stop::Panic(p))
            }
            r => r,
        }
    }

    /// Runs `code` from `pc`, leaving both at the op that stopped it.
    fn exec_from(
        &mut self,
        code: &mut Rc<Code>,
        pc: &mut usize,
        base: usize,
    ) -> R<Result<Value, Value>> {
        loop {
            let op = code.ops[*pc];
            *pc += 1;
            match op {
                Op::Move { dst, src } => {
                    let v = match *self.val(code, base, src) {
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
                    let v = self.read(code, base, &code.places[place as usize])?;
                    self.stack[base + dst as usize] = v;
                }
                Op::Write { place, src } => {
                    let v = mem::take(&mut self.stack[base + src as usize]);
                    self.write(code, base, &code.places[place as usize], v)?;
                }
                Op::Restore { dst, src } => {
                    let v = mem::take(&mut self.stack[base + src as usize]);
                    self.stack[base + dst as usize] = v;
                }
                Op::DeadlineBegin { src, saved } => {
                    let Value::Mono(at) = *self.val(code, base, src) else {
                        return unsupported("a `deadline` that is not a monotonic instant");
                    };
                    self.stack[base + saved as usize] = match self.it.deadline {
                        Some(d) => Value::Mono(d),
                        None => Value::Unit,
                    };
                    self.it.deadline = Some(self.it.deadline.map_or(at, |d| d.min(at)));
                }
                Op::DeadlineEnd { saved } => {
                    self.it.deadline = match self.stack[base + saved as usize] {
                        Value::Mono(d) => Some(d),
                        _ => None,
                    };
                }
                Op::Clear(r) => self.stack[base + r as usize] = Value::Unit,
                Op::Jmp(to) => {
                    // A backward jump is a loop iteration, one step (R487).
                    if (to as usize) < *pc {
                        self.tick()?;
                    }
                    *pc = to as usize;
                }
                Op::JmpIf { cond, to } => match self.val(code, base, cond) {
                    Value::Bool(true) => *pc = to as usize,
                    Value::Bool(false) => {}
                    v => return unsupported(format!("condition is not a bool: {v:?}")),
                },
                Op::JmpIfNot { cond, to } => match self.val(code, base, cond) {
                    Value::Bool(true) => {}
                    Value::Bool(false) => *pc = to as usize,
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
                    let t = match (self.val(code, base, a), self.val(code, base, b)) {
                        (Value::Int(a), Value::Int(b)) => {
                            let t = cmp(c, a, b);
                            set(&mut self.stack[base + dst as usize], Scalar::B(t));
                            t
                        }
                        _ => {
                            let v = self.slow(code, base, site)?;
                            self.stack[base + dst as usize] = v;
                            match &self.stack[base + dst as usize] {
                                Value::Bool(t) => *t,
                                v => return unsupported(format!("condition is not a bool: {v:?}")),
                            }
                        }
                    };
                    if t == when {
                        *pc = to as usize;
                    }
                }
                Op::SetCont { reg, to } => {
                    self.stack[base + reg as usize] = Value::Int(i128::from(to));
                }
                Op::JmpCont(reg) => match self.stack[base + reg as usize] {
                    Value::Int(to) => *pc = to as usize,
                    _ => unreachable!("join point continuation is set before the jump"),
                },
                Op::Ret(o) => return Ok(Ok(dup(self.val(code, base, o)))),
                Op::Fail(o) => return Ok(Err(dup(self.val(code, base, o)))),
                Op::Par(i) => {
                    let (entries, join) = code.pars[i as usize].clone();
                    if let Some(r) = self.run_tasks(code, base, &entries)? {
                        return Ok(r);
                    }
                    *pc = join as usize;
                }
                Op::TaskEnd => {
                    self.it.task_done = true;
                    return Ok(Ok(Value::Unit));
                }
                Op::Call { site, dst } => {
                    let s = &code.sites[site as usize];
                    self.it.park_ok = self.task_base == Some(base);
                    let r = self.call_site(code, base, s);
                    self.it.park_ok = false;
                    let r = r?;
                    if self.it.park.is_some() {
                        // Run this call again when the task resumes.
                        *pc -= 1;
                        return Ok(Ok(Value::Unit));
                    }
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
                    self.push_args(code, base, s);
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
                    *code = self.codes[g].clone();
                    *pc = 0;
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
                        (self.val(code, base, a), self.val(code, base, b))
                    {
                        match int_op(op, ty, *a, *b) {
                            Ok(v) => set(&mut self.stack[base + dst as usize], v),
                            Err(kind) => return int_panic(kind, op, ty, *a, *b),
                        }
                    } else {
                        let v = self.slow(code, base, site)?;
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
                        (self.val(code, base, a), self.val(code, base, b))
                    {
                        let v = float_op(op, *a, *b);
                        set(&mut self.stack[base + dst as usize], v);
                    } else {
                        let v = self.slow(code, base, site)?;
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
                    let v = match (self.val(code, base, a), self.val(code, base, b)) {
                        (Value::Bool(a), Value::Bool(b)) => Value::Bool(match op {
                            BoolOp::And => *a && *b,
                            BoolOp::Or => *a || *b,
                            BoolOp::Eq => a == b,
                            BoolOp::Ne => a != b,
                        }),
                        _ => self.slow(code, base, site)?,
                    };
                    self.stack[base + dst as usize] = v;
                }
                Op::Not { dst, a, site } => {
                    let v = match self.val(code, base, a) {
                        Value::Bool(a) => Value::Bool(!a),
                        _ => self.slow(code, base, site)?,
                    };
                    self.stack[base + dst as usize] = v;
                }
                Op::Check { src, info } => {
                    if !matches!(self.stack[base + src as usize], Value::Bool(true)) {
                        let c = &code.contracts[info as usize];
                        let mut fields = vec![("clause".to_owned(), Operand::Int(c.clause.into()))];
                        if let Some(h) = c.item {
                            fields.push(("item".to_owned(), Operand::Hash(h)));
                        }
                        for (n, r) in &c.vars {
                            let o = Operand::of(&self.stack[base + *r as usize]);
                            if !matches!(o, Operand::Type(_)) {
                                fields.push((n.clone(), o));
                            }
                        }
                        return panic_fields(
                            "contract",
                            yan_rt::abi::panic_message("contract"),
                            fields,
                        );
                    }
                }
                Op::Assert { src, info } => {
                    if !matches!(self.stack[base + src as usize], Value::Bool(true)) {
                        let a = &code.asserts[info as usize];
                        let mut msg = format!("line {}", a.line);
                        let mut fields = Vec::new();
                        if let [l, r] = a.sides[..] {
                            let show = |o| crate::prelude::show(self.val(code, base, o), true);
                            msg += &format!(": left is {}, right is {}", show(l), show(r));
                            fields.push(("left", Operand::of(self.val(code, base, l))));
                            fields.push(("right", Operand::of(self.val(code, base, r))));
                        }
                        for (n, r) in &a.vars {
                            let o = Operand::of(&self.stack[base + *r as usize]);
                            if !matches!(o, Operand::Type(_)) {
                                fields.push((n.as_str(), o));
                            }
                        }
                        return panic_fields("assert", msg, fields);
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
                        *pc = to as usize;
                    }
                }
                Op::Tuple { dst, list } => {
                    let v = Value::Tuple(
                        code.lists[list as usize]
                            .iter()
                            .map(|o| self.val(code, base, *o).clone())
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
                            .map(|(n, o)| (n.clone(), self.val(code, base, *o).clone()))
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
                            .map(|o| self.val(code, base, *o).clone())
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
                if let Some((l, depth, ..)) = &mut self.limits {
                    *depth += 1;
                    if *depth > l.depth {
                        return Err(crate::Stop::Limit("depth"));
                    }
                }
                self.tick()?;
                self.enter(f, clo.as_deref(), new_base, nargs);
                let r = self.exec(f, new_base)?;
                if let Some((_, depth, ..)) = &mut self.limits {
                    *depth -= 1;
                }
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
            Target::Native
                if let Some((id, b)) = s.id.and_then(|id| Some((id, bind::get(id)?))) =>
            {
                let r = match b {
                    Bind::Plain(f) => Ok(f(&mut self.stack[new_base..])?),
                    Bind::Cap(f) => Ok(f(&mut self.it, &mut self.stack[new_base..])?),
                    Bind::CapFallible(f) => f(&mut self.it, &mut self.stack[new_base..])?,
                    Bind::Fallible(f) => f(&self.stack[new_base..])?,
                    Bind::Vm(f) => {
                        // Taken off the stack: closures the function calls run on it.
                        let mut args: Vec<Value> =
                            self.stack[new_base..].iter_mut().map(mem::take).collect();
                        let r = f(self, id, &mut args)?;
                        for (i, v) in args.into_iter().enumerate() {
                            self.stack[new_base + i] = v;
                        }
                        Ok(r)
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
            Target::Native => {
                let r = match intrinsic(&s.name, &self.stack[new_base..]) {
                    Some(r) => Ok(r?),
                    None => return unsupported(format!("call of `{}`", s.name)),
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
                    let at = usize::try_from(*i).ok();
                    let (got, len) = match xs {
                        Value::List(xs) | Value::SortedSet(xs) => {
                            (at.and_then(|i| xs.get(i)), xs.len())
                        }
                        Value::Deque(d, _) => (at.and_then(|i| d.get(i)), d.len()),
                        _ => unreachable!("matched"),
                    };
                    match got {
                        Some(v) => v,
                        None => return out_of_range(*i, len),
                    }
                }
                (Proj_::Index(i), Value::Bytes(b)) => {
                    return index_byte(b, self.val(code, base, *i));
                }
                (Proj_::Index(i), Value::ByteArray(b)) => {
                    return index_byte(b, self.val(code, base, *i));
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
        let at = |o: &Opnd| match o.konst() {
            Some(k) => &code.consts[k],
            None => &self.stack[base + o.idx()],
        };
        // A byte of a `bytearray` is no `Value` slot: the last projection stores into it.
        let r = match p.projs.split_last() {
            Some((Proj_::Index(i), init)) => walk_mut(&mut root, init, at).and_then(|s| match s {
                Value::ByteArray(b) => {
                    let (Value::Int(i), Value::Int(x)) = (at(i), &v) else {
                        unreachable!("checked")
                    };
                    let len = b.len();
                    match usize::try_from(*i).ok().and_then(|i| b.get_mut(i)) {
                        Some(c) => {
                            *c = *x as u8;
                            Ok(())
                        }
                        None => out_of_range(*i, len),
                    }
                }
                s => walk_mut(s, &p.projs[init.len()..], at).map(|slot| *slot = v.clone()),
            }),
            _ => walk_mut(&mut root, &p.projs, at).map(|slot| *slot = v),
        };
        self.stack[base + p.root as usize] = root;
        r
    }
}

fn index_byte(b: &[u8], i: &Value) -> R<Value> {
    let Value::Int(i) = i else {
        return unsupported("non-integer index");
    };
    match usize::try_from(*i).ok().and_then(|i| b.get(i)) {
        Some(c) => Ok(Value::Int(*c as i128)),
        None => out_of_range(*i, b.len()),
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
                let at = usize::try_from(*i).ok();
                let len = match xs {
                    Value::List(xs) => xs.len(),
                    Value::Deque(d, _) => d.len(),
                    _ => unreachable!("matched"),
                };
                let got = match xs {
                    Value::List(xs) => at.and_then(|i| xs.get_mut(i)),
                    Value::Deque(d, _) => at.and_then(|i| d.get_mut(i)),
                    _ => unreachable!("matched"),
                };
                match got {
                    Some(s) => s,
                    None => return out_of_range(*i, len),
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
