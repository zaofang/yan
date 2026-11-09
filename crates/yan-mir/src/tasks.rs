//! The task transform (RFC 0043): each `par concurrent` becomes a dispatcher over its branches,
//! which stay blocks of this function, so every value live across a suspension is a local of
//! the frame. A task's resume points are its entry and the blocks its `Suspend`s name; `state`
//! picks one when yan-rt's scheduler (`ParNext`) chooses the task.

use crate::*;

const I64: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

struct Region {
    par: BlockId,
    tasks: Vec<BlockId>,
    join: BlockId,
    parent: Option<usize>,
}

/// The `par concurrent` regions of `f` and the (region, task) each block runs in: `None`
/// outside every region, or for a block that is not reachable (`seen` false).
pub(crate) struct Walk {
    regions: Vec<Region>,
    pub owner: Vec<Option<(usize, usize)>>,
    pub seen: Vec<bool>,
}

pub(crate) fn walk(f: &Func) -> Walk {
    let mut regions: Vec<Region> = Vec::new();
    let mut owner: Vec<Option<(usize, usize)>> = vec![None; f.blocks.len()];
    let mut seen = vec![false; f.blocks.len()];
    let mut work: Vec<(BlockId, Option<(usize, usize)>)> = vec![(0, None)];
    while let Some((b, ctx)) = work.pop() {
        if std::mem::replace(&mut seen[b as usize], true) {
            continue;
        }
        owner[b as usize] = ctx;
        match &f.blocks[b as usize].term {
            Term::Par { tasks, join } => {
                let r = regions.len();
                regions.push(Region {
                    par: b,
                    tasks: tasks.clone(),
                    join: *join,
                    parent: ctx.map(|(r, _)| r),
                });
                work.push((*join, ctx));
                work.extend(tasks.iter().enumerate().map(|(k, t)| (*t, Some((r, k)))));
            }
            Term::Suspend { resume } => work.push((*resume, ctx)),
            // The frame's drops run where the block's own return leaves.
            Term::Unwind { drops } => {
                let (r, _) = ctx.expect("`Unwind` is in a task");
                work.push((*drops, owner[regions[r].par as usize]));
            }
            Term::Jump(t) => work.push((*t, ctx)),
            Term::Branch { then, els, .. } => work.extend([(*then, ctx), (*els, ctx)]),
            Term::Switch { targets, .. } => work.extend(targets.iter().map(|t| (*t, ctx))),
            Term::TaskEnd
            | Term::Return(_)
            | Term::TailCall { .. }
            | Term::Panic(_)
            | Term::PanicFields { .. } => {}
        }
    }
    Walk {
        regions,
        owner,
        seen,
    }
}

