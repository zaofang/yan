//! The canonical MIR text (RFC 0097 D1, D3, D5). One printing per program: ids are the in-memory
//! ids, mnemonics come from `mnemonic.rs`, and nothing depends on hash order.
//!
//! ```text
//! mir 1
//! main fn0
//! str 0 "hi"
//!
//! fn 0 "main"() -> (unit) {
//!   local %0: i64
//! bb0:
//!   %0 = copy 1
//!   ret ()
//! }
//! ```

use std::fmt::{self, Display, Formatter, Write};

use crate::{
    ArmEvent, Block, Func, Inst, IntTy, Loc, Operand, Program, RetagKind, ScalarOp, Term, Ty,
};

/// The version in the `mir <version>` header; a reader of another version must refuse the text.
pub const VERSION: u32 = 1;

impl Display for IntTy {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}{}", if self.signed { 'i' } else { 'u' }, self.bits)
    }
}

impl Display for Ty {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Ty::Int(t) => t.fmt(f),
            Ty::F64 => f.write_str("f64"),
            Ty::Bool => f.write_str("bool"),
            Ty::Unit => f.write_str("unit"),
            Ty::Ptr => f.write_str("ptr"),
        }
    }
}

/// A quoted string; only `"`, `\`, `\n`, `\r`, `\t` and non-printable characters are escaped.
fn quoted(f: &mut Formatter, s: &str) -> fmt::Result {
    f.write_char('"')?;
    for c in s.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            '\r' => f.write_str("\\r")?,
            '\t' => f.write_str("\\t")?,
            c if c.is_control() => write!(f, "\\u{{{:x}}}", c as u32)?,
            c => f.write_char(c)?,
        }
    }
    f.write_char('"')
}

fn list<T>(
    f: &mut Formatter,
    items: &[T],
    mut item: impl FnMut(&mut Formatter, &T) -> fmt::Result,
) -> fmt::Result {
    for (i, x) in items.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        item(f, x)?;
    }
    Ok(())
}

fn operands(f: &mut Formatter, xs: &[Operand]) -> fmt::Result {
    list(f, xs, |f, x| x.fmt(f))
}

fn tys(f: &mut Formatter, xs: &[Ty]) -> fmt::Result {
    list(f, xs, |f, x| x.fmt(f))
}

fn dsts(f: &mut Formatter, dst: &[u32]) -> fmt::Result {
    if dst.is_empty() {
        return Ok(());
    }
    list(f, dst, |f, d| write!(f, "%{d}"))?;
    f.write_str(" = ")
}

fn dst(f: &mut Formatter, d: Option<u32>) -> fmt::Result {
    d.map_or(Ok(()), |d| write!(f, "%{d} = "))
}

impl Display for Operand {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Operand::Local(l) => write!(f, "%{l}"),
            Operand::Int(n) => write!(f, "{n}"),
            // Debug is the shortest text that parses back to the same value and always has a `.`
            // or an exponent, so it never reads as an integer. NaN keeps its bits.
            Operand::F64(x) if x.is_nan() => write!(f, "nan:{:#x}", x.to_bits()),
            Operand::F64(x) if x.is_infinite() => {
                f.write_str(if *x < 0.0 { "-inf" } else { "inf" })
            }
            Operand::F64(x) => write!(f, "{x:?}"),
            Operand::Bool(b) => write!(f, "{b}"),
            Operand::Unit => f.write_str("()"),
            Operand::Str(i) => write!(f, "s{i}"),
            Operand::Func(i) => write!(f, "fn{i}"),
        }
    }
}

impl Display for Loc {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Loc::Local(l) => write!(f, "%{l}"),
            Loc::Word { ptr, word } => write!(f, "{ptr}[{word}]"),
        }
    }
}

impl Display for ArmEvent {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "arm.{}", self.mnemonic())?;
        match self {
            ArmEvent::Enter(n) => write!(f, " {n}"),
            _ => Ok(()),
        }
    }
}

impl Display for ScalarOp {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.write_str(self.mnemonic())?;
        match self {
            ScalarOp::WrapAs(t) | ScalarOp::SatAs(t) | ScalarOp::CheckedAs(t) => write!(f, ".{t}"),
            _ => Ok(()),
        }
    }
}

