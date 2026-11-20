//! Canonical mnemonics for the MIR text format (RFC 0097 D5). One table per enum: the printer
//! reads `mnemonic`, the parser reads `from_mnemonic`. Runtime calls print as `rt.<mnemonic>`.

use crate::{ArmEvent, AtomicOp, MemOrder, RtFn, ScalarOp};

macro_rules! mnemonics {
    ($ty:ident { $($v:ident = $name:literal,)* } $(; $($extra:pat => $en:literal),*)?) => {
        impl $ty {
            pub const MNEMONICS: &[(&str, $ty)] = &[$(($name, $ty::$v)),*];

            pub fn mnemonic(self) -> &'static str {
                match self {
                    $($ty::$v => $name,)*
                    $($($extra => $en,)*)?
                }
            }

            pub fn from_mnemonic(s: &str) -> Option<$ty> {
                match s {
                    $($name => Some($ty::$v),)*
                    _ => None,
                }
            }
        }
    };
}

mnemonics!(ScalarOp {
    AddChecked = "add_checked",
    SubChecked = "sub_checked",
    MulChecked = "mul_checked",
    FloorDiv = "floor_div",
    Mod = "mod",
    Div = "div",
    And = "and",
    Or = "or",
    Xor = "xor",
    Not = "not",
    Add = "add",
    Sub = "sub",
    Mul = "mul",
    Eq = "eq",
    Ne = "ne",
    Lt = "lt",
    Le = "le",
    Gt = "gt",
    Ge = "ge",
    Exclusive = "exclusive",
    CtSelect = "ct_select",
    CtEq = "ct_eq",
    CtLt = "ct_lt",
    Shl = "shl",
    Shr = "shr",
    WrappingShl = "wrapping_shl",
    RotateLeft = "rotate_left",
    RotateRight = "rotate_right",
    CountOnes = "count_ones",
    CountZeros = "count_zeros",
    LeadingZeros = "leading_zeros",
    TrailingZeros = "trailing_zeros",
    BitWidth = "bit_width",
    SwapBytes = "swap_bytes",
    ReverseBits = "reverse_bits",
    IsPowerOfTwo = "is_power_of_two",
}; ScalarOp::WrapAs(_) => "wrap_as", ScalarOp::SatAs(_) => "sat_as", ScalarOp::CheckedAs(_) => "checked_as");

mnemonics!(AtomicOp {
    Load = "load",
    Store = "store",
    Swap = "swap",
    Cas = "cas",
    Add = "add",
    Sub = "sub",
    And = "and",
    Or = "or",
    Xor = "xor",
    Max = "max",
    Min = "min",
});

mnemonics!(MemOrder {
    Relaxed = "relaxed",
    Acquire = "acquire",
    Release = "release",
    AcqRel = "acq_rel",
    SeqCst = "seq_cst",
});

