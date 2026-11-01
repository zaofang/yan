//! Mid-level IR (22.2, R560): one control-flow graph per function over typed locals, with std
//! calls named by registry id (R561). It is lowered from checked Yong and feeds the code
//! generators; the interpreter keeps running checked Yong.
//!
//! This first cut covers scalar programs: integers up to 64 bits, `f64`, `bool` and `Unit`
//! locals, direct and tail calls, loops, join points, contracts and assertions, and printing a
//! literal. Anything else is reported as unsupported.

mod lower;

pub use lower::lower;
use yong_std::registry::Id;

pub type Local = u32;
pub type BlockId = u32;
pub type FuncId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntTy {
    pub bits: u8,
    pub signed: bool,
}

impl IntTy {
    pub fn range(self) -> (i128, i128) {
        if self.signed {
            let m = 1i128 << (self.bits - 1);
            (-m, m - 1)
        } else {
            (0, (1i128 << self.bits) - 1)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ty {
    Int(IntTy),
    F64,
    Bool,
    Unit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Local(Local),
    Int(i128),
    F64(f64),
    Bool(bool),
    Unit,
    /// A string literal, by index into [`Program::strings`].
    Str(u32),
}

/// A scalar intrinsic, decoded from its `<op>_<ty>` path once at lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarOp {
    AddChecked,
    SubChecked,
    MulChecked,
    FloorDiv,
    Mod,
    /// Integer operands, `f64` result.
    Div,
    And,
    Or,
    Xor,
    Not,
    /// Float arithmetic, which never panics.
    Add,
    Sub,
    Mul,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Panics when its two operands are equal (`exclusive_check`).
    Exclusive,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    Copy {
        dst: Local,
        src: Operand,
    },
    Scalar {
        dst: Option<Local>,
        id: Id,
        op: ScalarOp,
        /// The operand type.
        ty: Ty,
        args: Vec<Operand>,
    },
    Call {
        dst: Option<Local>,
        func: FuncId,
        args: Vec<Operand>,
    },
    /// A std function bound in yong-rt.
    Std {
        dst: Option<Local>,
        id: Id,
        args: Vec<Operand>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Jump(BlockId),
    Branch {
        cond: Operand,
        then: BlockId,
        els: BlockId,
    },
    /// To `targets[on]`.
    Switch {
        on: Local,
        targets: Vec<BlockId>,
    },
    Return(Operand),
    TailCall {
        func: FuncId,
        args: Vec<Operand>,
    },
    /// A panic of this kind (8.3).
    Panic(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Func {
    pub name: String,
    /// The first `params` locals are the parameters, in order.
    pub params: u32,
    pub ret: Ty,
    pub locals: Vec<Ty>,
    /// Block 0 is the entry.
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub funcs: Vec<Func>,
    pub main: FuncId,
    pub strings: Vec<String>,
}
