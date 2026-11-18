//! The checked machine of 6.1 (D1 of the `run --ub` design): executes `yan-mir` over abstract
//! bytes. Locals are scalars; memory is allocations of `Uninit | Init` bytes; a pointer is its
//! address. Layouts and drops come from MIR, as for native code. With `ub` set, an access to a
//! dead allocation is `ub(2)`, one outside every allocation is `ub(3)`, a word access at an address that is not a multiple of 8 is `ub(4)` and a `bool` read from a
//! word other than 0 or 1 is `ub(6)`, a capability read from memory is `ub(9)`, a block freed twice is `ub(8)`, a write to a `Str` literal is `ub(7)` (6.7 items; D3 of the design). Without it an access outside
//! any allocation is a fault, everything else behaves as native code does. Bounds are judged by
//! the accessed address, not by a poison bit on the pointer value (D4), so a pointer that left
//! its allocation and came back is not caught.
//! Whatever MIR construct or runtime function it does not model is reported, never approximated.
//! Tasks, supervisors and clusters are yan-rt's own thread-local state, so the machine makes the
//! scheduling decisions native code makes; a run owns its thread.

mod ffi;
mod mem;
mod ops;
mod race;
mod rt;

use mem::{Byte, Memory};
use race::{Kind, Race};
use std::cell::RefCell;
use std::collections::HashMap;
use yan_mir::{
    ArmEvent, AtomicOp, BlockId, Func, FuncId, Inst, Loc, Local, MemOrder, Operand, Program, Term,
    Ty,
};

