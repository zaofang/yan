//! Plain `par` on the worker pool (RFC 0086). An arm that is safe for a worker (it reads and
//! writes only what it owns or shares immutably, calls only such functions, and leaves by
//! falling out of its end or by `?`, `fail` or `return`, D8) is outlined as `g$arm(frame) -> i64` over a heap frame, one word per
//! local of `g` after the arm code addresses. `PoolBegin` decides at run time: 0 runs the arms
//! inline as before, 1 stores the locals live into the arms, runs them on workers with
//! `PoolRun`, and loads the locals they define for the code after the join. An arm that is
//! not safe keeps the inline path alone. An arm that leaves ends at a `Leave` marker and a jump to
//! the caller's code for the enclosing frame's drops: it stores the locals that code reads in
//! exit words of its own and returns the code of the exit; `PoolRun` reports the lowest arm's
//! code and the caller loads those words and jumps there.

use std::collections::HashSet;

use crate::co::{live_sets, map_inst, map_term, new_block, new_local, term_uses, uses_defs};
use crate::*;

const I64: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

pub fn run(p: &mut Program) {
    let n = p.funcs.len();
    let safe = safe_funcs(p);
    let mut outlined: Vec<Func> = Vec::new();
    for g in 0..n {
        if !p.funcs[g].pinned.is_empty() || p.funcs[g].name.ends_with("$co") {
            continue;
        }
        let mut tried: HashSet<BlockId> = HashSet::new();
        while let Some(site) = next_site(&p.funcs[g], &tried) {
            tried.insert(site);
            if let Some(r) = region(&p.funcs[g], site, &safe) {
                transform(&mut p.funcs[g], &r, n, &mut outlined);
            }
        }
    }
    p.funcs.extend(outlined);
}

/// The functions whose every instruction may run on a worker, to a fixed point.
fn safe_funcs(p: &Program) -> Vec<bool> {
    let ok_inst = |i: &Inst| match i {
        Inst::Rt { f, .. } => f.pool_safe(),
        Inst::CallIndirect { .. }
        | Inst::Atomic { .. }
        | Inst::Fence { .. }
        | Inst::AddrOf { .. } => false,
        _ => true,
    };
    let mut safe: Vec<bool> = p
        .funcs
        .iter()
        .map(|f| {
            f.pinned.is_empty()
                && f.blocks.iter().all(|b| {
                    b.insts.iter().all(ok_inst)
                        && !matches!(
                            b.term,
                            Term::Par { .. } | Term::Suspend { .. } | Term::TaskEnd
                        )
                })
        })
        .collect();
    loop {
        let mut changed = false;
        for g in 0..safe.len() {
            if !safe[g] {
                continue;
            }
            let bad = p.funcs[g].blocks.iter().any(|b| {
                b.insts
                    .iter()
                    .any(|i| matches!(i, Inst::Call { func, .. } if !safe[*func as usize]))
                    || matches!(b.term, Term::TailCall { func, .. } if !safe[func as usize])
            });
            if bad {
                safe[g] = false;
                changed = true;
            }
        }
        if !changed {
            return safe;
        }
    }
}

fn first_arm(b: &Block) -> Option<ArmEvent> {
    match b.insts.first() {
        Some(Inst::Arm(e)) => Some(*e),
        _ => None,
    }
}

/// A block whose last instruction is a `Fork` that was not tried yet.
fn next_site(f: &Func, tried: &HashSet<BlockId>) -> Option<BlockId> {
    (0..f.blocks.len() as BlockId).find(|b| {
        !tried.contains(b)
            && matches!(
                f.blocks[*b as usize].insts.last(),
                Some(Inst::Arm(ArmEvent::Fork))
            )
    })
}

/// An arm's `Leave`: `block` jumps to `target`, which is not in the arm.
struct Exit {
    arm: usize,
    block: BlockId,
    target: BlockId,
}

struct Region {
    site: BlockId,
    exits: Vec<Exit>,
    /// Per arm: its blocks, the entry first.
    arms: Vec<Vec<BlockId>>,
    join: BlockId,
}