/// Remove `Par` and `TaskEnd` from `f`, and the `Suspend`s inside regions. In a coroutine
/// variant (`co`) the `Suspend`s outside every region stay, for `co` to turn into returns,
/// and a block whose tasks are all parked suspends the variant with one of its own (RFC 0043,
/// nested par). Elsewhere such a `Suspend` is left by an operation that cannot park, so it is
/// unreachable. A task that returns cancels the others and the block returns its result, of
/// slot types `ret`, once they end (9.4, R394); tail calls in tasks are calls by now.
pub fn split(f: &mut Func, co: bool, ret: &[Ty]) {
    let Walk {
        regions,
        mut owner,
        mut seen,
    } = walk(f);
    for (b, blk) in f.blocks.iter_mut().enumerate() {
        let stray = match blk.term {
            Term::Suspend { .. } => !co,
            Term::TaskEnd => true,
            _ => false,
        };
        if owner[b].is_none() && stray {
            blk.term = Term::Panic("unreachable");
        }
    }
    if regions.is_empty() {
        return;
    }

    let push = |f: &mut Func, b: BlockId, i: Inst| {
        let b = &mut f.blocks[b as usize];
        b.insts.push(i);
        b.lines.push(0);
    };
    let rt = |f: Option<Local>, rf: RtFn, args: Vec<Operand>| Inst::Rt {
        dst: f,
        f: rf,
        args,
    };
    let new_block = |f: &mut Func, term: Term| {
        f.blocks.push(Block {
            insts: Vec::new(),
            lines: Vec::new(),
            term,
        });
        f.blocks.len() as BlockId - 1
    };
    let new_local = |f: &mut Func| {
        f.locals.push(I64);
        f.locals.len() as Local - 1
    };

    // Per region: its dispatcher, each task's state local and resume points.
    let mut disp = Vec::new();
    let mut states: Vec<Vec<Local>> = Vec::new();
    let mut resumes: Vec<Vec<Vec<BlockId>>> = Vec::new();
    let mut handles = Vec::new();
    let mut pending = Vec::new();
    let mut results: Vec<Vec<Local>> = Vec::new();
    // Per region: which target of `leave` the kept result takes, and `leave`.
    let mut sites = Vec::new();
    let mut leaves = Vec::new();
    for r in &regions {
        let n = r.tasks.len();
        let can_yield = co || r.parent.is_some();
        let h = new_local(f);
        let next = new_local(f);
        let d = new_block(f, Term::Panic("unreachable"));
        push(f, d, rt(Some(next), RtFn::ParNext, vec![Operand::Local(h)]));
        let st: Vec<Local> = (0..n).map(|_| new_local(f)).collect();
        for s in &st {
            push(
                f,
                r.par,
                Inst::Copy {
                    dst: *s,
                    src: Operand::Int(0),
                },
            );
        }
        push(
            f,
            r.par,
            rt(
                Some(h),
                RtFn::ParBegin,
                vec![Operand::Int(n as i128), Operand::Int(i128::from(can_yield))],
            ),
        );
        let p = new_local(f);
        let site = new_local(f);
        for l in [p, site] {
            push(
                f,
                r.par,
                Inst::Copy {
                    dst: l,
                    src: Operand::Int(0),
                },
            );
        }
        f.blocks[r.par as usize].term = Term::Jump(d);
        let res: Vec<Local> = ret
            .iter()
            .map(|t| {
                f.locals.push(*t);
                f.locals.len() as Local - 1
            })
            .collect();
        let ret_res = new_block(
            f,
            Term::Return(res.iter().map(|l| Operand::Local(*l)).collect()),
        );
        let leave = new_block(
            f,
            Term::Switch {
                on: site,
                targets: vec![ret_res],
            },
        );
        let done = new_block(
            f,
            Term::Switch {
                on: p,
                targets: vec![r.join, leave],
            },
        );
        push(f, done, rt(None, RtFn::ParEnd, vec![Operand::Local(h)]));
        // The task blocks get their `Switch` once every resume point is known.
        let mut targets: Vec<BlockId> = (0..n)
            .map(|_| new_block(f, Term::Panic("unreachable")))
            .collect();
        targets.push(done);
        // Every task parked: suspend the task (or variant) the block runs in, and ask again on
        // resuming.
        let yields = new_block(
            f,
            if can_yield {
                Term::Suspend { resume: d }
            } else {
                Term::Panic("unreachable")
            },
        );
        targets.push(yields);
        let idle = idle_message(f, d);
        targets.push(idle);
        owner.resize(f.blocks.len(), None);
        seen.resize(f.blocks.len(), true);
        owner[yields as usize] = owner[r.par as usize];
        owner[idle as usize] = owner[r.par as usize];
        // The block's own return leaves from the task around it, if any.
        owner[leave as usize] = owner[r.par as usize];
        owner[ret_res as usize] = owner[r.par as usize];
        f.blocks[d as usize].term = Term::Switch { on: next, targets };
        disp.push(d);
        handles.push(h);
        pending.push(p);
        results.push(res);
        sites.push(site);
        leaves.push(leave);
        states.push(st);
        resumes.push(r.tasks.iter().map(|t| vec![*t]).collect());
    }

    for b in 0..owner.len() {
        let Some((r, k)) = owner[b] else {
            if !seen[b] {
                f.blocks[b].term = Term::Panic("unreachable");
            }
            continue;
        };
        let b = b as BlockId;
        match f.blocks[b as usize].term.clone() {
            Term::Suspend { resume } => {
                let rs = &mut resumes[r][k];
                let at = rs.iter().position(|x| *x == resume).unwrap_or_else(|| {
                    rs.push(resume);
                    rs.len() - 1
                });
                push(
                    f,
                    b,
                    Inst::Copy {
                        dst: states[r][k],
                        src: Operand::Int(at as i128),
                    },
                );
                f.blocks[b as usize].term = Term::Jump(disp[r]);
            }
            Term::TaskEnd => {
                push(
                    f,
                    b,
                    rt(None, RtFn::ParTaskEnd, vec![Operand::Local(handles[r])]),
                );
                f.blocks[b as usize].term = Term::Jump(disp[r]);
            }
            // Leaving the function from a task: the block keeps one result (R394).
            term @ (Term::Return(_) | Term::Unwind { .. }) => {
                let first = new_local(f);
                push(
                    f,
                    b,
                    rt(
                        Some(first),
                        RtFn::ParCancel,
                        vec![Operand::Local(handles[r])],
                    ),
                );
                let (mut keep, at): (Vec<Inst>, usize) = match term {
                    Term::Return(os) => (
                        results[r]
                            .iter()
                            .zip(os)
                            .map(|(d, o)| Inst::Copy { dst: *d, src: o })
                            .collect(),
                        0,
                    ),
                    Term::Unwind { drops } => {
                        let Term::Switch { targets, .. } = &mut f.blocks[leaves[r] as usize].term
                        else {
                            unreachable!("`leave` switches")
                        };
                        targets.push(drops);
                        (Vec::new(), targets.len() - 1)
                    }
                    _ => unreachable!(),
                };
                keep.push(Inst::Copy {
                    dst: sites[r],
                    src: Operand::Int(at as i128),
                });
                keep.push(Inst::Copy {
                    dst: pending[r],
                    src: Operand::Int(1),
                });
                let lines = vec![0; keep.len()];
                f.blocks.push(Block {
                    insts: keep,
                    lines,
                    term: Term::Jump(disp[r]),
                });
                let save = f.blocks.len() as BlockId - 1;
                f.blocks[b as usize].term = Term::Switch {
                    on: first,
                    targets: vec![disp[r], save],
                };
            }
            Term::TailCall { .. } => unreachable!("tail calls in tasks are calls"),
            _ => {}
        }
    }

    for (r, d) in disp.iter().enumerate() {
        let Term::Switch { targets, .. } = f.blocks[*d as usize].term.clone() else {
            unreachable!("a dispatcher switches")
        };
        for (k, t) in targets[..regions[r].tasks.len()].iter().enumerate() {
            f.blocks[*t as usize].term = Term::Switch {
                on: states[r][k],
                targets: resumes[r][k].clone(),
            };
        }
    }
}

