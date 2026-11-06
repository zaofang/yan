//! Mid-level IR (22.2, R560): one control-flow graph per function over typed locals, with std
//! calls named by registry id (R561). It is lowered from checked Yan Core and feeds the code
//! generators; the interpreter keeps running checked Yan Core.
//!
//! Locals are scalars. Records, tuples and non-recursive sums are split into one local per
//! scalar slot (a sum is a tag slot followed by every variant's slots), and `inout` parameters
//! come back as extra results. `Str` and `List` are pointers to yan-rt blocks, which are not
//! freed yet. Anything else is reported as unsupported.

mod layout;
mod lower;

pub use layout::{Layout, SumLayout, layout};
pub use lower::{Shape, lower};
use yan_std::registry::Id;

pub type Local = u32;
pub type BlockId = u32;
pub type FuncId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    Int(IntTy),
    F64,
    Bool,
    Unit,
    /// A yan-rt block (`Str`, `List`).
    Ptr,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Local(Local),
    Int(i128),
    F64(f64),
    Bool(bool),
    Unit,
    /// A `Str` literal, by index into [`Program::strings`].
    Str(u32),
    /// A function's code address.
    Func(FuncId),
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
    /// The results go to `dst` in order: the callee's result slots, then its `inout` slots.
    Call {
        dst: Vec<Local>,
        func: FuncId,
        args: Vec<Operand>,
    },
    /// A call of a code address with the tail convention and these slot types.
    CallIndirect {
        dst: Vec<Local>,
        code: Operand,
        args: Vec<Operand>,
        params: Vec<Ty>,
        ret: Vec<Ty>,
    },
    /// A yan-rt C-ABI function.
    Rt {
        dst: Option<Local>,
        f: RtFn,
        args: Vec<Operand>,
    },
    /// `dst = ptr[word]`, one 8-byte word per slot.
    Load {
        dst: Local,
        ptr: Operand,
        word: u32,
    },
    Store {
        ptr: Operand,
        word: u32,
        src: Operand,
    },
}

/// The yan-rt entry points native code calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtFn {
    /// `(Str)`
    Print,
    /// `(Str, Str) -> Str`
    Concat,
    /// `(i64) -> Str`
    ShowI64,
    /// `(u64) -> Str`
    ShowU64,
    /// `(f64) -> Str`
    ShowF64,
    /// `(i64, i64) -> f64`, correctly rounded `int / int`.
    DivI64,
    /// `(u64, u64) -> f64`, correctly rounded.
    DivU64,
    /// `(bool) -> Str`
    ShowBool,
    /// `(Str) -> Str`, CPython `repr`.
    Repr,
    /// `(Str, Str) -> bool`
    StrEq,
    /// `(words) -> List` with elements of `words` slots.
    ListNew,
    /// `(List) -> Ptr` to a new last element's slots.
    ListPush,
    /// `(List) -> i64`
    ListLen,
    /// `(List, i64) -> Ptr` to an element's slots; panics `bounds`.
    ListAt,
    /// `(List) -> List`, a copy of the slots.
    ListCopy,
    /// `(Ptr out, Str, Str)`: the slots of `Result[T, JsonError]` into `out`.
    JsonParse,
    /// `(Ptr value, Str schema) -> Str`
    JsonDump,
    /// `(Ptr slots) -> Str`: the text of a `JsonError`.
    JsonErrorShow,
    /// `(Ptr out, Str, Str, Str)`: the slots of `Result[list[T], CsvError]` into `out`.
    CsvParse,
    /// `(List rows, Str delimiter, Str schema) -> Str`
    CsvDump,
    /// `(Ptr slots) -> Str`: the text of a `CsvError`.
    CsvErrorShow,
    /// `(words) -> Ptr` to that many zeroed words.
    Alloc,
    /// `(Str)`: free the block; a literal's block is left alone.
    StrFree,
    /// `(List)`: free the block, not its elements.
    ListFree,
    /// `(Ptr)`: free a closure's captures block; null is ignored.
    EnvFree,
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
    Return(Vec<Operand>),
    TailCall {
        func: FuncId,
        args: Vec<Operand>,
    },
    /// A panic of this kind (8.3).
    Panic(&'static str),
    /// A panic of this kind with named scalar operands (R796): each field is a name (a `Str`
    /// literal) and its value.
    PanicFields {
        kind: &'static str,
        fields: Vec<(Operand, Operand)>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub insts: Vec<Inst>,
    /// The source line of each instruction (0: none); same length as `insts`.
    pub lines: Vec<u32>,
    pub term: Term,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Func {
    pub name: String,
    /// The first `params` locals are the parameters, in order.
    pub params: u32,
    /// The result slots, then the final value of each `inout` parameter's slots.
    pub ret: Vec<Ty>,
    pub locals: Vec<Ty>,
    /// Block 0 is the entry.
    pub blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub funcs: Vec<Func>,
    pub main: FuncId,
    /// `main` returns a `Result`, whose tag is its first slot; `Err` exits with status 1.
    pub main_fails: bool,
    pub strings: Vec<String>,
}
