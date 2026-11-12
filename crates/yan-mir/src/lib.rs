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
pub use lower::{Shape, layout_of, lower};
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
    /// `std.ct`: branchless on `int` operands (R611).
    CtSelect,
    CtEq,
    CtLt,
    /// `std.bits` and the shift operators on fixed-width integers (`n` is an `int`).
    Shl,
    Shr,
    WrappingShl,
    RotateLeft,
    RotateRight,
    CountOnes,
    CountZeros,
    LeadingZeros,
    TrailingZeros,
    BitWidth,
    SwapBytes,
    ReverseBits,
    IsPowerOfTwo,
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
    /// `(Str)`: the shown failure of `main` on stderr, then exit status 1; does not return.
    FailMain,
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
    /// `(Str, i64) -> Str`: `n` copies, none for `n <= 0`.
    StrRepeat,
    /// `(Str) -> i64`
    CharCount,
    /// `(Str) -> i64`
    ByteLen,
    /// `(Str) -> i64`, panics `conversion`
    IntOf,
    /// `(f64) -> i64`, panics `conversion` (`int(x)`)
    TruncF64,
    /// `(f64) -> i64`, panics `conversion` (`round(x)`)
    RoundF64,
    /// `(i64) -> f64`
    IntToF64,
    /// `(i64) -> i64`, panics `overflow`
    AbsI64,
    /// `(f64) -> f64`
    AbsF64,
    /// `(f64) -> f64`
    Floor,
    /// `(f64) -> f64`
    Ceil,
    /// `(f64) -> f64`
    TruncF,
    /// `(f64) -> f64`
    NextUp,
    /// `(f64) -> f64`
    NextDown,
    /// `(f64, f64) -> f64`
    Fmod,
    Pown,
    Pow,
    /// `(op, f64) -> f64`, op indexes `lower::math::MATH1`.
    /// `(f64, i64) -> f64`
    Ldexp,
    /// `(f64) -> f64`, the mantissa of `frexp`.
    FrexpM,
    /// `(f64) -> i64`, the exponent of `frexp`.
    FrexpE,
    /// `(str, tag, str) -> List`; the separator is used when `tag` is 0, else whitespace splits.
    StrSplit,
    /// `(str) -> List`
    StrSplitLines,
    /// `(str) -> bytes`
    BytesOfStr,
    RegexCheck,
    RegexEscape,
    RegexSplit,
    RegexSub,
    RegexFind,
    RegexFindAll,
    CharStr,
    StrChars,
    CharAt,
    CharPred,
    StrPred,
    StrCount,
    ChrCheck,
    PathNew,
    PathOp,
    PathWith,
    PathJoin,
    PathIsAbsolute,
    PathStartsWith,
    PathRelativeTo,
    PathParts,
    /// `(list[uint8]) -> bytes`
    BytesFromList,
    /// `(bytes)`
    BytesFree,
    /// `(bytes) -> bytes`
    BytesCopy,
    /// `(bytes) -> int`
    BytesLen,
    /// `(bytes, bytes) -> bytes`
    BytesConcat,
    /// `(bytes) -> list[uint8]`
    BytesToList,
    /// `(bytes, bytes) -> bool`
    BytesStarts,
    /// `(bytes, bytes) -> bool`
    BytesEnds,
    /// `(bytes) -> str`
    BytesHex,
    /// `(bytes) -> str`
    BytesLossy,
    /// `(bytes) -> str`, CPython's repr (R584).
    BytesRepr,
    /// `(bytes, i) -> *u64` a word holding the byte; panics out of range. Valid until the next call.
    BytesIndex,
    /// `(bytes, i) -> int`, the byte or -1 when `i` is out of range.
    BytesAt,
    /// `(bytes, bytes) -> int`, the first index or -1.
    BytesFind,
    /// `(bytes, i, j) -> bytes`; panics out of range.
    BytesSlice,
    /// `(bytes) -> int`: -1 when UTF-8, else the valid prefix length.
    Utf8Check,
    /// `(str) -> int`: -1 when valid hex, else the first fault.
    HexCheck,
    /// `(str) -> bytes` of a string `HexCheck` accepted.
    HexDecode,
    /// `(bytes, url, pad) -> str`
    Base64,
    /// `(str, url) -> int`: -1 when strict base64, else the first fault.
    Base64Check,
    /// `(str, url) -> bytes` of a string `Base64Check` accepted.
    Base64Decode,
    /// `(at, reason) -> str`, `invalid <kind> at <at>` (R583).
    EncodingErrorShow,
    /// `(schema, a, b) -> int`: -1, 0 or 1 by the key order of the schema.
    KeyCmp,
    /// `(list, i) -> *slots`, the new element's slots.
    ListInsert,
    /// `(list, i, j) -> list`, a shallow copy; panics out of range.
    ListSliceExact,
    /// `(str, out) -> bool`, parsing into the word at `out`.
    ParseInt,
    /// `(str, out) -> bool`, parsing into the word at `out`.
    ParseFloat,
    Math1,
    /// `(op, f64, f64) -> f64`, op indexes `lower::math::MATH2`.
    Math2,
    /// `(op, mode, f64, f64, f64) -> f64`: op 0 add, 1 sub, 2 mul, 3 div, 4 fma, 5 sqrt; mode is the
    /// `math.Round` tag (nearest, up, down, toward zero).
    Rounded,
    /// `(xs, ys or 0, mode) -> f64`: the exactly rounded sum, or dot product, of float lists.
    DotExact,
    /// `(f64, f64) -> f64`
    Copysign,
    /// `(f64) -> bool`
    IsNan,
    /// `(f64) -> bool`
    IsInf,
    /// `(f64) -> bool`
    IsFinite,
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
    KeyEq,
    ListReverse,
    /// `(Str, 3 x (tag, value)) -> Str`: the chars picked by a slice (N50).
    StrSlice,
    /// `(List, 3 x (tag, value)) -> List`: the elements picked, shallow-copied.
    ListSlice,
    /// `(i64 kind, i64 n, i64 r) -> List` of one-word elements: the number of selections, then
    /// each selection's `r` indices (`yan_rt::select`).
    Select,
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
    /// `(Ptr out, Ptr value, Str schema, i64 start) -> i64`: the value after the `Wire` round
    /// trip into `out`; 0, or 1 when the codec rejects it. A rejected start argument panics.
    WireRoundtrip,
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
    /// `(state, words, capacity, policy, restart, thunk, code, env) -> actor` (`yan_rt::actor`).
    ActorSpawn,
    /// `(actor, out, in_task) -> status`: a handler takes the state.
    ActorTake,
    /// `(actor, state)`: the state goes back.
    ActorPut,
    /// `(actor, state) -> starts`: a `@reentrant` handler lends the state back.
    ActorLend,
    /// `(actor, starts, out, in_task) -> status`: a `@reentrant` handler takes it back.
    ActorRetake,
    /// `(actor, state) -> bool`: a handler call ends; whether its failure stops the actor.
    ActorGive,
    /// `(actor, thunk, code, env, args, in_task) -> status`: enqueue a message.
    ActorSend,
    /// `(i) -> u64`: a work slot.
    ActorArg,
    /// `()`: the message in the work slots ran.
    ActorRan,
    /// `() -> bool`: an idle actor's message into the work slots.
    ActorIdle,
    /// `(actor, shown failure)`: a handler failed.
    ActorCrash,
    /// `(actor) -> kind`: why a stopped actor stopped, 1 `Shutdown` or 2 `Crashed`.
    ActorReason,
    /// `(actor) -> str`: the shown failure of a crashed actor.
    ActorReasonText,
    /// `() -> bool`: the next restart into the work slots.
    ActorRestartNext,
    /// `(actor, state)`: a restarted actor's new state.
    ActorRestarted,
    /// `(low, strategy, intensity, period) -> low`: open a supervisor scope.
    SupBegin,
    /// `()`: end the innermost supervisor scope.
    SupEnd,
    /// `(depth)`: end the scopes past `depth`.
    SupEndTo,
    /// `(nodes, seed, Ptr plan) -> sim`: a `sim` cluster (RFC 0063 D3).
    SimNew,
    /// `(low, sim) -> low`: `with cluster = sim`, a supervisor scope with each node's root.
    SimBegin,
    /// `() -> List[Node]`: the bound cluster's nodes.
    ClusterNodes,
    /// `() -> Node`: the node the calling task runs on (D1).
    ClusterSelfNode,
    /// `(Node)`: the calling task runs on that node until `ClusterLeave`.
    ClusterPlace,
    /// `()`: the calling task returns from the node it was placed on.
    ClusterLeave,
    /// `(actor)`: the request message of a call through `Remote` (D5).
    ClusterReach,
    /// `(actor)`: the message of a send through `Remote`.
    ClusterFaultSend,
    /// `(status)`: a send through `Remote` whose argument was rejected fails `SchemaMismatch`.
    ClusterSendMismatch,
    /// `(mismatch)`: a call through `Remote` whose argument was rejected ends.
    ClusterAdmit,
    /// `(mismatch) -> lost`: the reply message of a call through `Remote`.
    ClusterArrive,
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
                | RtFn::ListSliceExact
                | RtFn::Base64
                | RtFn::Base64Decode
                | RtFn::EncodingErrorShow
                | RtFn::HexDecode
                | RtFn::BytesSlice
                | RtFn::BytesOfStr
                | RtFn::RegexCheck
                | RtFn::RegexEscape
                | RtFn::RegexSplit
                | RtFn::RegexSub
                | RtFn::RegexFind
                | RtFn::RegexFindAll
                | RtFn::CharStr
                | RtFn::StrChars
                | RtFn::PathNew
                | RtFn::PathOp
                | RtFn::PathWith
                | RtFn::PathJoin
                | RtFn::PathRelativeTo
                | RtFn::PathParts
                | RtFn::BytesFromList
                | RtFn::BytesCopy
                | RtFn::BytesConcat
                | RtFn::BytesToList
                | RtFn::BytesHex
                | RtFn::BytesLossy
                | RtFn::BytesRepr
                | RtFn::ListNew
                | RtFn::ListPush
                | RtFn::ListCopy
                | RtFn::Select
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
    /// The branches of a `par concurrent` as tasks, then `join`. This and the next three exist
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
    /// The running task leaves the function, and if the block keeps its result (R394) the block
    /// continues at `drops` once every task ended: the drops of the enclosing frame, run once.
    Unwind {
        drops: BlockId,
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
    /// Takes no arguments and returns `unit`: a failing `main` is wrapped (`lower/entry.rs`).
    pub main: FuncId,
    pub strings: Vec<String>,
}
