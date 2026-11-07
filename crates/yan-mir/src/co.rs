//! Coroutine variants (RFC 0043 D1 b): a function that may block, called from a task, runs as
//! `g$co(frame) -> status` over a heap frame, so a wait inside it suspends the task. Status 0:
//! done, with the results in the frame; 1: suspended, call again to resume.
//!
//! Frame words: the state, then `g`'s result slots, then one slot per local of `g$co` (local 0
//! is the frame itself and has none). A parameter is the local after the frame, so the caller
//! stores the arguments in their slots and state 0 loads them. A suspension stores the locals
//! live at its resume point, and resuming loads them back.

use std::collections::HashSet;

use crate::tasks::{Walk, walk};
use crate::*;

const I64: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

/// Make the coroutine variants, route calls from tasks to them, then remove the task terms.
pub fn run(p: &mut Program) {
    let walks: Vec<Walk> = p.funcs.iter().map(walk).collect();
    let may = may_suspend(p, &walks);
    let n = p.funcs.len();
    let mut co_id: Vec<Option<FuncId>> = vec![None; n];
    let mut next = n as FuncId;
    for (g, m) in may.iter().enumerate() {
        if *m {
            co_id[g] = Some(next);
            next += 1;
        }
    }
    let nret: Vec<usize> = p.funcs.iter().map(|f| f.ret.len()).collect();
    let mut cx = Cx {
        co_id,
        nret,
        patches: Vec::new(),
    };

    let mut cos = Vec::new();
    for g in 0..n {
        if may[g] {
            let at = n + cos.len();
            cos.push(cx.make_co(&p.funcs[g], &walks[g], at));
        }
    }
    for (g, w) in walks.iter().enumerate() {
        let f = &mut p.funcs[g];
        let tasks: Vec<BlockId> = (0..f.blocks.len() as BlockId)
            .filter(|b| w.seen[*b as usize] && w.owner[*b as usize].is_some())
            .collect();
        let ret = f.ret.clone();
        for b in &tasks {
            cx.untail(f, *b, true, &ret);
        }
        cx.co_calls(f, g, tasks);
    }
    p.funcs.extend(cos);

    // The frame sizes are known once every variant is built.
    for (f, b, i, h) in std::mem::take(&mut cx.patches) {
        let size = p.funcs[cx.co_id[h].expect("a variant") as usize]
            .locals
            .len()
            + cx.nret[h];
        let Inst::Rt { args, .. } = &mut p.funcs[f].blocks[b as usize].insts[i] else {
            unreachable!("a frame allocation")
        };
        args[0] = Operand::Int(size as i128);
    }
    for f in &mut p.funcs[..n] {
        let ret = f.ret.clone();
        tasks::split(f, false, &ret);
    }
}

fn blocking(i: &Inst) -> bool {
    matches!(
        i,
        Inst::Rt {
            f: RtFn::ChanSend | RtFn::ChanRecv,
            ..
        }
    )
}

/// The functions that may block, in their own `par concurrent` blocks too: a block whose tasks
/// are all parked suspends the task it runs in.
fn may_suspend(p: &Program, walks: &[Walk]) -> Vec<bool> {
    let mut may = vec![false; p.funcs.len()];
    loop {
        let mut changed = false;
        for (g, f) in p.funcs.iter().enumerate() {
            if may[g] {
                continue;
            }
            let hit = f.blocks.iter().enumerate().any(|(b, blk)| {
                walks[g].seen[b]
                    && (blk.insts.iter().any(|i| {
                        blocking(i) || matches!(i, Inst::Call { func, .. } if may[*func as usize])
                    }) || matches!(blk.term, Term::TailCall { func, .. } if may[func as usize]))
            });
            if hit {
                may[g] = true;
                changed = true;
            }
        }
        if !changed {
            return may;
        }
    }
}

struct Cx {
    co_id: Vec<Option<FuncId>>,
    nret: Vec<usize>,
    /// Frame allocations to size: (function, block, instruction, callee).
    patches: Vec<(usize, BlockId, usize, usize)>,
}

fn new_local(f: &mut Func, t: Ty) -> Local {
    f.locals.push(t);
    f.locals.len() as Local - 1
}

fn new_block(f: &mut Func, insts: Vec<Inst>, term: Term) -> BlockId {
    let lines = vec![0; insts.len()];
    f.blocks.push(Block { insts, lines, term });
    f.blocks.len() as BlockId - 1
}

