//! `--ub` item 8 on hand-built MIR: a use of a moved-from local or word (RFC 0087 D1).

use yan_machine::{Stop, run};
use yan_mir::{Block, Func, Inst, IntTy, Loc, Operand, Program, RtFn, Term, Ty};

const INT: Ty = Ty::Int(IntTy {
    bits: 64,
    signed: true,
});

fn program(insts: Vec<Inst>) -> Program {
    let n = insts.len();
    Program {
        funcs: vec![Func {
            name: "main".into(),
            params: 0,
            ret: vec![Ty::Unit],
            locals: vec![INT, INT, Ty::Ptr],
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

fn set(dst: u32, v: i128) -> Inst {
    Inst::Copy {
        dst,
        src: Operand::Int(v),
    }
}

fn read(dst: u32, src: u32) -> Inst {
    Inst::Copy {
        dst,
        src: Operand::Local(src),
    }
}

fn mv(l: u32) -> Inst {
    Inst::Move {
        places: vec![Loc::Local(l)],
    }
}

fn item(p: &Program, ub: bool) -> Option<u8> {
    match run(p, ub) {
        Ok(_) => None,
        Err(Stop::Ub(i, _)) => Some(i),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn a_read_of_a_moved_local_is_item_8() {
    let p = program(vec![set(0, 1), read(1, 0), mv(0), read(1, 0)]);
    assert_eq!(item(&p, true), Some(8));
    assert_eq!(item(&p, false), None);
}

#[test]
fn an_assignment_ends_the_moved_state() {
    let p = program(vec![set(0, 1), mv(0), set(0, 2), read(1, 0)]);
    assert_eq!(item(&p, true), None);
}

#[test]
fn a_sibling_local_stays_readable() {
    let p = program(vec![set(0, 1), set(1, 2), mv(0), read(1, 1)]);
    assert_eq!(item(&p, true), None);
}

fn block_program(after: Vec<Inst>) -> Program {
    let mut insts = vec![
        Inst::Rt {
            dst: Some(2),
            f: RtFn::Alloc,
            args: vec![Operand::Int(2)],
        },
        Inst::Store {
            ptr: Operand::Local(2),
            word: 0,
            src: Operand::Int(7),
        },
        Inst::Store {
            ptr: Operand::Local(2),
            word: 1,
            src: Operand::Int(8),
        },
        Inst::Move {
            places: vec![Loc::Word {
                ptr: Operand::Local(2),
                word: 0,
            }],
        },
    ];
    insts.extend(after);
    program(insts)
}

fn load(word: u32) -> Inst {
    Inst::Load {
        dst: 0,
        ptr: Operand::Local(2),
        word,
    }
}

#[test]
fn a_load_of_a_moved_word_is_item_8() {
    assert_eq!(item(&block_program(vec![load(0)]), true), Some(8));
}

#[test]
fn a_sibling_word_and_a_restored_word_are_defined() {
    assert_eq!(item(&block_program(vec![load(1)]), true), None);
    let store = Inst::Store {
        ptr: Operand::Local(2),
        word: 0,
        src: Operand::Int(9),
    };
    assert_eq!(item(&block_program(vec![store, load(0)]), true), None);
}
