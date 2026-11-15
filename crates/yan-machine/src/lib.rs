//! The checked machine of 6.1 (D1 of the `run --ub` design): executes `yan-mir` over abstract
//! bytes. Locals are scalars; memory is allocations of `Uninit | Init` bytes; a pointer is its
//! address. Layouts and drops come from MIR, as for native code. Checks (`--ub`) are not here yet:
//! an access outside any allocation is a fault, everything else behaves as native code does.
//! Whatever MIR construct or runtime function it does not model is reported, never approximated.

mod mem;
mod ops;
mod rt;

use mem::{Byte, Memory};
use std::collections::HashMap;
use yan_mir::{BlockId, Func, FuncId, Inst, Local, Operand, Program, Term, Ty};

/// Why a run stopped before `main` returned.
#[derive(Debug)]
pub enum Stop {
    Panic(&'static str),
    /// `main` failed: the message for stderr and the exit status.
    Fail(String, i64),
    Unsupported(String),
    Fault(String),
}

pub fn unsupported<T>(what: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::Unsupported(what.into()))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    I(i128),
    F(f64),
    B(bool),
    U,
}

impl Val {
    pub fn int(self) -> i128 {
        match self {
            Val::I(i) => i,
            Val::B(b) => i128::from(b),
            Val::F(_) | Val::U => 0,
        }
    }
    pub fn addr(self) -> u64 {
        self.int() as u64
    }
    pub fn float(self) -> f64 {
        match self {
            Val::F(f) => f,
            _ => 0.0,
        }
    }
    pub fn truth(self) -> bool {
        self.int() != 0
    }
}

/// Function code addresses are tagged so they never alias a data address.
const FUNC_TAG: i128 = 1 << 62;

struct Frame {
    locals: Vec<Val>,
    /// Address-taken locals live in memory: their allocation base and word.
    pinned: HashMap<Local, (u64, u64)>,
}

pub struct Machine<'p> {
    prog: &'p Program,
    mem: Memory,
    strs: Vec<Option<u64>>,
    pub(crate) lists: HashMap<u64, rt::List>,
    exposed: HashMap<u64, i128>,
}

pub fn run(prog: &Program) -> Result<(), Stop> {
    let mut m = Machine {
        prog,
        mem: Memory::new(),
        strs: vec![None; prog.strings.len()],
        lists: HashMap::new(),
        exposed: HashMap::new(),
    };
    m.call(prog.main, vec![]).map(drop)
}

fn encode(v: Val) -> u64 {
    match v {
        Val::I(i) => i as u64,
        Val::F(f) => f.to_bits(),
        Val::B(b) => u64::from(b),
        Val::U => 0,
    }
}

fn decode(t: Ty, w: u64) -> Val {
    match t {
        Ty::Int(it) if it.signed || it.bits < 64 => Val::I(ops::wrap(it, i128::from(w as i64))),
        Ty::Int(_) => Val::I(i128::from(w)),
        Ty::F64 => Val::F(f64::from_bits(w)),
        Ty::Bool => Val::B(w != 0),
        Ty::Ptr => Val::I(i128::from(w)),
        Ty::Unit => Val::U,
    }
}

