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

impl Walk {
    /// Reachable and outside every `par concurrent`.
    pub fn top(&self, b: usize) -> bool {
        self.seen[b] && self.owner[b].is_none()
    }
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

/// Remove `Par`, `Suspend` and `TaskEnd` from `f`. A `Suspend` outside every region is left
/// by an operation that cannot park there, so it is unreachable.
pub fn split(f: &mut Func) {
    let Walk {
        regions,
        owner,
        seen,
    } = walk(f);
    for (b, blk) in f.blocks.iter_mut().enumerate() {
        if owner[b].is_none() && matches!(blk.term, Term::Suspend { .. } | Term::TaskEnd) {
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
    for r in &regions {
        let n = r.tasks.len();
        let next = new_local(f);
        let d = new_block(f, Term::Panic("unreachable"));
        push(f, d, rt(Some(next), RtFn::ParNext, Vec::new()));
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
            rt(None, RtFn::ParBegin, vec![Operand::Int(n as i128)]),
        );
        f.blocks[r.par as usize].term = Term::Jump(d);
        let done = new_block(f, Term::Jump(r.join));
        push(f, done, rt(None, RtFn::ParEnd, Vec::new()));
        // The task blocks get their `Switch` once every resume point is known.
        let mut targets: Vec<BlockId> = (0..n)
            .map(|_| new_block(f, Term::Panic("unreachable")))
            .collect();
        targets.push(done);
        f.blocks[d as usize].term = Term::Switch { on: next, targets };
        disp.push(d);
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
                push(f, b, rt(None, RtFn::ParTaskEnd, Vec::new()));
                f.blocks[b as usize].term = Term::Jump(disp[r]);
            }
            // Leaving the function from a task ends every block around it.
            Term::Return(_) | Term::TailCall { .. } => {
                let mut at = Some(r);
                while let Some(r) = at {
                    push(f, b, rt(None, RtFn::ParEnd, Vec::new()));
                    at = regions[r].parent;
                }
            }
            _ => {}
        }
    }

    for (r, d) in disp.iter().enumerate() {
        let Term::Switch { targets, .. } = f.blocks[*d as usize].term.clone() else {
            unreachable!("a dispatcher switches")
        };
        for (k, t) in targets[..targets.len() - 1].iter().enumerate() {
            f.blocks[*t as usize].term = Term::Switch {
                on: states[r][k],
                targets: resumes[r][k].clone(),
            };
        }
    }
}