impl Cx {
    /// A tail call from `b` becomes a call and a return, so a callee that suspends can resume
    /// into it (always, in a coroutine variant, whose results go to the frame).
    fn untail(&self, f: &mut Func, b: BlockId, always: bool, ret: &[Ty]) {
        let Term::TailCall { func, args } = f.blocks[b as usize].term.clone() else {
            return;
        };
        if !always && self.co_id[func as usize].is_none() {
            return;
        }
        let dst: Vec<Local> = ret.iter().map(|t| new_local(f, *t)).collect();
        let blk = &mut f.blocks[b as usize];
        blk.insts.push(Inst::Call {
            dst: dst.clone(),
            func,
            args,
        });
        blk.lines.push(0);
        blk.term = Term::Return(dst.into_iter().map(Operand::Local).collect());
    }

    /// Calls in `blocks` (and the blocks split from them) of functions that may suspend become
    /// calls of their variants, suspending while the callee is suspended.
    fn co_calls(&mut self, f: &mut Func, me: usize, mut blocks: Vec<BlockId>) {
        while let Some(b) = blocks.pop() {
            let Some(i) = f.blocks[b as usize].insts.iter().position(
                |i| matches!(i, Inst::Call { func, .. } if self.co_id[*func as usize].is_some()),
            ) else {
                continue;
            };
            let blk = &mut f.blocks[b as usize];
            let rest = blk.insts.split_off(i + 1);
            let rest_lines = blk.lines.split_off(i + 1);
            let Some(Inst::Call { dst, func, args }) = blk.insts.pop() else {
                unreachable!("the call")
            };
            let line = blk.lines.pop().unwrap_or(0);
            let term = std::mem::replace(&mut blk.term, Term::Panic("unreachable"));
            let h = func as usize;
            let nret = self.nret[h];

            let fr = new_local(f, Ty::Ptr);
            let st = new_local(f, I64);
            let mut pre = vec![
                Inst::Rt {
                    dst: Some(fr),
                    f: RtFn::Alloc,
                    args: vec![Operand::Int(0)],
                },
                Inst::Store {
                    ptr: Operand::Local(fr),
                    word: 0,
                    src: Operand::Int(0),
                },
            ];
            for (k, a) in args.into_iter().enumerate() {
                pre.push(Inst::Store {
                    ptr: Operand::Local(fr),
                    word: (nret + k + 1) as u32,
                    src: a,
                });
            }
            let blk = &mut f.blocks[b as usize];
            self.patches.push((me, b, blk.insts.len(), h));
            blk.lines.extend(std::iter::repeat_n(line, pre.len()));
            blk.insts.extend(pre);

            let mut post: Vec<Inst> = dst
                .iter()
                .enumerate()
                .map(|(j, d)| Inst::Load {
                    dst: *d,
                    ptr: Operand::Local(fr),
                    word: (j + 1) as u32,
                })
                .collect();
            post.push(Inst::Rt {
                dst: None,
                f: RtFn::EnvFree,
                args: vec![Operand::Local(fr)],
            });
            let n_post = post.len();
            post.extend(rest);
            let done = new_block(f, post, term);
            let blk = &mut f.blocks[done as usize];
            blk.lines = std::iter::repeat_n(line, n_post)
                .chain(rest_lines)
                .collect();
            let call = new_block(
                f,
                vec![Inst::Call {
                    dst: vec![st],
                    func: self.co_id[h].expect("a variant"),
                    args: vec![Operand::Local(fr)],
                }],
                Term::Panic("unreachable"),
            );
            f.blocks[call as usize].lines = vec![line];
            let suspend = new_block(f, Vec::new(), Term::Suspend { resume: call });
            f.blocks[call as usize].term = Term::Switch {
                on: st,
                targets: vec![done, suspend],
            };
            f.blocks[b as usize].term = Term::Jump(call);
            blocks.push(done);
        }
    }