impl Machine<'_> {
    pub(crate) fn read_word(&self, addr: u64) -> Result<u64, Stop> {
        let Some(bytes) = self.mem.read(addr, 8) else {
            return Err(Stop::Fault(format!(
                "read of 8 bytes at {addr:#x} outside any allocation"
            )));
        };
        let mut w = [0u8; 8];
        for (d, b) in w.iter_mut().zip(bytes) {
            *d = match b {
                Byte::Init(x) => x,
                Byte::Uninit => 0,
            };
        }
        Ok(u64::from_le_bytes(w))
    }

    pub(crate) fn write_word(&mut self, addr: u64, w: u64) -> Result<(), Stop> {
        let bytes: Vec<Byte> = w.to_le_bytes().into_iter().map(Byte::Init).collect();
        self.mem.write(addr, &bytes).ok_or_else(|| {
            Stop::Fault(format!(
                "write of 8 bytes at {addr:#x} outside any allocation"
            ))
        })
    }

    fn str_lit(&mut self, i: u32) -> u64 {
        if let Some(a) = self.strs[i as usize] {
            return a;
        }
        let prog = self.prog;
        let a = self.new_str(&prog.strings[i as usize]);
        self.strs[i as usize] = Some(a);
        a
    }

    fn eval(&mut self, fr: &Frame, o: &Operand, f: &Func) -> Result<Val, Stop> {
        Ok(match o {
            Operand::Local(l) => self.get(fr, f, *l)?,
            Operand::Int(i) => Val::I(*i),
            Operand::F64(x) => Val::F(*x),
            Operand::Bool(b) => Val::B(*b),
            Operand::Unit => Val::U,
            Operand::Str(i) => Val::I(i128::from(self.str_lit(*i))),
            Operand::Func(id) => Val::I(FUNC_TAG + i128::from(*id)),
        })
    }

    fn evals(&mut self, fr: &Frame, os: &[Operand], f: &Func) -> Result<Vec<Val>, Stop> {
        os.iter().map(|o| self.eval(fr, o, f)).collect()
    }

    fn get(&self, fr: &Frame, f: &Func, l: Local) -> Result<Val, Stop> {
        match fr.pinned.get(&l) {
            Some((base, word)) => Ok(decode(
                f.locals[l as usize],
                self.read_word(base + 8 * word)?,
            )),
            None => Ok(fr.locals[l as usize]),
        }
    }

    fn set(&mut self, fr: &mut Frame, f: &Func, l: Local, v: Val) -> Result<(), Stop> {
        let v = match (f.locals[l as usize], v) {
            (Ty::Int(_), Val::B(b)) => Val::I(i128::from(b)),
            (Ty::Bool, Val::I(i)) => Val::B(i != 0),
            (_, v) => v,
        };
        match fr.pinned.get(&l) {
            Some((base, word)) => self.write_word(base + 8 * word, encode(v)),
            None => {
                fr.locals[l as usize] = v;
                Ok(())
            }
        }
    }

    fn frame(&mut self, f: &Func, args: &[Val]) -> Result<Frame, Stop> {
        let mut fr = Frame {
            locals: f.locals.iter().map(|t| decode(*t, 0)).collect(),
            pinned: HashMap::new(),
        };
        for group in &f.pinned {
            let base = self.mem.alloc(8 * group.len());
            for (w, l) in group.iter().enumerate() {
                fr.pinned.insert(*l, (base, w as u64));
            }
        }
        for (i, a) in args.iter().enumerate().take(f.params as usize) {
            self.set(&mut fr, f, i as Local, *a)?;
        }
        Ok(fr)
    }

    fn release(&mut self, fr: &Frame) {
        for (base, _) in fr.pinned.values() {
            self.mem.free(*base);
        }
    }

    pub fn call(&mut self, func: FuncId, args: Vec<Val>) -> Result<Vec<Val>, Stop> {
        let (mut func, mut args) = (func, args);
        'call: loop {
            let prog = self.prog;
            let f = &prog.funcs[func as usize];
            let mut fr = self.frame(f, &args)?;
            let mut block: BlockId = 0;
            loop {
                let b = &f.blocks[block as usize];
                for inst in &b.insts {
                    self.inst(&mut fr, f, inst)?;
                }
                match &b.term {
                    Term::Jump(t) => block = *t,
                    Term::Branch { cond, then, els } => {
                        block = if self.eval(&fr, cond, f)?.truth() {
                            *then
                        } else {
                            *els
                        };
                    }
                    Term::Switch { on, targets } => {
                        let i = self.get(&fr, f, *on)?.int();
                        let Some(t) = usize::try_from(i).ok().and_then(|i| targets.get(i)) else {
                            return Err(Stop::Panic("unreachable"));
                        };
                        block = *t;
                    }
                    Term::Return(os) => {
                        let out = self.evals(&fr, os, f)?;
                        self.release(&fr);
                        return Ok(out);
                    }
                    Term::TailCall { func: g, args: a } => {
                        args = self.evals(&fr, a, f)?;
                        self.release(&fr);
                        func = *g;
                        continue 'call;
                    }
                    Term::Panic(kind) => return Err(Stop::Panic(kind)),
                    Term::PanicFields { kind, .. } => return Err(Stop::Panic(kind)),
                    Term::Par { .. }
                    | Term::Suspend { .. }
                    | Term::TaskEnd
                    | Term::Unwind { .. } => {
                        return unsupported("tasks in the machine");
                    }
                }
            }
        }
    }

    fn inst(&mut self, fr: &mut Frame, f: &Func, inst: &Inst) -> Result<(), Stop> {
        match inst {
            Inst::Copy { dst, src } => {
                let v = self.eval(fr, src, f)?;
                self.set(fr, f, *dst, v)
            }
            Inst::Scalar { dst, op, ty, args } => {
                let a = self.evals(fr, args, f)?;
                let v = ops::scalar(*op, *ty, &a)?;
                match dst {
                    Some(d) => self.set(fr, f, *d, v),
                    None => Ok(()),
                }
            }
            Inst::Call { dst, func, args } => {
                let a = self.evals(fr, args, f)?;
                let out = self.call(*func, a)?;
                self.bind(fr, f, dst, out)
            }
            Inst::CallIndirect {
                dst, code, args, ..
            } => {
                let c = self.eval(fr, code, f)?.int();
                let a = self.evals(fr, args, f)?;
                let Ok(id) = FuncId::try_from(c - FUNC_TAG) else {
                    return Err(Stop::Fault(format!(
                        "call of {c:#x}, which is not a function"
                    )));
                };
                let out = self.call(id, a)?;
                self.bind(fr, f, dst, out)
            }
            Inst::Rt { dst, f: rt, args } => {
                let a = self.evals(fr, args, f)?;
                let v = self.rt(*rt, &a)?;
                match dst {
                    Some(d) => self.set(fr, f, *d, v),
                    None => Ok(()),
                }
            }
            Inst::Load { dst, ptr, word } => {
                let p = self.eval(fr, ptr, f)?.addr();
                let w = self.read_word(p + 8 * u64::from(*word))?;
                let v = decode(f.locals[*dst as usize], w);
                self.set(fr, f, *dst, v)
            }
            Inst::Store { ptr, word, src } => {
                let p = self.eval(fr, ptr, f)?.addr();
                let v = self.eval(fr, src, f)?;
                self.write_word(p + 8 * u64::from(*word), encode(v))
            }
            Inst::AddrOf { dst, local } => {
                let Some((base, word)) = fr.pinned.get(local) else {
                    return Err(Stop::Fault("address of a local that is not pinned".into()));
                };
                let a = base + 8 * word;
                self.set(fr, f, *dst, Val::I(i128::from(a)))
            }
        }
    }

    fn bind(&mut self, fr: &mut Frame, f: &Func, dst: &[Local], out: Vec<Val>) -> Result<(), Stop> {
        for (d, v) in dst.iter().zip(out) {
            self.set(fr, f, *d, v)?;
        }
        Ok(())
    }
}