/// Why a run stopped before `main` returned.
#[derive(Debug)]
pub enum Stop {
    Panic(&'static str),
    /// `main` failed: the message for stderr and the exit status.
    Fail(String, i64),
    Unsupported(String),
    Fault(String),
    /// A 6.7 item violated under `--ub`.
    Ub(u8, String),
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
pub(crate) const FUNC_TAG: i128 = 1 << 62;

struct Frame {
    locals: Vec<Val>,
    /// Address-taken locals live in memory: their allocation base and word.
    pinned: HashMap<Local, (u64, u64)>,
    /// `par` regions open when the frame began; any still open when it leaves are joined.
    regions: usize,
    /// Locals whose value moved out, with the move's block and instruction (under `--ub`).
    moved: HashMap<Local, (BlockId, usize)>,
    /// The block and instruction being run.
    at: (BlockId, usize),
}

pub struct Machine<'p> {
    prog: &'p Program,
    ub: bool,
    mem: Memory,
    strs: Vec<Option<u64>>,
    pub(crate) lists: HashMap<u64, rt::List>,
    pub(crate) maps: HashMap<u64, rt::Map>,
    /// `bytes` and `bytearray` contents by handle block; handles stay valid as they grow.
    bytes: HashMap<u64, Vec<u8>>,
    exposed: HashMap<u64, i128>,
    /// `arena` handle blocks to the slot blocks they own.
    arenas: HashMap<u64, Vec<u64>>,
    scratch: Option<u64>,
    civil_out: Vec<i64>,
    /// `fake_fs` file systems by handler minus one; handler 0 is the real one.
    fs_fakes: Vec<HashMap<String, String>>,
    fs_reason: String,
    race: RefCell<Race>,
    /// Where each moved-from word of memory was moved, by address (under `--ub`).
    move_sites: HashMap<u64, String>,
    /// The seed of the run's nondeterministic choices (R1101).
    #[expect(dead_code, reason = "read by the store-buffer loads of RFC 0090 D3")]
    seed: u64,
    /// The words an atomic instruction accessed (under `--ub`), for `ub(15)`.
    atomic_words: std::collections::HashSet<u64>,
}

/// An allocation still live when `main` returned (the leak report of 6.9; leaks are not UB).
pub struct Leak {
    pub size: usize,
    /// The function that was running when it was allocated.
    pub site: String,
}

/// Runs `main`; on return, the allocations still live, string literals and runtime-owned blocks
/// aside.
pub fn run(prog: &Program, ub: bool) -> Result<Vec<Leak>, Stop> {
    run_seeded(prog, ub, 0)
}

/// [`run`] with the seed of the run's nondeterministic choices (R1101).
pub fn run_seeded(prog: &Program, ub: bool, seed: u64) -> Result<Vec<Leak>, Stop> {
    let mut m = Machine {
        prog,
        ub,
        seed,
        mem: Memory::new(),
        strs: vec![None; prog.strings.len()],
        lists: HashMap::new(),
        maps: HashMap::new(),
        bytes: HashMap::new(),
        exposed: HashMap::new(),
        arenas: HashMap::new(),
        scratch: None,
        civil_out: Vec::new(),
        fs_fakes: Vec::new(),
        fs_reason: String::new(),
        race: RefCell::new(Race::new()),
        move_sites: HashMap::new(),
        atomic_words: std::collections::HashSet::new(),
    };
    yan_rt::actor::defer_disposal();
    yan_rt::actor::defer_stopped_text();
    m.call(prog.main, vec![])?;
    yan_rt::actor::end_root();
    m.deliver_stopped();
    m.dispose()?;
    Ok(m.mem
        .live(|a| m.strs.contains(&Some(a)) || m.scratch == Some(a))
        .into_iter()
        .map(|(size, site)| Leak {
            size,
            site: prog.funcs[site as usize].name.clone(),
        })
        .collect())
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
    /// A failed `requires` of an `unsafe fn` under `--ub`: 6.7 item 16, naming the function and
    /// the clause by its position among its `requires` and `ensures` (R882).
    fn precondition_ub(&self, f: &Func, fields: &[(Operand, Operand)]) -> Stop {
        let clause = fields.iter().find_map(|(k, v)| match (k, v) {
            (Operand::Str(k), Operand::Int(c)) if self.prog.strings[*k as usize] == "clause" => {
                Some(*c)
            }
            _ => None,
        });
        let at = clause.map_or(String::new(), |c| format!(", clause {c}"));
        Stop::Ub(
            16,
            format!(
                "a call to `unsafe fn {}` breaks its precondition{at}",
                f.name
            ),
        )
    }

    /// The failure of an 8-byte access at `addr` that no allocation serves.
    fn bad_access(&self, what: &str, addr: u64) -> Stop {
        if !self.ub {
            return Stop::Fault(format!(
                "{what} of 8 bytes at {addr:#x} outside any allocation"
            ));
        }
        match self.mem.find(addr) {
            Some(i) if !self.mem.is_live(i) => Stop::Ub(
                2,
                format!("{what} of 8 bytes at {addr:#x} through a freed allocation"),
            ),
            _ => Stop::Ub(
                3,
                format!("{what} of 8 bytes at {addr:#x} outside its allocation"),
            ),
        }
    }

    /// A word access at an address that is not a multiple of 8 is `ub(4)`.
    fn aligned(&self, what: &str, addr: u64) -> Result<(), Stop> {
        if self.ub && !addr.is_multiple_of(8) {
            return Err(Stop::Ub(
                4,
                format!("{what} of 8 bytes at misaligned address {addr:#x}"),
            ));
        }
        Ok(())
    }

    /// `Atomic.wait` on `addr` is `ub(15)` unless it is 4-aligned, inside a live allocation and a
    /// word an atomic instruction accessed. Returns whether the word equals `expected`.
    fn wait_word(&self, addr: u64, expected: u64) -> Result<bool, Stop> {
        if self.ub {
            let why = if !addr.is_multiple_of(4) {
                Some("is not 4-aligned")
            } else if self.mem.read(addr, 4).is_none() {
                Some("is not inside a live allocation")
            } else if !self.atomic_words.contains(&addr) {
                Some("is not a word accessed atomically")
            } else {
                None
            };
            if let Some(why) = why {
                return Err(Stop::Ub(
                    15,
                    format!("`Atomic.wait` on {addr:#x}, which {why}"),
                ));
            }
        }
        let w = self.read_word_with(addr, false)?;
        Ok(w as u32 == expected as u32)
    }

    fn typed(&self, t: Ty, w: u64) -> Result<Val, Stop> {
        if self.ub && t == Ty::Bool && w > 1 {
            return Err(Stop::Ub(6, format!("{w} is not a valid bool")));
        }
        Ok(decode(t, w))
    }

    fn raced(&self, addr: u64, write: bool) -> Result<(), Stop> {
        if !self.ub {
            return Ok(());
        }
        match self.race.borrow_mut().access(addr, write) {
            Some(msg) => Err(Stop::Ub(1, msg)),
            None => Ok(()),
        }
    }

    fn atomic_raced(&self, addr: u64, kind: Kind, order: MemOrder) -> Result<(), Stop> {
        if !self.ub {
            return Ok(());
        }
        match self.race.borrow_mut().atomic(addr, kind, order) {
            Some(msg) => Err(Stop::Ub(1, msg)),
            None => Ok(()),
        }
    }

    pub(crate) fn read_word(&self, addr: u64) -> Result<u64, Stop> {
        self.read_word_with(addr, true)
    }

    fn read_word_with(&self, addr: u64, race: bool) -> Result<u64, Stop> {
        self.aligned("read", addr)?;
        let Some(bytes) = self.mem.read(addr, 8) else {
            return Err(self.bad_access("read", addr));
        };
        if race {
            self.raced(addr, false)?;
        }
        if self.ub && bytes.contains(&Byte::Moved) {
            let site = self.move_sites.get(&addr).map_or("", String::as_str);
            return Err(Stop::Ub(
                8,
                format!("read of the moved-from word at {addr:#x}{site}"),
            ));
        }
        let mut w = [0u8; 8];
        for (d, b) in w.iter_mut().zip(bytes) {
            *d = match b {
                Byte::Init(x) => x,
                Byte::Uninit | Byte::Moved => 0,
            };
        }
        Ok(u64::from_le_bytes(w))
    }

    pub(crate) fn write_word(&mut self, addr: u64, w: u64) -> Result<(), Stop> {
        self.write_word_with(addr, w, true)
    }

    fn write_word_with(&mut self, addr: u64, w: u64, race: bool) -> Result<(), Stop> {
        self.aligned("write", addr)?;
        if self.ub && self.mem.is_immutable(addr) {
            return Err(Stop::Ub(
                7,
                format!("write at {addr:#x} to immutable memory"),
            ));
        }
        if race && self.mem.find(addr).is_some() {
            self.raced(addr, true)?;
        }
        let bytes: Vec<Byte> = w.to_le_bytes().into_iter().map(Byte::Init).collect();
        self.mem
            .write(addr, &bytes)
            .ok_or_else(|| self.bad_access("write", addr))
    }

    fn str_lit(&mut self, i: u32) -> u64 {
        if let Some(a) = self.strs[i as usize] {
            return a;
        }
        let prog = self.prog;
        let a = self.new_str(&prog.strings[i as usize]);
        self.mem.freeze(a);
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
        if let Some((b, i)) = fr.moved.get(&l) {
            return Err(Stop::Ub(
                8,
                format!(
                    "use of a moved-from local in `{}`, moved at block {b}, instruction {i}",
                    f.name
                ),
            ));
        }
        match fr.pinned.get(&l) {
            Some((base, word)) => {
                self.typed(f.locals[l as usize], self.read_word(base + 8 * word)?)
            }
            None => Ok(fr.locals[l as usize]),
        }
    }

    fn set(&mut self, fr: &mut Frame, f: &Func, l: Local, v: Val) -> Result<(), Stop> {
        let v = match (f.locals[l as usize], v) {
            (Ty::Int(_), Val::B(b)) => Val::I(i128::from(b)),
            (Ty::Bool, Val::I(i)) => Val::B(i != 0),
            (_, v) => v,
        };
        fr.moved.remove(&l);
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
            regions: self.race.borrow().depth(),
            moved: HashMap::new(),
            at: (0, 0),
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
        self.race.borrow_mut().unwind(fr.regions);
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
                for (i, inst) in b.insts.iter().enumerate() {
                    fr.at = (block, i);
                    self.mem.site = func;
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
                    Term::Panic("forged") if self.ub => {
                        return Err(Stop::Ub(
                            9,
                            "a capability read from memory is forged".into(),
                        ));
                    }
                    Term::Panic(kind) => return Err(Stop::Panic(kind)),
                    Term::PanicFields { kind, fields } => {
                        return Err(match self.ub && f.unsafe_fn && *kind == "contract" {
                            true => self.precondition_ub(f, fields),
                            false => Stop::Panic(kind),
                        });
                    }
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
                self.dispose()?;
                match dst {
                    Some(d) => self.set(fr, f, *d, v),
                    None => Ok(()),
                }
            }
            Inst::Load { dst, ptr, word } => {
                let p = self.eval(fr, ptr, f)?.addr();
                let w = self.read_word(p + 8 * u64::from(*word))?;
                let v = self.typed(f.locals[*dst as usize], w)?;
                self.set(fr, f, *dst, v)
            }
            Inst::Store { ptr, word, src } => {
                let p = self.eval(fr, ptr, f)?.addr();
                let v = self.eval(fr, src, f)?;
                self.write_word(p + 8 * u64::from(*word), encode(v))
            }
            Inst::Atomic {
                dst,
                op,
                ty,
                args,
                ptr,
                order,
                failure,
            } => {
                // One step, so no task interleaves. The word is shadowed as one atomic access.
                let p = self.eval(fr, ptr, f)?.addr();
                let a = args
                    .iter()
                    .map(|o| self.eval(fr, o, f).map(encode))
                    .collect::<Result<Vec<u64>, Stop>>()?;
                let old = self.read_word_with(p, false)?;
                if self.ub {
                    self.atomic_words.insert(p);
                }
                let new = ops::atomic_new(*op, *ty, old, &a);
                if let Some(w) = new {
                    self.write_word_with(p, w, false)?;
                }
                let (kind, order) = match (op, new) {
                    (AtomicOp::Load, _) => (Kind::Read, order),
                    (AtomicOp::Store, _) => (Kind::Write, order),
                    (AtomicOp::Cas, None) => (Kind::Read, failure),
                    _ => (Kind::Rmw, order),
                };
                self.atomic_raced(p, kind, *order)?;
                match dst {
                    Some(d) => {
                        let v = self.typed(f.locals[*d as usize], old)?;
                        self.set(fr, f, *d, v)
                    }
                    None => Ok(()),
                }
            }
            Inst::Fence { order } => {
                if self.ub {
                    self.race.borrow_mut().fence(*order);
                }
                Ok(())
            }
            Inst::Move { places } if self.ub => {
                for place in places {
                    match place {
                        Loc::Local(l) => {
                            fr.moved.insert(*l, fr.at);
                        }
                        Loc::Word { ptr, word } => {
                            let addr = self.eval(fr, ptr, f)?.addr() + 8 * u64::from(*word);
                            if self.mem.mark_moved(addr, 8).is_some() {
                                let (b, i) = fr.at;
                                self.move_sites.insert(
                                    addr,
                                    format!(
                                        ", moved in `{}` at block {b}, instruction {i}",
                                        f.name
                                    ),
                                );
                            }
                        }
                    }
                }
                Ok(())
            }
            Inst::Move { .. } => Ok(()),
            Inst::Arm(e) => {
                let mut r = self.race.borrow_mut();
                match e {
                    ArmEvent::Fork => r.fork(),
                    ArmEvent::Enter(k) => r.enter(*k),
                    ArmEvent::Exit => r.exit(),
                    ArmEvent::Join => r.join(),
                    ArmEvent::Leave => {}
                }
                Ok(())
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