/// A block that runs an idle actor's queued message, which owns its captures block, then
/// returns to the dispatcher `d` (RFC 0056 D4).
fn idle_message(f: &mut Func, d: BlockId) -> BlockId {
    let mut local = |t: Ty| {
        f.locals.push(t);
        f.locals.len() as Local - 1
    };
    let (got, code, env, u) = (local(I64), local(Ty::Ptr), local(Ty::Ptr), local(Ty::Unit));
    let rt = |dst: Option<Local>, f: RtFn, args: Vec<Operand>| Inst::Rt { dst, f, args };
    let insts = vec![
        rt(Some(got), RtFn::ActorIdle, Vec::new()),
        rt(Some(code), RtFn::ActorArg, vec![Operand::Int(0)]),
        rt(Some(env), RtFn::ActorArg, vec![Operand::Int(1)]),
        Inst::CallIndirect {
            dst: vec![u],
            code: Operand::Local(code),
            args: vec![Operand::Local(env)],
            params: vec![Ty::Ptr],
            ret: vec![Ty::Unit],
        },
        rt(None, RtFn::EnvFree, vec![Operand::Local(env)]),
        rt(None, RtFn::ActorRan, Vec::new()),
    ];
    f.blocks.push(Block {
        lines: vec![0; insts.len()],
        insts,
        term: Term::Jump(d),
    });
    f.blocks.len() as BlockId - 1
}