impl Display for Inst {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Inst::Copy { dst, src } => write!(f, "%{dst} = copy {src}"),
            Inst::Scalar {
                dst: d,
                op,
                ty,
                args,
            } => {
                dst(f, *d)?;
                write!(f, "{ty}.{op} ")?;
                operands(f, args)
            }
            Inst::Call { dst: d, func, args } => {
                dsts(f, d)?;
                write!(f, "call fn{func}(")?;
                operands(f, args)?;
                f.write_char(')')
            }
            Inst::CallIndirect {
                dst: d,
                code,
                args,
                params,
                ret,
            } => {
                dsts(f, d)?;
                write!(f, "call_indirect {code}(")?;
                operands(f, args)?;
                f.write_str(") : (")?;
                tys(f, params)?;
                f.write_str(") -> (")?;
                tys(f, ret)?;
                f.write_char(')')
            }
            Inst::Rt {
                dst: d,
                f: rt,
                args,
            } => {
                dst(f, *d)?;
                write!(f, "rt.{}(", rt.mnemonic())?;
                operands(f, args)?;
                f.write_char(')')
            }
            Inst::Load { dst, ptr, word } => write!(f, "%{dst} = load {ptr}[{word}]"),
            Inst::Store { ptr, word, src } => write!(f, "store {ptr}[{word}], {src}"),
            Inst::AddrOf { dst, local } => write!(f, "%{dst} = addr %{local}"),
            Inst::Atomic {
                dst: d,
                op,
                ty,
                order,
                failure,
                ptr,
                args,
            } => {
                dst(f, *d)?;
                write!(
                    f,
                    "atomic.{}.{ty} {} {} {ptr}",
                    op.mnemonic(),
                    order.mnemonic(),
                    failure.mnemonic()
                )?;
                for a in args {
                    write!(f, ", {a}")?;
                }
                Ok(())
            }
            Inst::Fence { order } => write!(f, "fence {}", order.mnemonic()),
            Inst::Arm(e) => e.fmt(f),
            Inst::Retag {
                ptr,
                prov,
                bytes,
                kind,
                protected,
            } => write!(
                f,
                "retag.{}{} {ptr}, %{prov}, {bytes}",
                match kind {
                    RetagKind::Unique => "unique",
                    RetagKind::Frozen => "frozen",
                    RetagKind::Cell => "cell",
                },
                if *protected { ".protected" } else { "" }
            ),
            Inst::Access {
                ptr,
                prov,
                bytes,
                write,
            } => write!(
                f,
                "access.{} {ptr}, {prov}, {bytes}",
                if *write { "w" } else { "r" }
            ),
            Inst::Move { places } => {
                f.write_str("move")?;
                for (i, p) in places.iter().enumerate() {
                    f.write_str(if i == 0 { " " } else { ", " })?;
                    p.fmt(f)?;
                }
                Ok(())
            }
        }
    }
}

fn blocks(f: &mut Formatter, bs: &[u32]) -> fmt::Result {
    f.write_char('[')?;
    list(f, bs, |f, b| write!(f, "bb{b}"))?;
    f.write_char(']')
}

impl Display for Term {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            Term::Jump(b) => write!(f, "jump bb{b}"),
            Term::Branch { cond, then, els } => write!(f, "br {cond}, bb{then}, bb{els}"),
            Term::Switch { on, targets } => {
                write!(f, "switch %{on} ")?;
                blocks(f, targets)
            }
            Term::Return(xs) => {
                f.write_str("ret")?;
                if xs.is_empty() {
                    return Ok(());
                }
                f.write_char(' ')?;
                operands(f, xs)
            }
            Term::TailCall { func, args } => {
                write!(f, "tailcall fn{func}(")?;
                operands(f, args)?;
                f.write_char(')')
            }
            Term::Panic(kind) => {
                f.write_str("panic ")?;
                quoted(f, kind)
            }
            Term::PanicFields { kind, fields } => {
                f.write_str("panic ")?;
                quoted(f, kind)?;
                f.write_str(" (")?;
                list(f, fields, |f, (name, v)| write!(f, "{name}: {v}"))?;
                f.write_char(')')
            }
            Term::Par { tasks, join } => {
                f.write_str("par ")?;
                blocks(f, tasks)?;
                write!(f, " join bb{join}")
            }
            Term::Suspend { resume } => write!(f, "suspend bb{resume}"),
            Term::TaskEnd => f.write_str("task_end"),
            Term::Unwind { drops } => write!(f, "unwind bb{drops}"),
        }
    }
}

/// A block as it prints inside function body: label, instructions, terminator. A nonzero source
/// line trails its instruction as ` @line`.
fn block(f: &mut Formatter, id: usize, b: &Block) -> fmt::Result {
    writeln!(f, "bb{id}:")?;
    for (inst, &line) in b.insts.iter().zip(&b.lines) {
        write!(f, "  {inst}")?;
        if line != 0 {
            write!(f, " @{line}")?;
        }
        f.write_char('\n')?;
    }
    writeln!(f, "  {}", b.term)
}