/// The arms of the `par` forked at `site`, when each is safe for a worker.
fn region(f: &Func, site: BlockId, safe: &[bool]) -> Option<Region> {
    let Term::Jump(mut entry) = f.blocks[site as usize].term else {
        return None;
    };
    let mut arms = Vec::new();
    let mut exits = Vec::new();
    loop {
        if first_arm(&f.blocks[entry as usize]) != Some(ArmEvent::Enter(arms.len() as u32)) {
            return None;
        }
        let mut seen = vec![entry];
        let mut stops = HashSet::new();
        let mut work = vec![entry];
        while let Some(b) = work.pop() {
            let blk = &f.blocks[b as usize];
            if blk.insts.iter().any(|i| match i {
                Inst::Arm(ArmEvent::Fork) => true,
                Inst::Rt { f, .. } => !f.pool_safe(),
                Inst::Call { func, .. } => !safe[*func as usize],
                Inst::CallIndirect { .. }
                | Inst::Atomic { .. }
                | Inst::Fence { .. }
                | Inst::AddrOf { .. } => true,
                _ => false,
            }) {
                return None;
            }
            if blk
                .insts
                .iter()
                .any(|i| matches!(i, Inst::Arm(ArmEvent::Leave)))
            {
                let Term::Jump(target) = blk.term else {
                    return None;
                };
                exits.push(Exit {
                    arm: arms.len(),
                    block: b,
                    target,
                });
                continue;
            }
            let succ = match &blk.term {
                Term::Jump(_) | Term::Branch { .. } | Term::Switch { .. } => {
                    crate::co::succs(&blk.term)
                }
                Term::Panic(_) | Term::PanicFields { .. } => Vec::new(),
                _ => return None,
            };
            for s in succ {
                if first_arm(&f.blocks[s as usize])
                    .is_some_and(|e| matches!(e, ArmEvent::Enter(_) | ArmEvent::Join))
                {
                    stops.insert(s);
                } else if !seen.contains(&s) {
                    seen.push(s);
                    work.push(s);
                }
            }
        }
        let [stop] = stops.into_iter().collect::<Vec<_>>()[..] else {
            return None;
        };
        arms.push(seen);
        if first_arm(&f.blocks[stop as usize]) == Some(ArmEvent::Join) {
            return Some(Region {
                site,
                exits,
                arms,
                join: stop,
            });
        }
        entry = stop;
    }
}

fn word(t: Ty) -> bool {
    match t {
        Ty::Int(i) => i.bits <= 64,
        Ty::F64 | Ty::Bool | Ty::Ptr => true,
        Ty::Unit => false,
    }
}

