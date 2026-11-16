//! `--ub` data races between a plain access and an atomic one, on hand-built MIR (the surfaces
//! cannot yet spell a plain access beside an `Atomic`): one `par` arm writes a word then
//! publishes a flag, the other loads the flag and reads the word.

use yan_machine::{Stop, run};
use yan_mir::{
    ArmEvent, AtomicOp, Block, Func, Inst, IntTy, MemOrder, Operand, Program, RtFn, Term, Ty,
};

const INT: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

fn program(publish: MemOrder, observe: MemOrder, fences: Option<(MemOrder, MemOrder)>) -> Program {
    let (data, flag, seen, got) = (0, 1, 2, 3);
    let local = |l| Operand::Local(l);
    let alloc = |dst| Inst::Rt {
        dst: Some(dst),
        f: RtFn::Alloc,
        args: vec![Operand::Int(1)],
    };
    let atomic = |op, order, args: Vec<Operand>, dst| Inst::Atomic {
        dst,
        op,
        ty: INT,
        order,
        failure: MemOrder::SeqCst,
        ptr: local(flag),
        args,
    };
    let mut insts = vec![
        alloc(data),
        alloc(flag),
        Inst::Store {
            ptr: local(data),
            word: 0,
            src: Operand::Int(0),
        },
        atomic(
            AtomicOp::Store,
            MemOrder::SeqCst,
            vec![Operand::Int(0)],
            None,
        ),
        Inst::Arm(ArmEvent::Fork),
        Inst::Arm(ArmEvent::Enter(0)),
        Inst::Store {
            ptr: local(data),
            word: 0,
            src: Operand::Int(7),
        },
    ];
    if let Some((release, _)) = fences {
        insts.push(Inst::Fence { order: release });
    }
    insts.push(atomic(
        AtomicOp::Store,
        publish,
        vec![Operand::Int(1)],
        None,
    ));
    insts.extend([
        Inst::Arm(ArmEvent::Exit),
        Inst::Arm(ArmEvent::Enter(1)),
        atomic(AtomicOp::Load, observe, vec![], Some(seen)),
    ]);
    if let Some((_, acquire)) = fences {
        insts.push(Inst::Fence { order: acquire });
    }
    insts.extend([
        Inst::Load {
            dst: got,
            ptr: local(data),
            word: 0,
        },
        Inst::Arm(ArmEvent::Exit),
        Inst::Arm(ArmEvent::Join),
    ]);
    let n = insts.len();
    Program {
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            ret: vec![Ty::Unit],
            locals: vec![Ty::Ptr, Ty::Ptr, INT, INT],
            pinned: Vec::new(),
            unsafe_fn: false,
            blocks: vec![Block {
                insts,
                lines: vec![0; n],
                term: Term::Return(vec![Operand::Unit]),
            }],
        }],
        main: 0,
        strings: Vec::new(),
    }
}

fn races(p: &Program) -> bool {
    match run(p, true) {
        Ok(_) => false,
        Err(Stop::Ub(1, _)) => true,
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn release_and_acquire_order_the_plain_accesses() {
    use MemOrder::*;
    assert!(!races(&program(Release, Acquire, None)));
    assert!(!races(&program(SeqCst, SeqCst, None)));
    assert!(!races(&program(AcqRel, AcqRel, None)));
}

#[test]
fn a_relaxed_side_leaves_them_unordered() {
    use MemOrder::*;
    assert!(races(&program(Relaxed, Acquire, None)));
    assert!(races(&program(Release, Relaxed, None)));
}

#[test]
fn fences_order_relaxed_accesses() {
    use MemOrder::*;
    assert!(!races(&program(Relaxed, Relaxed, Some((Release, Acquire)))));
    assert!(races(&program(Relaxed, Relaxed, Some((Acquire, Release)))));
}

#[test]
fn a_plain_access_races_an_atomic_one() {
    let mut p = program(MemOrder::SeqCst, MemOrder::SeqCst, None);
    // Reading the flag word plainly in the second arm, which no edge orders after the store.
    let at = p.funcs[0].blocks[0]
        .insts
        .iter()
        .position(|i| matches!(i, Inst::Load { .. }))
        .expect("a load");
    p.funcs[0].blocks[0].insts[at] = Inst::Load {
        dst: 3,
        ptr: Operand::Local(1),
        word: 0,
    };
    p.funcs[0].blocks[0].insts.remove(at - 1);
    p.funcs[0].blocks[0].lines.pop();
    assert!(races(&p));
}