impl Func {
    fn print(&self, f: &mut Formatter, id: usize) -> fmt::Result {
        if self.unsafe_fn {
            f.write_str("unsafe ")?;
        }
        write!(f, "fn {id} ")?;
        quoted(f, &self.name)?;
        f.write_char('(')?;
        let params = self.params as usize;
        let first = &self.locals[..params];
        list(f, first, |f, t| t.fmt(f))?;
        f.write_str(") -> (")?;
        tys(f, &self.ret)?;
        f.write_str(") {\n")?;
        for (i, t) in self.locals.iter().enumerate().skip(params) {
            writeln!(f, "  local %{i}: {t}")?;
        }
        for group in &self.pinned {
            f.write_str("  pin ")?;
            list(f, group, |f, l| write!(f, "%{l}"))?;
            f.write_char('\n')?;
        }
        for (i, b) in self.blocks.iter().enumerate() {
            block(f, i, b)?;
        }
        f.write_str("}\n")
    }
}

impl Display for Func {
    /// A function on its own prints with id 0; inside a [`Program`] it prints with its own id.
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        self.print(f, 0)
    }
}

impl Display for Program {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        writeln!(f, "mir {VERSION}")?;
        writeln!(f, "main fn{}", self.main)?;
        for (i, s) in self.strings.iter().enumerate() {
            write!(f, "str {i} ")?;
            quoted(f, s)?;
            f.write_char('\n')?;
        }
        for (i, func) in self.funcs.iter().enumerate() {
            f.write_char('\n')?;
            func.print(f, i)?;
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{AtomicOp, MemOrder, RtFn};

    const I64: Ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    const U8: IntTy = IntTy {
        bits: 8,
        signed: false,
    };

    fn blk(insts: Vec<Inst>, term: Term) -> Block {
        let lines = (0..insts.len() as u32)
            .map(|i| if i == 0 { 3 } else { 0 })
            .collect();
        Block { insts, lines, term }
    }

    pub(crate) fn sample() -> Program {
        let main = Func {
            name: "main".into(),
            unsafe_fn: false,
            params: 0,
            ret: vec![Ty::Unit],
            locals: vec![I64, I64, Ty::Bool, Ty::Ptr, Ty::F64],
            pinned: vec![vec![1]],
            blocks: vec![
                blk(
                    vec![
                        Inst::Copy {
                            dst: 0,
                            src: Operand::Int(-7),
                        },
                        Inst::Scalar {
                            dst: Some(1),
                            op: ScalarOp::AddChecked,
                            ty: I64,
                            args: vec![Operand::Local(0), Operand::Int(1)],
                        },
                        Inst::Scalar {
                            dst: Some(0),
                            op: ScalarOp::WrapAs(U8),
                            ty: I64,
                            args: vec![Operand::Local(1)],
                        },
                        Inst::Call {
                            dst: vec![0, 1],
                            func: 1,
                            args: vec![Operand::Local(0)],
                        },
                        Inst::Rt {
                            dst: None,
                            f: RtFn::Print,
                            args: vec![Operand::Str(0)],
                        },
                        Inst::Copy {
                            dst: 4,
                            src: Operand::F64(0.5),
                        },
                        Inst::AddrOf { dst: 3, local: 1 },
                        Inst::Load {
                            dst: 0,
                            ptr: Operand::Local(3),
                            word: 2,
                        },
                        Inst::Store {
                            ptr: Operand::Local(3),
                            word: 2,
                            src: Operand::Int(9),
                        },
                        Inst::Atomic {
                            dst: Some(0),
                            op: AtomicOp::Cas,
                            ty: I64,
                            order: MemOrder::AcqRel,
                            failure: MemOrder::Acquire,
                            ptr: Operand::Local(3),
                            args: vec![Operand::Int(1), Operand::Int(2)],
                        },
                        Inst::Fence {
                            order: MemOrder::SeqCst,
                        },
                        Inst::Arm(ArmEvent::Enter(2)),
                        Inst::Move {
                            places: vec![
                                Loc::Local(0),
                                Loc::Word {
                                    ptr: Operand::Local(3),
                                    word: 1,
                                },
                            ],
                        },
                    ],
                    Term::Branch {
                        cond: Operand::Bool(true),
                        then: 1,
                        els: 2,
                    },
                ),
                blk(vec![], Term::Return(vec![Operand::Unit])),
                blk(
                    vec![],
                    Term::PanicFields {
                        kind: "overflow",
                        fields: vec![(Operand::Str(1), Operand::Local(0))],
                    },
                ),
            ],
        };
        let id = Func {
            name: "id \"x\"".into(),
            unsafe_fn: true,
            params: 1,
            ret: vec![I64, I64],
            locals: vec![I64],
            pinned: vec![],
            blocks: vec![blk(
                vec![],
                Term::Return(vec![Operand::Local(0), Operand::Local(0)]),
            )],
        };
        Program {
            funcs: vec![main, id],
            main: 0,
            strings: vec!["hi\n".into(), "a\"b\\c\u{1}é".into()],
        }
    }

    pub(crate) const GOLDEN: &str = r#"mir 1
main fn0
str 0 "hi\n"
str 1 "a\"b\\c\u{1}é"

fn 0 "main"() -> (unit) {
  local %0: i64
  local %1: i64
  local %2: bool
  local %3: ptr
  local %4: f64
  pin %1
bb0:
  %0 = copy -7 @3
  %1 = i64.add_checked %0, 1
  %0 = i64.wrap_as.u8 %1
  %0, %1 = call fn1(%0)
  rt.print(s0)
  %4 = copy 0.5
  %3 = addr %1
  %0 = load %3[2]
  store %3[2], 9
  %0 = atomic.cas.i64 acq_rel acquire %3, 1, 2
  fence seq_cst
  arm.enter 2
  move %0, %3[1]
  br true, bb1, bb2
bb1:
  ret ()
bb2:
  panic "overflow" (s1: %0)
}

unsafe fn 1 "id \"x\""(i64) -> (i64, i64) {
bb0:
  ret %0, %0
}
"#;

    #[test]
    fn golden() {
        assert_eq!(sample().to_string(), GOLDEN);
    }

    #[test]
    fn deterministic() {
        let p = sample();
        assert_eq!(p.to_string(), p.clone().to_string());
    }

    #[test]
    fn operand_forms_are_distinct() {
        let forms = [
            Operand::Int(1),
            Operand::F64(1.0),
            Operand::F64(-0.0),
            Operand::F64(1e300),
            Operand::F64(f64::INFINITY),
            Operand::F64(f64::NEG_INFINITY),
            Operand::F64(f64::NAN),
            Operand::Bool(false),
            Operand::Unit,
            Operand::Str(1),
            Operand::Func(1),
            Operand::Local(1),
        ];
        let text: Vec<_> = forms.iter().map(|o| o.to_string()).collect();
        assert_eq!(
            text,
            [
                "1",
                "1.0",
                "-0.0",
                "1e300",
                "inf",
                "-inf",
                "nan:0x7ff8000000000000",
                "false",
                "()",
                "s1",
                "fn1",
                "%1"
            ]
        );
    }

    #[test]
    fn task_terminators() {
        let t = |t: Term| t.to_string();
        assert_eq!(
            t(Term::Par {
                tasks: vec![1, 2],
                join: 3
            }),
            "par [bb1, bb2] join bb3"
        );
        assert_eq!(t(Term::Suspend { resume: 4 }), "suspend bb4");
        assert_eq!(t(Term::TaskEnd), "task_end");
        assert_eq!(t(Term::Unwind { drops: 5 }), "unwind bb5");
        assert_eq!(
            t(Term::Switch {
                on: 2,
                targets: vec![1, 3]
            }),
            "switch %2 [bb1, bb3]"
        );
        assert_eq!(
            t(Term::TailCall {
                func: 2,
                args: vec![]
            }),
            "tailcall fn2()"
        );
        assert_eq!(t(Term::Return(vec![])), "ret");
    }

    #[test]
    fn indirect_call() {
        let i = Inst::CallIndirect {
            dst: vec![2],
            code: Operand::Local(1),
            args: vec![Operand::Int(4)],
            params: vec![I64],
            ret: vec![Ty::Bool],
        };
        assert_eq!(i.to_string(), "%2 = call_indirect %1(4) : (i64) -> (bool)");
    }

    #[test]
    fn lowered_program_prints_one_canonical_text() {
        let src = "module t\nfn main() -> Unit {\n var n: i64 = 0\n\
                   loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n\
                   if big { break 'l }\n }\n assert eq_i64(let n, let 3)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty());
        let p = crate::lower(&m.unwrap()).expect("lowers");
        let text = p.to_string();
        assert!(text.starts_with("mir 1\nmain fn"));
        assert!(text.contains("i64.add_checked"));
        assert_eq!(text, p.clone().to_string());
    }
}
