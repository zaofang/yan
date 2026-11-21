//! `--ub` item 5 on hand-built MIR: Tree Borrows retags and accesses (RFC 0088 D1, D2).

use yan_machine::{Stop, run};
use yan_mir::{Block, Func, Inst, IntTy, Operand, Program, RetagKind, RtFn, Term, Ty};

const INT: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

const BASE: u32 = 0;
const SCRATCH: u32 = 1;
const A: u32 = 2;
const B: u32 = 3;

fn program(after: Vec<Inst>) -> Program {
    let mut insts = vec![
        Inst::Rt {
            dst: Some(BASE),
            f: RtFn::Alloc,
            args: vec![Operand::Int(2)],
        },
        Inst::Store {
            ptr: Operand::Local(BASE),
            word: 0,
            src: Operand::Int(7),
        },
    ];
    insts.extend(after);
    let n = insts.len();
    Program {
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            ret: vec![Ty::Unit],
            locals: vec![Ty::Ptr, INT, INT, INT],
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

fn retag(prov: u32, kind: RetagKind) -> Inst {
    Inst::Retag {
        ptr: Operand::Local(BASE),
        prov,
        bytes: 8,
        kind,
        protected: false,
    }
}

fn access(prov: u32, write: bool) -> Vec<Inst> {
    vec![
        Inst::Access {
            ptr: Operand::Local(BASE),
            prov: Operand::Local(prov),
            bytes: 8,
            write,
        },
        if write {
            Inst::Store {
                ptr: Operand::Local(BASE),
                word: 0,
                src: Operand::Int(1),
            }
        } else {
            Inst::Load {
                dst: SCRATCH,
                ptr: Operand::Local(BASE),
                word: 0,
            }
        },
    ]
}

fn root(write: bool) -> Vec<Inst> {
    access(0, write)[1..].to_vec()
}

fn item(p: &Program, ub: bool) -> Option<u8> {
    match run(p, ub) {
        Ok(_) => None,
        Err(Stop::Ub(i, _)) => Some(i),
        Err(e) => panic!("{e:?}"),
    }
}

fn seq(parts: Vec<Vec<Inst>>) -> Program {
    program(parts.into_iter().flatten().collect())
}

#[test]
fn a_write_through_a_unique_tag_then_the_owner_then_the_tag_is_item_5() {
    let p = seq(vec![
        vec![retag(A, RetagKind::Unique)],
        access(A, true),
        root(true),
        access(A, false),
    ]);
    assert_eq!(item(&p, true), Some(5));
    assert_eq!(item(&p, false), None);
}

#[test]
fn the_owner_reading_between_two_unique_reads_is_defined() {
    let p = seq(vec![
        vec![retag(A, RetagKind::Unique)],
        access(A, false),
        root(false),
        access(A, false),
    ]);
    assert_eq!(item(&p, true), None);
}

#[test]
fn a_write_through_a_frozen_tag_is_item_5() {
    let p = seq(vec![vec![retag(A, RetagKind::Frozen)], access(A, true)]);
    assert_eq!(item(&p, true), Some(5));
}

#[test]
fn a_sibling_write_invalidates_the_other_tag() {
    let p = seq(vec![
        vec![retag(A, RetagKind::Unique), retag(B, RetagKind::Unique)],
        access(A, true),
        access(B, true),
        access(A, false),
    ]);
    assert_eq!(item(&p, true), Some(5));
}

#[test]
fn a_cell_tag_tolerates_the_owner_writing() {
    let p = seq(vec![
        vec![retag(A, RetagKind::Cell)],
        root(true),
        access(A, true),
    ]);
    assert_eq!(item(&p, true), None);
}

#[test]
fn a_retag_loop_stays_bounded_by_the_tag_gc() {
    let mut insts = Vec::new();
    for _ in 0..2000 {
        insts.push(retag(A, RetagKind::Unique));
        insts.extend(access(A, true));
        insts.push(Inst::Copy {
            dst: A,
            src: Operand::Int(0),
        });
    }
    assert_eq!(item(&program(insts), true), None);
}