mnemonics!(RtFn {
    Print = "print",
    Eprint = "eprint",
    FailMain = "fail_main",
    ReadLine = "read_line",
    ReadAll = "read_all",
    FsRead = "fs_read",
    FsListDir = "fs_list_dir",
    FsFake = "fs_fake",
    LogNew = "log_new",
    LogSpan = "log_span",
    LogEvent = "log_event",
    Civil = "civil",
    CivilOut = "civil_out",
    CivilOutF = "civil_out_f",
    ZoneLookup = "zone_lookup",
    ZoneId = "zone_id",
    EnvFake = "env_fake",
    EnvOnly = "env_only",
    EnvArgs = "env_args",
    EnvVar = "env_var",
    EnvVars = "env_vars",
    EnvZone = "env_zone",
    FsReason = "fs_reason",
    Concat = "concat",
    ShowI64 = "show_i64",
    ShowU64 = "show_u64",
    ShowF64 = "show_f64",
    DivI64 = "div_i64",
    DivU64 = "div_u64",
    ShowBool = "show_bool",
    Repr = "repr",
    StrEq = "str_eq",
    StrRepeat = "str_repeat",
    ShowFixed = "show_fixed",
    GraphemeCount = "grapheme_count",
    RandSeeded = "rand_seeded",
    RandSplit = "rand_split",
    RandNext = "rand_next",
    RandF64 = "rand_f64",
    RandBelow = "rand_below",
    PtrExpose = "ptr_expose",
    PtrExposed = "ptr_exposed",
    StrPartition = "str_partition",
    CharCount = "char_count",
    ByteLen = "byte_len",
    IntOf = "int_of",
    FloatOf = "float_of",
    TruncF64 = "trunc_f64",
    RoundF64 = "round_f64",
    IntToF64 = "int_to_f64",
    AbsI64 = "abs_i64",
    AbsF64 = "abs_f64",
    Floor = "floor",
    Ceil = "ceil",
    TruncF = "trunc_f",
    NextUp = "next_up",
    NextDown = "next_down",
    Fmod = "fmod",
    Pown = "pown",
    Pow = "pow",
    Ldexp = "ldexp",
    FrexpM = "frexp_m",
    FrexpE = "frexp_e",
    StrSplit = "str_split",
    StrSplitLines = "str_split_lines",
    StrUpper = "str_upper",
    StrLower = "str_lower",
    StrStrip = "str_strip",
    StrStarts = "str_starts",
    StrEnds = "str_ends",
    StrContains = "str_contains",
    StrContainsChar = "str_contains_char",
    StrReplace = "str_replace",
    StrJoin = "str_join",
    BytesOfStr = "bytes_of_str",
    RegexCheck = "regex_check",
    RegexEscape = "regex_escape",
    RegexSplit = "regex_split",
    RegexSub = "regex_sub",
    RegexFind = "regex_find",
    RegexFindAll = "regex_find_all",
    CharStr = "char_str",
    StrChars = "str_chars",
    CharAt = "char_at",
    CharPred = "char_pred",
    StrPred = "str_pred",
    StrCount = "str_count",
    ChrCheck = "chr_check",
    PathNew = "path_new",
    PathOp = "path_op",
    PathWith = "path_with",
    PathJoin = "path_join",
    PathIsAbsolute = "path_is_absolute",
    PathStartsWith = "path_starts_with",
    PathRelativeTo = "path_relative_to",
    PathParts = "path_parts",
    PathWinPart = "path_win_part",
    PathToPosix = "path_to_posix",
    WinPathEq = "win_path_eq",
    BytesFromList = "bytes_from_list",
    BytesFree = "bytes_free",
    BytesCopy = "bytes_copy",
    BytesSet = "bytes_set",
    BytesPush = "bytes_push",
    BytesExtend = "bytes_extend",
    BytesPop = "bytes_pop",
    BytesClear = "bytes_clear",
    BytesLen = "bytes_len",
    BytesConcat = "bytes_concat",
    BytesToList = "bytes_to_list",
    BytesStarts = "bytes_starts",
    BytesEnds = "bytes_ends",
    BytesHex = "bytes_hex",
    BytesLossy = "bytes_lossy",
    BytesRepr = "bytes_repr",
    BigFromInt = "big_from_int",
    BigBin = "big_bin",
    BigCmp = "big_cmp",
    BigShift = "big_shift",
    BigPow = "big_pow",
    BigNot = "big_not",
    BigBits = "big_bits",
    BigStr = "big_str",
    TimeOp = "time_op",
    TimeF64 = "time_f64",
    TimeShow = "time_show",
    BytesIndex = "bytes_index",
    BytesAt = "bytes_at",
    BytesFind = "bytes_find",
    BytesSlice = "bytes_slice",
    Utf8Check = "utf8_check",
    HexCheck = "hex_check",
    HexDecode = "hex_decode",
    Base64 = "base64",
    Base64Check = "base64_check",
    Base64Decode = "base64_decode",
    EncodingErrorShow = "encoding_error_show",
    KeyCmp = "key_cmp",
    ListInsert = "list_insert",
    ListRemove = "list_remove",
    ListSliceExact = "list_slice_exact",
    ParseInt = "parse_int",
    ParseFloat = "parse_float",
    Math1 = "math1",
    Math2 = "math2",
    Rounded = "rounded",
    DotExact = "dot_exact",
    SumF64 = "sum_f64",
    Copysign = "copysign",
    IsNan = "is_nan",
    IsInf = "is_inf",
    IsFinite = "is_finite",
    ListNew = "list_new",
    ListPush = "list_push",
    ListLen = "list_len",
    ListData = "list_data",
    ListAt = "list_at",
    ListCopy = "list_copy",
    MapNew = "map_new",
    MapFind = "map_find",
    MapIndex = "map_index",
    MapPush = "map_push",
    MapRemove = "map_remove",
    MapLen = "map_len",
    MapAt = "map_at",
    HeapUp = "heap_up",
    HeapFix = "heap_fix",
    HeapPop = "heap_pop",
    HeapPushPop = "heap_push_pop",
    HeapSort = "heap_sort",
    KeyEq = "key_eq",
    ListReverse = "list_reverse",
    StrSlice = "str_slice",
    ListSlice = "list_slice",
    Select = "select",
    DqLimit = "dq_limit",
    DqMaxlen = "dq_maxlen",
    DqPush = "dq_push",
    DqPop = "dq_pop",
    DqExtend = "dq_extend",
    DqRotate = "dq_rotate",
    DqTake = "dq_take",
    DqContains = "dq_contains",
    DqSuffix = "dq_suffix",
    MapCopy = "map_copy",
    MapFree = "map_free",
    JsonParse = "json_parse",
    JsonDump = "json_dump",
    JsonErrorShow = "json_error_show",
    JsonParseValue = "json_parse_value",
    JsonDumpValue = "json_dump_value",
    CsvParse = "csv_parse",
    CsvDump = "csv_dump",
    CsvErrorShow = "csv_error_show",
    WireRoundtrip = "wire_roundtrip",
    Alloc = "alloc",
    StrFree = "str_free",
    ListFree = "list_free",
    EnvFree = "env_free",
    ArenaNew = "arena_new",
    ArenaAlloc = "arena_alloc",
    ArenaFree = "arena_free",
    ChanNew = "chan_new",
    ChanSend = "chan_send",
    ChanRecv = "chan_recv",
    ChanTrySend = "chan_try_send",
    ChanTryRecv = "chan_try_recv",
    FutexWait = "futex_wait",
    FutexNotify = "futex_notify",
    SelectWait = "select_wait",
    ChanDrain = "chan_drain",
    ChanDrop = "chan_drop",
    ChanDropSender = "chan_drop_sender",
    ChanClone = "chan_clone",
    ChanCloneSender = "chan_clone_sender",
    ParBegin = "par_begin",
    ParNext = "par_next",
    ParTaskEnd = "par_task_end",
    ParExitKind = "par_exit_kind",
    ParCancel = "par_cancel",
    ParEnd = "par_end",
    PoolBegin = "pool_begin",
    PoolRun = "pool_run",
    ActorSpawn = "actor_spawn",
    ActorTake = "actor_take",
    ActorPut = "actor_put",
    ActorLend = "actor_lend",
    ActorRetake = "actor_retake",
    ActorGive = "actor_give",
    ActorSend = "actor_send",
    ActorArg = "actor_arg",
    ActorRan = "actor_ran",
    ActorIdle = "actor_idle",
    ActorCrash = "actor_crash",
    ActorReason = "actor_reason",
    ActorReasonText = "actor_reason_text",
    ActorRestartNext = "actor_restart_next",
    ActorRestarted = "actor_restarted",
    SupBegin = "sup_begin",
    SupEnd = "sup_end",
    SupEndTo = "sup_end_to",
    DeadlineBegin = "deadline_begin",
    ShieldBegin = "shield_begin",
    DeadlineEnd = "deadline_end",
    DeadlineEndTo = "deadline_end_to",
    FfiCall = "ffi_call",
    FfiCallF64 = "ffi_call_f64",
    SimNew = "sim_new",
    SimBegin = "sim_begin",
    ClusterNodes = "cluster_nodes",
    ClusterSelfNode = "cluster_self_node",
    ClusterPlace = "cluster_place",
    ClusterLeave = "cluster_leave",
    ClusterReach = "cluster_reach",
    ClusterFaultSend = "cluster_fault_send",
    SimPct = "sim_pct",
    ProcAmbient = "proc_ambient",
    ClusterReplied = "cluster_replied",
    SimCheck = "sim_check",
    ClusterMonitor = "cluster_monitor",
    ClusterGroup = "cluster_group",
    GroupJoin = "group_join",
    GroupLeave = "group_leave",
    GroupMembers = "group_members",
    ClusterSendMismatch = "cluster_send_mismatch",
    ClusterAdmit = "cluster_admit",
    ClusterArrive = "cluster_arrive",
    CostBegin = "cost_begin",
    CostEnd = "cost_end",
    CostCount = "cost_count",
});