    /// `g$co`, as function `at`, from `g` before its task transform.
    fn make_co(&mut self, g: &Func, w: &Walk, at: usize) -> Func {
        let mut f = Func {
            name: format!("{}$co", g.name),
            params: 1,
            ret: vec![I64],
            locals: std::iter::once(Ty::Ptr)
                .chain(g.locals.iter().copied())
                .collect(),
            blocks: g.blocks.clone(),
        };
        for b in &mut f.blocks {
            b.insts.iter_mut().for_each(shift_inst);
            shift_term(&mut b.term);
        }
        let reach: Vec<usize> = (0..f.blocks.len()).filter(|b| w.seen[*b]).collect();
        for b in &reach {
            self.untail(&mut f, *b as BlockId, true, &g.ret);
            if w.owner[*b].is_none() {
                for i in &mut f.blocks[*b].insts {
                    if let Inst::Rt {
                        f: RtFn::ChanSend | RtFn::ChanRecv,
                        args,
                        ..
                    } = i
                    {
                        *args.last_mut().expect("in_task") = Operand::Int(1);
                    }
                }
            }
        }
        self.co_calls(&mut f, at, reach.iter().map(|b| *b as BlockId).collect());
        // What `Suspend`s remain suspend the variant.
        tasks::split(&mut f, true, &g.ret);

        let me = self
            .co_id
            .iter()
            .position(|c| *c == Some(at as FuncId))
            .expect("g");
        let nret = self.nret[me];
        let slot = |l: Local| (nret as u32) + l;

        // Results go to the frame, wherever the function returns from.
        for b in 0..f.blocks.len() {
            if let Term::Return(os) = f.blocks[b].term.clone() {
                let blk = &mut f.blocks[b];
                for (j, o) in os.into_iter().enumerate() {
                    blk.insts.push(Inst::Store {
                        ptr: Operand::Local(0),
                        word: (j + 1) as u32,
                        src: o,
                    });
                    blk.lines.push(0);
                }
                blk.term = Term::Return(vec![Operand::Int(0)]);
            }
        }

        let live = live_in(&f);
        let state = new_local(&mut f, I64);
        let entry = new_block(&mut f, Vec::new(), Term::Panic("unreachable"));
        let mut reloads = vec![(0 as BlockId, entry)];
        for b in 0..f.blocks.len() {
            let Term::Suspend { resume } = f.blocks[b].term else {
                continue;
            };
            let idx = reloads.len();
            let blk = &mut f.blocks[b];
            for l in &live[resume as usize] {
                blk.insts.push(Inst::Store {
                    ptr: Operand::Local(0),
                    word: slot(*l),
                    src: Operand::Local(*l),
                });
                blk.lines.push(0);
            }
            blk.insts.push(Inst::Store {
                ptr: Operand::Local(0),
                word: 0,
                src: Operand::Int(idx as i128),
            });
            blk.lines.push(0);
            blk.term = Term::Return(vec![Operand::Int(1)]);
            let r = new_block(&mut f, Vec::new(), Term::Jump(resume));
            reloads.push((resume, r));
        }
        for (target, r) in &reloads {
            let loads: Vec<Inst> = live[*target as usize]
                .iter()
                .map(|l| Inst::Load {
                    dst: *l,
                    ptr: Operand::Local(0),
                    word: slot(*l),
                })
                .collect();
            let blk = &mut f.blocks[*r as usize];
            blk.lines = vec![0; loads.len()];
            blk.insts = loads;
            blk.term = Term::Jump(*target);
        }
        // Block 0 is the entry: swap the dispatcher in, and the old entry out.
        let disp = Block {
            insts: vec![Inst::Load {
                dst: state,
                ptr: Operand::Local(0),
                word: 0,
            }],
            lines: vec![0],
            term: Term::Switch {
                on: state,
                targets: reloads.iter().map(|(_, r)| *r).collect(),
            },
        };
        let old = f.blocks.len() as BlockId;
        f.blocks.push(disp);
        f.blocks.swap(0, old as usize);
        for b in &mut f.blocks {
            swap_targets(&mut b.term, 0, old);
        }
        // Frame allocations recorded in the old entry moved with it.
        for patch in &mut self.patches {
            if patch.0 == at && patch.1 == 0 {
                patch.1 = old;
            }
        }
        f
    }
}

fn succs(t: &Term) -> Vec<BlockId> {
    match t {
        Term::Jump(b) | Term::Suspend { resume: b } => vec![*b],
        Term::Branch { then, els, .. } => vec![*then, *els],
        Term::Switch { targets, .. } => targets.clone(),
        Term::Par { tasks, join } => tasks.iter().copied().chain([*join]).collect(),
        Term::TaskEnd
        | Term::Return(_)
        | Term::TailCall { .. }
        | Term::Panic(_)
        | Term::PanicFields { .. } => Vec::new(),
    }
}

fn swap_targets(t: &mut Term, a: BlockId, b: BlockId) {
    let sw = |x: &mut BlockId| {
        if *x == a {
            *x = b;
        } else if *x == b {
            *x = a;
        }
    };
    match t {
        Term::Jump(x) | Term::Suspend { resume: x } => sw(x),
        Term::Branch { then, els, .. } => {
            sw(then);
            sw(els);
        }
        Term::Switch { targets, .. } => targets.iter_mut().for_each(sw),
        Term::Par { tasks, join } => {
            tasks.iter_mut().for_each(sw);
            sw(join);
        }
        Term::TaskEnd
        | Term::Return(_)
        | Term::TailCall { .. }
        | Term::Panic(_)
        | Term::PanicFields { .. } => {}
    }
}

fn shift_op(o: &mut Operand) {
    if let Operand::Local(l) = o {
        *l += 1;
    }
}