fn transform(f: &mut Func, r: &Region, funcs: usize, out: &mut Vec<Func>) {
    let n = r.arms.len();
    let live = live_sets(f);
    let defs: Vec<HashSet<Local>> = r
        .arms
        .iter()
        .map(|bs| {
            bs.iter()
                .flat_map(|b| f.blocks[*b as usize].insts.iter())
                .flat_map(|i| uses_defs(i).1)
                .collect()
        })
        .collect();
    let join_live: HashSet<Local> = live[r.join as usize].iter().copied().collect();
    let outs: Vec<Vec<Local>> = defs
        .iter()
        .map(|d| {
            let mut v: Vec<Local> = d.intersection(&join_live).copied().collect();
            v.sort_unstable();
            v
        })
        .collect();
    // Two arms that define one local the join reads would race for it.
    let mut all_outs: Vec<Local> = outs.iter().flatten().copied().collect();
    all_outs.sort_unstable();
    if all_outs.windows(2).any(|w| w[0] == w[1]) {
        return;
    }
    // What the code at an exit reads and an arm defines goes in words of its own.
    let xouts: Vec<Vec<Local>> = r
        .exits
        .iter()
        .map(|x| {
            let mut v: Vec<Local> = live[x.target as usize]
                .iter()
                .copied()
                .filter(|l| defs[x.arm].contains(l))
                .collect();
            v.sort_unstable();
            v
        })
        .collect();
    // An arm loads what is live at its entry and it reads or hands on; a local live through it
    // that a sibling arm defines stays out of the frame, since nothing wrote it before the fork.
    let ins: Vec<Vec<Local>> = r
        .arms
        .iter()
        .enumerate()
        .map(|(k, bs)| {
            let mut needed: HashSet<Local> = outs[k].iter().copied().collect();
            for (j, x) in r.exits.iter().enumerate() {
                if x.arm == k {
                    needed.extend(&xouts[j]);
                }
            }
            for b in bs {
                let blk = &f.blocks[*b as usize];
                needed.extend(blk.insts.iter().flat_map(|i| uses_defs(i).0));
                needed.extend(term_uses(&blk.term));
            }
            live[bs[0] as usize]
                .iter()
                .copied()
                .filter(|l| needed.contains(l))
                .collect()
        })
        .collect();
    let moved: Vec<Local> = ins
        .iter()
        .flatten()
        .chain(&all_outs)
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let tys = f.locals.clone();
    if moved
        .iter()
        .chain(xouts.iter().flatten())
        .any(|l| tys[*l as usize] != Ty::Unit && !word(tys[*l as usize]))
    {
        return;
    }
    let mut next_word = (n + f.locals.len()) as u32;
    let xslots: Vec<Vec<(Local, u32)>> = xouts
        .iter()
        .map(|ls| {
            ls.iter()
                .filter(|l| tys[**l as usize] != Ty::Unit)
                .map(|l| {
                    next_word += 1;
                    (*l, next_word - 1)
                })
                .collect()
        })
        .collect();
    let slot = |l: Local| (n as u32) + l;
    let typed = |ls: &[Local]| -> Vec<Local> {
        ls.iter()
            .copied()
            .filter(|l| tys[*l as usize] != Ty::Unit)
            .collect()
    };

    // The arms as functions.
    let mut ids = Vec::new();
    for (k, bs) in r.arms.iter().enumerate() {
        let mut a = Func {
            unsafe_fn: f.unsafe_fn,
            name: format!("{}$arm{k}", f.name),
            params: 1,
            ret: vec![I64],
            locals: std::iter::once(Ty::Ptr)
                .chain(f.locals.iter().copied())
                .collect(),
            pinned: Vec::new(),
            blocks: Vec::new(),
        };
        let at = |b: BlockId| 1 + bs.iter().position(|x| *x == b).expect("an arm block") as BlockId;
        let exit = bs.len() as BlockId + 1;
        let mut entry = Block {
            insts: typed(&ins[k])
                .iter()
                .map(|l| Inst::Load {
                    dst: l + 1,
                    ptr: Operand::Local(0),
                    word: slot(*l),
                })
                .collect(),
            lines: Vec::new(),
            term: Term::Jump(1),
        };
        entry.lines = vec![0; entry.insts.len()];
        a.blocks.push(entry);
        for b in bs {
            let mut blk = f.blocks[*b as usize].clone();
            blk.insts
                .iter_mut()
                .for_each(|i| map_inst(i, &mut |l| l + 1));
            map_term(&mut blk.term, &mut |l| l + 1);
            let to = |t: &mut BlockId| *t = if bs.contains(t) { at(*t) } else { exit };
            let leave = r.exits.iter().position(|x| x.arm == k && x.block == *b);
            match &mut blk.term {
                Term::Jump(t) if leave.is_some() => {
                    let at_arm = r.exits[..leave.unwrap()]
                        .iter()
                        .filter(|x| x.arm == k)
                        .count();
                    *t = bs.len() as BlockId + 2 + at_arm as BlockId;
                }
                Term::Jump(t) => to(t),
                Term::Branch { then, els, .. } => {
                    to(then);
                    to(els);
                }
                Term::Switch { targets, .. } => targets.iter_mut().for_each(to),
                _ => {}
            }
            a.blocks.push(blk);
        }
        let stores: Vec<Inst> = typed(&outs[k])
            .iter()
            .map(|l| Inst::Store {
                ptr: Operand::Local(0),
                word: slot(*l),
                src: Operand::Local(l + 1),
            })
            .collect();
        a.blocks.push(Block {
            lines: vec![0; stores.len()],
            insts: stores,
            term: Term::Return(vec![Operand::Int(0)]),
        });
        for (j, x) in r.exits.iter().enumerate().filter(|(_, x)| x.arm == k) {
            let _ = x;
            let stores: Vec<Inst> = xslots[j]
                .iter()
                .map(|(l, w)| Inst::Store {
                    ptr: Operand::Local(0),
                    word: *w,
                    src: Operand::Local(l + 1),
                })
                .collect();
            a.blocks.push(Block {
                lines: vec![0; stores.len()],
                insts: stores,
                term: Term::Return(vec![Operand::Int(j as i128 + 1)]),
            });
        }
        ids.push((funcs + out.len()) as FuncId);
        out.push(a);
    }

    // The fork: ask the pool, else go on inline.
    let inline = match f.blocks[r.site as usize].term {
        Term::Jump(b) => b,
        _ => unreachable!("a fork jumps to its first arm"),
    };
    let m = new_local(f, I64);
    let go = new_local(f, Ty::Bool);
    let fr = new_local(f, Ty::Ptr);
    let rt = |dst: Option<Local>, f: RtFn, args: Vec<Operand>| Inst::Rt { dst, f, args };
    let mut pre = vec![
        rt(Some(m), RtFn::PoolBegin, vec![Operand::Int(n as i128)]),
        Inst::Scalar {
            dst: Some(go),
            op: ScalarOp::Ne,
            ty: I64,
            args: vec![Operand::Local(m), Operand::Int(0)],
        },
    ];
    let words = next_word as usize;
    let mut run_insts = vec![rt(Some(fr), RtFn::Alloc, vec![Operand::Int(words as i128)])];
    for (k, id) in ids.iter().enumerate() {
        let c = new_local(f, Ty::Ptr);
        run_insts.push(Inst::Copy {
            dst: c,
            src: Operand::Func(*id),
        });
        run_insts.push(Inst::Store {
            ptr: Operand::Local(fr),
            word: k as u32,
            src: Operand::Local(c),
        });
    }
    let mut stored = typed(&moved);
    stored.retain(|l| ins.iter().any(|i| i.contains(l)));
    stored.sort_unstable();
    for l in stored {
        run_insts.push(Inst::Store {
            ptr: Operand::Local(fr),
            word: slot(l),
            src: Operand::Local(l),
        });
    }
    let code = new_local(f, I64);
    run_insts.push(rt(
        Some(code),
        RtFn::PoolRun,
        vec![Operand::Local(fr), Operand::Int(n as i128)],
    ));
    // Code 0: every arm fell out of its end. Code j + 1: the exit `j` of the lowest arm that left.
    let loads = |words: Vec<(Local, u32)>| -> Vec<Inst> {
        words
            .into_iter()
            .map(|(l, w)| Inst::Load {
                dst: l,
                ptr: Operand::Local(fr),
                word: w,
            })
            .collect()
    };
    let mut targets = Vec::new();
    let mut fall = loads(typed(&all_outs).into_iter().map(|l| (l, slot(l))).collect());
    fall.push(rt(None, RtFn::EnvFree, vec![Operand::Local(fr)]));
    targets.push(new_block(f, fall, Term::Jump(r.join)));
    for (j, x) in r.exits.iter().enumerate() {
        let mut ex = loads(xslots[j].clone());
        ex.push(rt(None, RtFn::EnvFree, vec![Operand::Local(fr)]));
        targets.push(new_block(f, ex, Term::Jump(x.target)));
    }
    let pool = new_block(f, run_insts, Term::Switch { on: code, targets });
    let blk = &mut f.blocks[r.site as usize];
    blk.lines.extend(std::iter::repeat_n(0, pre.len()));
    blk.insts.append(&mut pre);
    blk.term = Term::Branch {
        cond: Operand::Local(go),
        then: pool,
        els: inline,
    };
}
