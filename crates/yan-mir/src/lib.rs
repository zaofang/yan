//! Mid-level IR (22.2, R560): one control-flow graph per function over typed locals, with std
//! calls named by registry id (R561). It is lowered from checked Yan Core and feeds the code
//! generators; the interpreter keeps running checked Yan Core.
//!
//! Locals are scalars. Records, tuples and non-recursive sums are split into one local per
//! scalar slot (a sum is a tag slot followed by every variant's slots), and `inout` parameters
//! come back as extra results. `Str` and `List` are pointers to yan-rt blocks, which are not
//! freed yet. Anything else is reported as unsupported.

mod co;
mod layout;
mod lower;
mod tasks;

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
    /// `() -> Str`, a null pointer at end of input
    ReadLine,
    /// `() -> Str`
    ReadAll,
    /// `(Str) -> Str`, a null pointer on failure
    FsRead,
    /// `() -> Str`, why the last `FsRead` failed
    FsReason,
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
    /// `(Str) -> i64`
    CharCount,
    /// `(Str) -> i64`
    ByteLen,
    /// `(Str) -> i64`, panics `conversion`
    IntOf,
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
    /// `(key words, value words, Str key schema) -> Map`.
    MapNew,
    /// `(Map, Ptr key) -> Ptr` to the entry's value slots, or null.
    MapFind,
    /// `(Map, Ptr key) -> Ptr` to the entry's value slots; panics `bounds`.
    MapIndex,
    /// `(Map, Ptr key) -> Ptr` to a new entry's value slots; the key is not in the map.
    MapPush,
    /// `(Map, Ptr key, Ptr out) -> i64`: 1 with the entry's key and value slots in `out`, or 0.
    MapRemove,
    /// `(Map) -> i64`
    MapLen,
    /// `(Map, i64) -> Ptr` to the key then value slots of an entry, in insertion order.
    MapAt,
    /// `(List, schema)`: restore heap order after an append.
    HeapUp,
    /// `(List, schema)`: heapify.
    HeapFix,
    /// `(List, schema, Ptr out) -> i64`: 1 with the least element in `out`, or 0.
    HeapPop,
    /// `(List, schema, Ptr x, Ptr out)`: push `x`, pop the least into `out`.
    HeapPushPop,
    /// `(List, schema)`: sort ascending in place.
    HeapSort,
    /// `(Str, 3 x (tag, value)) -> Str`: the chars picked by a slice (N50).
    StrSlice,
    /// `(List, 3 x (tag, value)) -> List`: the elements picked, shallow-copied.
    ListSlice,
    /// `(List, i64)`: set a deque's `maxlen`.
    DqLimit,
    /// `(List) -> i64`: a deque's `maxlen`, or -1.
    DqMaxlen,
    /// `(List, i64 front, Ptr x, Ptr out) -> i64`: push `x`; 1 with the element pushed out in `out`.
    DqPush,
    /// `(List, i64 front, Ptr out) -> i64`: 1 with the end element in `out`, or 0.
    DqPop,
    /// `(List, List) -> List`: append the second (consumed); the elements pushed out.
    DqExtend,
    /// `(List, i64)`: rotate right.
    DqRotate,
    /// `(List) -> List`: every element moved to a new list.
    DqTake,
    /// `(List, schema, Ptr x) -> i64`: 1 when `x` is an element.
    DqContains,
    /// `(List) -> Str`: `, maxlen=N`, or empty.
    DqSuffix,
    /// `(Map) -> Map`, a copy of the slots.
    MapCopy,
    /// `(Map)`: free the block, not its entries.
    MapFree,
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
    /// `(cap, words) -> Ptr`: a channel of `words`-slot elements, shared by both ends.
    ChanNew,
    /// `(Ptr chan, Ptr slots, in_task) -> i64`: 1 when it parked the task, 2 when the task is
    /// cancelled.
    ChanSend,
    /// `(Ptr chan, Ptr out, in_task) -> i64`: the oldest element's slots into `out`; 1 and 2
    /// as for `ChanSend`.
    ChanRecv,
    /// `(Ptr chan)`: drop one end.
    ChanDrop,
    /// `(n, can_yield) -> handle`: enter a `par concurrent` of `n` tasks.
    ParBegin,
    /// `(handle) -> i64`: the task to run next, the task count when all are done, or past it
    /// when the block yields to its enclosing task.
    ParNext,
    /// `(handle)`: the running task's branch ended.
    ParTaskEnd,
    /// `(kind)`: the task about to leave the function fails (1), or fails `Cancelled` (2).
    /// Without it, it returns (0).
    ParExitKind,
    /// `(handle) -> i64`: the running task leaves the function and cancels the others; 1 if
    /// its result replaces the one kept so far: the first, or a lower-index failure that is
    /// not `Cancelled` (R394).
    ParCancel,
    /// `(handle)`: leave the block.
    ParEnd,
    /// `(resource, bound, clause) -> token`: start a `cost` counter (R610); 0 steps, 1 alloc.
    CostBegin,
    /// `(token)`: end the counters started since `token`.
    CostEnd,
    /// `(resource, n)`: count `n`; panics `contract` past a running bound.
    CostCount,
}

impl RtFn {
    /// Whether the call makes one `alloc` (R925): a fresh `Str` or `List`, or a `List` grown.
    pub fn allocates(self) -> bool {
        matches!(
            self,
            RtFn::Concat
                | RtFn::ReadLine
                | RtFn::ReadAll
                | RtFn::FsRead
                | RtFn::FsReason
                | RtFn::ShowI64
                | RtFn::ShowU64
                | RtFn::ShowF64
                | RtFn::ShowBool
                | RtFn::Repr
                | RtFn::ListNew
                | RtFn::ListPush
                | RtFn::ListCopy
                | RtFn::MapNew
                | RtFn::MapPush
                | RtFn::MapCopy
                | RtFn::JsonDump
                | RtFn::JsonErrorShow
                | RtFn::CsvDump
                | RtFn::CsvErrorShow
        )
    }
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
    /// The branches of a `par concurrent` as tasks, then `join`. This and the next two exist
    /// only between lowering and the task transform (RFC 0043).
    Par {
        tasks: Vec<BlockId>,
        join: BlockId,
    },
    /// The running task yields; it continues at `resume`.
    Suspend {
        resume: BlockId,
    },
    /// The running task's branch ended.
    TaskEnd,
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