fn shift_inst(i: &mut Inst) {
    let ops: Vec<&mut Operand> = match i {
        Inst::Copy { dst, src } => {
            *dst += 1;
            vec![src]
        }
        Inst::Scalar { dst, args, .. } | Inst::Rt { dst, args, .. } => {
            if let Some(d) = dst {
                *d += 1;
            }
            args.iter_mut().collect()
        }
        Inst::Call { dst, args, .. } => {
            dst.iter_mut().for_each(|d| *d += 1);
            args.iter_mut().collect()
        }
        Inst::CallIndirect {
            dst, code, args, ..
        } => {
            dst.iter_mut().for_each(|d| *d += 1);
            std::iter::once(code).chain(args.iter_mut()).collect()
        }
        Inst::Load { dst, ptr, .. } => {
            *dst += 1;
            vec![ptr]
        }
        Inst::Store { ptr, src, .. } => vec![ptr, src],
    };
    ops.into_iter().for_each(shift_op);
}

fn shift_term(t: &mut Term) {
    match t {
        Term::Branch { cond, .. } => shift_op(cond),
        Term::Switch { on, .. } => *on += 1,
        Term::Return(os) | Term::TailCall { args: os, .. } => os.iter_mut().for_each(shift_op),
        Term::PanicFields { fields, .. } => fields.iter_mut().for_each(|(a, b)| {
            shift_op(a);
            shift_op(b);
        }),
        Term::Jump(_)
        | Term::Par { .. }
        | Term::Suspend { .. }
        | Term::TaskEnd
        | Term::Panic(_) => {}
    }
}

fn uses_defs(i: &Inst) -> (Vec<Local>, Vec<Local>) {
    let l = |o: &Operand| match o {
        Operand::Local(l) => Some(*l),
        _ => None,
    };
    match i {
        Inst::Copy { dst, src } => (l(src).into_iter().collect(), vec![*dst]),
        Inst::Scalar { dst, args, .. } | Inst::Rt { dst, args, .. } => (
            args.iter().filter_map(l).collect(),
            dst.iter().copied().collect(),
        ),
        Inst::Call { dst, args, .. } => (args.iter().filter_map(l).collect(), dst.clone()),
        Inst::CallIndirect {
            dst, code, args, ..
        } => (
            std::iter::once(code).chain(args).filter_map(l).collect(),
            dst.clone(),
        ),
        Inst::Load { dst, ptr, .. } => (l(ptr).into_iter().collect(), vec![*dst]),
        Inst::Store { ptr, src, .. } => {
            ([ptr, src].into_iter().filter_map(l).collect(), Vec::new())
        }
    }
}

fn term_uses(t: &Term) -> Vec<Local> {
    let l = |o: &Operand| match o {
        Operand::Local(l) => Some(*l),
        _ => None,
    };
    match t {
        Term::Branch { cond, .. } => l(cond).into_iter().collect(),
        Term::Switch { on, .. } => vec![*on],
        Term::Return(os) | Term::TailCall { args: os, .. } => os.iter().filter_map(l).collect(),
        Term::PanicFields { fields, .. } => fields
            .iter()
            .flat_map(|(a, b)| [l(a), l(b)])
            .flatten()
            .collect(),
        Term::Jump(_)
        | Term::Par { .. }
        | Term::Suspend { .. }
        | Term::TaskEnd
        | Term::Panic(_) => Vec::new(),
    }
}

/// The locals live on entry to each block, sorted, without the frame (local 0).
fn live_in(f: &Func) -> Vec<Vec<Local>> {
    let n = f.blocks.len();
    let mut gen_kill: Vec<(HashSet<Local>, HashSet<Local>)> = Vec::with_capacity(n);
    for b in &f.blocks {
        let (mut g, mut k) = (HashSet::new(), HashSet::new());
        for i in &b.insts {
            let (u, d) = uses_defs(i);
            g.extend(u.into_iter().filter(|x| !k.contains(x)));
            k.extend(d);
        }
        g.extend(term_uses(&b.term).into_iter().filter(|x| !k.contains(x)));
        gen_kill.push((g, k));
    }
    let mut live: Vec<HashSet<Local>> = vec![HashSet::new(); n];
    loop {
        let mut changed = false;
        for b in (0..n).rev() {
            let mut s: HashSet<Local> = HashSet::new();
            for t in succs(&f.blocks[b].term) {
                s.extend(live[t as usize].iter().copied());
            }
            let (g, k) = &gen_kill[b];
            let mut inn: HashSet<Local> = s.difference(k).copied().collect();
            inn.extend(g.iter().copied());
            if inn != live[b] {
                live[b] = inn;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    live.into_iter()
        .map(|s| {
            let mut v: Vec<Local> = s.into_iter().filter(|l| *l != 0).collect();
            v.sort_unstable();
            v
        })
        .collect()
}