impl ArmEvent {
    pub fn mnemonic(self) -> &'static str {
        match self {
            ArmEvent::Fork => "fork",
            ArmEvent::Enter(_) => "enter",
            ArmEvent::Exit => "exit",
            ArmEvent::Join => "join",
            ArmEvent::Leave => "leave",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IntTy;
    use std::collections::HashSet;

    fn check<T: Copy + PartialEq + std::fmt::Debug>(
        table: &[(&str, T)],
        len: usize,
        name: impl Fn(T) -> &'static str,
        parse: impl Fn(&str) -> Option<T>,
    ) {
        assert_eq!(table.len(), len);
        let mut seen = HashSet::new();
        for &(n, v) in table {
            assert!(seen.insert(n), "duplicate mnemonic {n}");
            assert_eq!(name(v), n);
            assert_eq!(parse(n), Some(v));
        }
    }

    #[test]
    fn scalar_ops() {
        check(
            ScalarOp::MNEMONICS,
            36,
            ScalarOp::mnemonic,
            ScalarOp::from_mnemonic,
        );
        let typed = [
            ScalarOp::WrapAs(IntTy {
                bits: 8,
                signed: false,
            }),
            ScalarOp::SatAs(IntTy {
                bits: 8,
                signed: false,
            }),
            ScalarOp::CheckedAs(IntTy {
                bits: 8,
                signed: false,
            }),
        ];
        for op in typed {
            assert!(ScalarOp::MNEMONICS.iter().all(|&(n, _)| n != op.mnemonic()));
        }
        let names: HashSet<_> = typed.map(ScalarOp::mnemonic).into();
        assert_eq!(names.len(), 3);
    }

    #[test]
    fn atomic_ops() {
        check(
            AtomicOp::MNEMONICS,
            11,
            AtomicOp::mnemonic,
            AtomicOp::from_mnemonic,
        );
    }

    #[test]
    fn mem_orders() {
        check(
            MemOrder::MNEMONICS,
            5,
            MemOrder::mnemonic,
            MemOrder::from_mnemonic,
        );
    }

    #[test]
    fn rt_fns() {
        check(RtFn::MNEMONICS, 270, RtFn::mnemonic, RtFn::from_mnemonic);
    }

    #[test]
    fn arm_events() {
        let names: HashSet<_> = [
            ArmEvent::Fork,
            ArmEvent::Enter(0),
            ArmEvent::Exit,
            ArmEvent::Join,
            ArmEvent::Leave,
        ]
        .map(ArmEvent::mnemonic)
        .into();
        assert_eq!(names.len(), 5);
    }
}
