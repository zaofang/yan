//! Debug code generation (M1b): yan-mir to machine code with Cranelift, run in process. Std
//! calls go to yan-rt's C-ABI symbols (R561); scalar intrinsics are lowered inline. Every
//! function uses the tail calling convention so `become` is a `return_call` (3.5).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block as CBlock, FuncRef, GlobalValue, Inst as CInst, InstBuilder, MemFlagsData,
    Signature, SourceLoc, StackSlotData, StackSlotKind, TrapCode, Value, types,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{
    DataDescription, DataId, FuncId as CFuncId, Linkage, Module, default_libcall_names,
};
use cranelift_object::{ObjectBuilder, ObjectModule};

mod debug;
use yan_mir::{Func, Inst, IntTy, Operand, Program, RtFn, ScalarOp, Term, Ty};
use yan_rt::{abi, actor, chan, cluster, dq, map, pq, sched, select, slice};

type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("cranelift: {e}")
}

fn clif(t: Ty) -> Option<types::Type> {
    match t {
        Ty::Int(_) | Ty::Ptr => Some(types::I64),
        Ty::F64 => Some(types::F64),
        Ty::Bool => Some(types::I8),
        Ty::Unit => None,
    }
}

/// Results beyond this many come back through a caller-provided return area: the tail
/// convention has only a few result registers and aarch64 rejects more.
const MAX_REG_RETS: usize = 4;

fn returns_by_area(ret: &[Ty]) -> bool {
    ret.iter().filter(|t| clif(**t).is_some()).count() > MAX_REG_RETS
}

/// A compiled program, ready to run.
pub struct Compiled {
    _lazy: Box<Lazy>,
    entry: extern "C" fn(),
}

impl Compiled {
    /// Run `main`. A panic exits the process with status 101 (8.3).
    pub fn run(&self) {
        (self.entry)();
        actor::end_root();
    }
}

/// When functions become machine code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jit {
    /// All of them before `main` starts, with direct calls.
    Eager,
    /// Each one at its first call (debug `yan run`). Calls go through the debug jump table of
    /// 18.4: one code address per function, first that of a stub that compiles the function,
    /// stores its address in the table and tail-calls it. Hot reload writes the same table.
    Lazy,
}

/// Where generated code finds the program's string literals.
enum Strings {
    /// Blocks already built in this process (JIT): their addresses are constants.
    Addr(Vec<i64>),
    /// Object files: data slots that the entry point fills with literal blocks before `main`
    /// runs, and the bytes of each panic kind.
    Slots {
        lits: Vec<DataId>,
        kinds: HashMap<&'static str, DataId>,
    },
}

/// [`Strings`] as seen from inside one function.
enum Strs<'a> {
    Addr(&'a [i64]),
    Slots {
        lits: Vec<GlobalValue>,
        kinds: HashMap<&'static str, GlobalValue>,
    },
}

/// The module and what it takes to define more functions in it.
struct State<M: Module> {
    module: M,
    program: Program,
    sigs: Vec<Signature>,
    ids: Vec<CFuncId>,
    rt_ids: Vec<CFuncId>,
    /// `yan_rt_panic`, `yan_rt_panic_ops` and `yan_rt_panic_fields`.
    panic: [CFuncId; 3],
    strings: Strings,
    fctx: FunctionBuilderContext,
    ctx: cranelift_codegen::Context,
    /// Each defined function's code size and `(offset, line)` rows, in definition order.
    lines: Vec<(u64, Vec<(u32, u32)>)>,
}

/// Owns the module; its address is baked into the stubs, so it is boxed.
struct Lazy {
    state: RefCell<State<JITModule>>,
    /// The jump table, read by the JIT code at its address. Empty for [`Jit::Eager`].
    table: Box<[Cell<usize>]>,
    done: RefCell<Vec<bool>>,
}

/// How generated code calls the program's functions.
enum Calls {
    Direct(Vec<FuncRef>),
    /// Load the callee's address from the jump table at `table`.
    Table {
        table: i64,
        sigs: Vec<cranelift_codegen::ir::SigRef>,
    },
}

impl<M: Module> State<M> {
    /// Define function `i`, calling the others directly or through the jump table at `table`.
    fn define(&mut self, i: usize, table: Option<i64>) -> R<()> {
        let State {
            module,
            program,
            sigs,
            ids,
            rt_ids,
            panic,
            strings,
            fctx,
            ctx,
            lines,
        } = self;
        let ptr = module.target_config().pointer_type();
        ctx.func.signature = sigs[i].clone();
        let mut b = FunctionBuilder::new(&mut ctx.func, fctx);
        let calls = match table {
            None => Calls::Direct(
                ids.iter()
                    .map(|id| module.declare_func_in_func(*id, b.func))
                    .collect(),
            ),
            Some(table) => Calls::Table {
                table,
                sigs: sigs.iter().map(|s| b.import_signature(s.clone())).collect(),
            },
        };
        let rt = Rt {
            fns: rt_ids
                .iter()
                .map(|id| module.declare_func_in_func(*id, b.func))
                .collect(),
            panic: panic.map(|id| module.declare_func_in_func(id, b.func)),
            ptr,
        };
        let strings = match strings {
            Strings::Addr(a) => Strs::Addr(a),
            Strings::Slots { lits, kinds } => Strs::Slots {
                lits: lits
                    .iter()
                    .map(|d| module.declare_data_in_func(*d, b.func))
                    .collect(),
                kinds: kinds
                    .iter()
                    .map(|(k, d)| (*k, module.declare_data_in_func(*d, b.func)))
                    .collect(),
            },
        };
        Gen {
            b: &mut b,
            f: &program.funcs[i],
            program,
            ret_area: None,
            vars: Vec::new(),
            blocks: Vec::new(),
            calls,
            rt,
            strings,
            kinds: HashMap::new(),
        }
        .func();
        b.finalize(module.target_config());
        module.define_function(ids[i], ctx).map_err(err)?;
        if let Some(code) = ctx.compiled_code() {
            let base = ctx.func.params.base_srcloc();
            let rows = code
                .buffer
                .get_srclocs_sorted()
                .iter()
                .map(|l| (l.start, line_of(l.loc, base)))
                .collect();
            lines.push((u64::from(code.code_info().total_size), rows));
        }
        module.clear_context(ctx);
        Ok(())
    }
}

impl Lazy {
    fn table_addr(&self) -> i64 {
        self.table.as_ptr() as i64
    }

    /// The code of function `i`, compiled now if this is its first call.
    fn resolve(&self, i: usize) -> R<usize> {
        if self.done.borrow()[i] {
            return Ok(self.table[i].get());
        }
        let mut st = self.state.borrow_mut();
        st.define(i, Some(self.table_addr()))?;
        st.module.finalize_definitions().map_err(err)?;
        let code = st.module.get_finalized_function(st.ids[i]) as usize;
        self.table[i].set(code);
        self.done.borrow_mut()[i] = true;
        Ok(code)
    }
}

/// Called by a stub: compile function `i` of `lazy` and return its code address.
#[allow(unsafe_code)]
extern "C" fn yan_jit_resolve(lazy: *const Lazy, i: i64) -> usize {
    // SAFETY: stubs pass the address of the boxed `Lazy` that the running `Compiled` owns.
    let lazy = unsafe { &*lazy };
    match lazy.resolve(i as usize) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("yan: not supported yet: {e}");
            std::process::exit(2)
        }
    }
}

/// Compile `p` for the host, every function before `main` starts.
pub fn compile(p: &Program) -> R<Compiled> {
    compile_with(p, Jit::Eager)
}

/// Panics that the code generator raises itself.
const PANIC_KINDS: [&str; 4] = ["div_zero", "exclusivity", "overflow", "unreachable"];

/// The source line of a machine-code region (0: none).
fn line_of(loc: cranelift_codegen::ir::MaybeRelSourceLoc, base: SourceLoc) -> u32 {
    let loc = if loc.is_rel() {
        loc.relocate(base)
    } else {
        loc.as_abs()
    };
    if loc.is_default() { 0 } else { loc.bits() }
}

fn host_isa(pic: bool) -> R<std::sync::Arc<dyn cranelift_codegen::isa::TargetIsa>> {
    let mut flags = settings::builder();
    flags.set("opt_level", "none").map_err(err)?;
    // Cranelift's tail calls need frame pointers.
    flags.set("preserve_frame_pointers", "true").map_err(err)?;
    flags
        .set("is_pic", if pic { "true" } else { "false" })
        .map_err(err)?;
    cranelift_native::builder()
        .map_err(err)?
        .finish(settings::Flags::new(flags))
        .map_err(err)
}

/// Compile `p` to a relocatable object for the host, with every function defined and no lazy
/// stubs. The object exports C `main`, which builds the string literals, runs the program's
/// `main` and exits through `yan_rt_exit`; link it with `libyan_rt.a`.
pub fn compile_object(p: &Program) -> R<Vec<u8>> {
    object(p, None)
}

/// As [`compile_object`], with DWARF line tables that map the code to lines of `source`.
pub fn compile_object_debug(p: &Program, source: &std::path::Path) -> R<Vec<u8>> {
    object(p, Some(source))
}

fn object(p: &Program, source: Option<&std::path::Path>) -> R<Vec<u8>> {
    let builder =
        ObjectBuilder::new(host_isa(true)?, "yan", default_libcall_names()).map_err(err)?;
    let mut module = ObjectModule::new(builder);
    let ptr = module.target_config().pointer_type();
    let (panic, exit, rt_ids) = declare_rt(&mut module)?;
    let (sigs, ids) = declare_funcs(&mut module, p)?;

    // Each literal is its bytes plus a zeroed slot that `main` fills with the block address.
    let mut lits = Vec::new();
    for (i, s) in p.strings.iter().enumerate() {
        let bytes = module
            .declare_data(&format!("yan.str.{i}.bytes"), Linkage::Local, false, false)
            .map_err(err)?;
        let mut d = DataDescription::new();
        d.define(s.as_bytes().to_vec().into_boxed_slice());
        module.define_data(bytes, &d).map_err(err)?;
        let slot = module
            .declare_data(&format!("yan.str.{i}"), Linkage::Local, true, false)
            .map_err(err)?;
        let mut d = DataDescription::new();
        d.define_zeroinit(8);
        module.define_data(slot, &d).map_err(err)?;
        lits.push((bytes, slot, s.len()));
    }
    let mut kinds = HashMap::new();
    let used = p
        .funcs
        .iter()
        .flat_map(|f| &f.blocks)
        .filter_map(|b| match b.term {
            Term::Panic(k) | Term::PanicFields { kind: k, .. } => Some(k),
            _ => None,
        });
    for (i, k) in PANIC_KINDS.iter().copied().chain(used).enumerate() {
        if kinds.contains_key(k) {
            continue;
        }
        let id = module
            .declare_data(&format!("yan.panic.{i}"), Linkage::Local, false, false)
            .map_err(err)?;
        let mut d = DataDescription::new();
        d.define(k.as_bytes().to_vec().into_boxed_slice());
        module.define_data(id, &d).map_err(err)?;
        kinds.insert(k, id);
    }
    let mut st = State {
        module,
        program: p.clone(),
        sigs,
        ids,
        rt_ids,
        panic,
        strings: Strings::Slots {
            lits: lits.iter().map(|l| l.1).collect(),
            kinds,
        },
        fctx: FunctionBuilderContext::new(),
        ctx: cranelift_codegen::Context::new(),
        lines: Vec::new(),
    };
    for i in 0..p.funcs.len() {
        st.define(i, None)?;
    }

    let State {
        mut module,
        mut fctx,
        mut ctx,
        ids,
        lines,
        ..
    } = st;
    let mut lit_sig = module.make_signature();
    lit_sig
        .params
        .extend([AbiParam::new(ptr), AbiParam::new(types::I64)]);
    lit_sig.returns.push(AbiParam::new(types::I64));
    let lit = module
        .declare_function("yan_rt_static_str", Linkage::Import, &lit_sig)
        .map_err(err)?;
    let mut entry_sig = module.make_signature();
    entry_sig.returns.push(AbiParam::new(types::I32));
    let entry = module
        .declare_function("main", Linkage::Export, &entry_sig)
        .map_err(err)?;
    ctx.func.signature = entry_sig;
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let blk = b.create_block();
        b.switch_to_block(blk);
        b.seal_block(blk);
        let lit = module.declare_func_in_func(lit, b.func);
        for (bytes, slot, len) in &lits {
            let bytes = module.declare_data_in_func(*bytes, b.func);
            let bytes = b.ins().symbol_value(ptr, bytes);
            let len = b.ins().iconst(types::I64, *len as i64);
            let c = b.ins().call(lit, &[bytes, len]);
            let block = b.inst_results(c)[0];
            let slot = module.declare_data_in_func(*slot, b.func);
            let slot = b.ins().symbol_value(ptr, slot);
            b.ins().store(MemFlagsData::trusted(), block, slot, 0);
        }
        let main = module.declare_func_in_func(ids[p.main as usize], b.func);
        b.ins().call(main, &[]);
        let code = b.ins().iconst(types::I64, 0);
        let exit = module.declare_func_in_func(exit, b.func);
        b.ins().call(exit, &[code]);
        b.ins().trap(TrapCode::unwrap_user(1));
        b.finalize(module.target_config());
    }
    module.define_function(entry, &mut ctx).map_err(err)?;
    let mut product = module.finish();
    if let Some(source) = source {
        let funcs: Vec<_> = lines
            .into_iter()
            .zip(&p.funcs)
            .zip(&ids)
            .map(|(((size, rows), f), id)| debug::Func {
                symbol: product.function_symbol(*id),
                name: f.name.clone(),
                size,
                rows,
            })
            .collect();
        debug::add(&mut product.object, source, &funcs)?;
    }
    product.emit().map_err(err)
}

/// Compile `p` for the host.
pub fn compile_with(p: &Program, jit: Jit) -> R<Compiled> {
    let isa = host_isa(false)?;
    let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
    jb.symbol("yan_rt_panic", abi::yan_rt_panic as *const u8);
    jb.symbol("yan_rt_panic_ops", abi::yan_rt_panic_ops as *const u8);
    jb.symbol("yan_rt_panic_fields", abi::yan_rt_panic_fields as *const u8);
    jb.symbol("yan_rt_exit", abi::yan_rt_exit as *const u8);
    jb.symbol("yan_jit_resolve", yan_jit_resolve as *const u8);
    for f in RT {
        let (name, addr, ..) = rt_fn(*f);
        jb.symbol(name, addr);
    }
    let mut module = JITModule::new(jb);
    let ptr = module.target_config().pointer_type();

    let (panic, _, rt_ids) = declare_rt(&mut module)?;
    let mut resolve_sig = module.make_signature();
    resolve_sig
        .params
        .extend([AbiParam::new(ptr), AbiParam::new(types::I64)]);
    resolve_sig.returns.push(AbiParam::new(ptr));
    let resolve = module
        .declare_function("yan_jit_resolve", Linkage::Import, &resolve_sig)
        .map_err(err)?;
    let (sigs, ids) = declare_funcs(&mut module, p)?;
    let ctx = module.make_context();
    let n = p.funcs.len();
    let lazy = Box::new(Lazy {
        state: RefCell::new(State {
            module,
            program: p.clone(),
            sigs,
            ids,
            rt_ids,
            panic,
            strings: Strings::Addr(
                p.strings
                    .iter()
                    .map(|s| abi::static_str_block(s) as i64)
                    .collect(),
            ),
            fctx: FunctionBuilderContext::new(),
            ctx,
            lines: Vec::new(),
        }),
        table: (0..if jit == Jit::Lazy { n } else { 0 })
            .map(|_| Cell::new(0))
            .collect(),
        done: RefCell::new(vec![false; n]),
    });
    let entry = {
        let mut st = lazy.state.borrow_mut();
        let main = match jit {
            Jit::Eager => {
                for i in 0..n {
                    st.define(i, None)?;
                }
                st.ids[p.main as usize]
            }
            Jit::Lazy => {
                let stubs = stubs(&mut st, &lazy, resolve)?;
                st.module.finalize_definitions().map_err(err)?;
                for (slot, id) in lazy.table.iter().zip(&stubs) {
                    slot.set(st.module.get_finalized_function(*id) as usize);
                }
                stubs[p.main as usize]
            }
        };
        entry_point(&mut st, main)?
    };
    Ok(Compiled { _lazy: lazy, entry })
}

/// Declare the runtime's symbols; returns the ids of panic, exit and the [`RT`] functions.
fn declare_rt<M: Module>(module: &mut M) -> R<([CFuncId; 3], CFuncId, Vec<CFuncId>)> {
    let ptr = module.target_config().pointer_type();
    let mut panic_sig = module.make_signature();
    panic_sig
        .params
        .extend([AbiParam::new(ptr), AbiParam::new(ptr)]);
    let panic = module
        .declare_function("yan_rt_panic", Linkage::Import, &panic_sig)
        .map_err(err)?;
    let mut ops_sig = module.make_signature();
    ops_sig
        .params
        .extend([AbiParam::new(ptr), AbiParam::new(ptr)]);
    ops_sig.params.extend([AbiParam::new(types::I64); 4]);
    let panic_ops = module
        .declare_function("yan_rt_panic_ops", Linkage::Import, &ops_sig)
        .map_err(err)?;
    let mut fields_sig = module.make_signature();
    fields_sig.params.extend([
        AbiParam::new(ptr),
        AbiParam::new(ptr),
        AbiParam::new(types::I64),
        AbiParam::new(ptr),
    ]);
    let panic_fields = module
        .declare_function("yan_rt_panic_fields", Linkage::Import, &fields_sig)
        .map_err(err)?;
    let mut exit_sig = module.make_signature();
    exit_sig.params.push(AbiParam::new(types::I64));
    let exit = module
        .declare_function("yan_rt_exit", Linkage::Import, &exit_sig)
        .map_err(err)?;
    let rt_ids = RT
        .iter()
        .map(|f| {
            let (name, _, params, ret) = rt_fn(*f);
            let mut sig = module.make_signature();
            // Rust reads a `u8` argument as a zero-extended register.
            sig.params.extend(params.iter().map(|t| {
                if *t == types::I8 {
                    AbiParam::new(*t).uext()
                } else {
                    AbiParam::new(*t)
                }
            }));
            sig.returns.extend(ret.map(AbiParam::new));
            module
                .declare_function(name, Linkage::Import, &sig)
                .map_err(err)
        })
        .collect::<R<Vec<_>>>()?;
    Ok(([panic, panic_ops, panic_fields], exit, rt_ids))
}

fn declare_funcs<M: Module>(module: &mut M, p: &Program) -> R<(Vec<Signature>, Vec<CFuncId>)> {
    let sigs: Vec<Signature> = p.funcs.iter().map(|f| signature(f, module)).collect();
    let ids = p
        .funcs
        .iter()
        .zip(&sigs)
        .enumerate()
        .map(|(i, (f, s))| {
            module
                .declare_function(&format!("yan.{i}.{}", f.name), Linkage::Local, s)
                .map_err(err)
        })
        .collect::<R<_>>()?;
    Ok((sigs, ids))
}

/// One stub per function, with its signature: resolve the function, then tail-call it with
/// the same arguments.
fn stubs(st: &mut State<JITModule>, lazy: &Lazy, resolve: CFuncId) -> R<Vec<CFuncId>> {
    let me = std::ptr::from_ref(lazy) as i64;
    let mut out = Vec::new();
    for i in 0..st.sigs.len() {
        let State {
            module,
            sigs,
            fctx,
            ctx,
            ..
        } = st;
        let id = module
            .declare_function(&format!("yan.stub.{i}"), Linkage::Local, &sigs[i])
            .map_err(err)?;
        ctx.func.signature = sigs[i].clone();
        let mut b = FunctionBuilder::new(&mut ctx.func, fctx);
        let blk = b.create_block();
        b.append_block_params_for_function_params(blk);
        b.switch_to_block(blk);
        b.seal_block(blk);
        let args = b.block_params(blk).to_vec();
        let resolve = module.declare_func_in_func(resolve, b.func);
        let ptr = module.target_config().pointer_type();
        let me = b.ins().iconst(ptr, me);
        let idx = b.ins().iconst(types::I64, i as i64);
        let c = b.ins().call(resolve, &[me, idx]);
        let code = b.inst_results(c)[0];
        let sig = b.import_signature(sigs[i].clone());
        b.ins().return_call_indirect(sig, code, &args);
        b.finalize(module.target_config());
        module.define_function(id, ctx).map_err(err)?;
        module.clear_context(ctx);
        out.push(id);
    }
    Ok(out)
}

/// Define the C-ABI trampoline the host calls, which calls the tail-convention `main`, and
/// finalize the module.
fn entry_point(st: &mut State<JITModule>, main: CFuncId) -> R<extern "C" fn()> {
    let State {
        module, fctx, ctx, ..
    } = st;
    let entry_sig = module.make_signature();
    let entry = module
        .declare_function("yan.entry", Linkage::Local, &entry_sig)
        .map_err(err)?;
    ctx.func.signature = entry_sig;
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, fctx);
        let blk = b.create_block();
        b.switch_to_block(blk);
        b.seal_block(blk);
        let main = module.declare_func_in_func(main, b.func);
        b.ins().call(main, &[]);
        b.ins().return_(&[]);
        b.finalize(module.target_config());
    }
    module.define_function(entry, ctx).map_err(err)?;
    module.clear_context(ctx);
    module.finalize_definitions().map_err(err)?;
    let code = module.get_finalized_function(entry);
    #[allow(unsafe_code)]
    // SAFETY: `yan.entry` was defined above with the host's default C signature, no arguments
    // and no result.
    let entry = unsafe { std::mem::transmute::<*const u8, extern "C" fn()>(code) };
    Ok(entry)
}

fn signature(f: &Func, m: &impl Module) -> Signature {
    let mut s = m.make_signature();
    s.call_conv = CallConv::Tail;
    if returns_by_area(&f.ret) {
        s.params
            .push(AbiParam::new(m.target_config().pointer_type()));
    } else {
        s.returns
            .extend(f.ret.iter().filter_map(|t| clif(*t)).map(AbiParam::new));
    }
    for t in &f.locals[..f.params as usize] {
        if let Some(t) = clif(*t) {
            s.params.push(AbiParam::new(t));
        }
    }
    s
}

const RT: &[RtFn] = &[
    RtFn::Print,
    RtFn::FailMain,
    RtFn::ReadLine,
    RtFn::ReadAll,
    RtFn::FsRead,
    RtFn::FsReason,
    RtFn::Concat,
    RtFn::ShowI64,
    RtFn::ShowU64,
    RtFn::ShowF64,
    RtFn::DivI64,
    RtFn::DivU64,
    RtFn::ShowBool,
    RtFn::Repr,
    RtFn::StrEq,
    RtFn::StrRepeat,
    RtFn::CharCount,
    RtFn::ByteLen,
    RtFn::IntOf,
    RtFn::TruncF64,
    RtFn::RoundF64,
    RtFn::IntToF64,
    RtFn::AbsI64,
    RtFn::AbsF64,
    RtFn::Floor,
    RtFn::Ceil,
    RtFn::TruncF,
    RtFn::NextUp,
    RtFn::NextDown,
    RtFn::Fmod,
    RtFn::Pown,
    RtFn::Pow,
    RtFn::Ldexp,
    RtFn::FrexpM,
    RtFn::FrexpE,
    RtFn::StrSplit,
    RtFn::StrSplitLines,
    RtFn::Math1,
    RtFn::Math2,
    RtFn::Copysign,
    RtFn::IsNan,
    RtFn::IsInf,
    RtFn::IsFinite,
    RtFn::ListNew,
    RtFn::ListPush,
    RtFn::ListLen,
    RtFn::ListAt,
    RtFn::ListCopy,
    RtFn::HeapUp,
    RtFn::HeapFix,
    RtFn::HeapPop,
    RtFn::HeapPushPop,
    RtFn::HeapSort,
    RtFn::KeyEq,
    RtFn::ListReverse,
    RtFn::StrSlice,
    RtFn::ListSlice,
    RtFn::Select,
    RtFn::DqLimit,
    RtFn::DqMaxlen,
    RtFn::DqPush,
    RtFn::DqPop,
    RtFn::DqExtend,
    RtFn::DqRotate,
    RtFn::DqTake,
    RtFn::DqContains,
    RtFn::DqSuffix,
    RtFn::MapNew,
    RtFn::MapFind,
    RtFn::MapIndex,
    RtFn::MapPush,
    RtFn::MapRemove,
    RtFn::MapLen,
    RtFn::MapAt,
    RtFn::MapCopy,
    RtFn::MapFree,
    RtFn::JsonParse,
    RtFn::JsonDump,
    RtFn::JsonErrorShow,
    RtFn::CsvParse,
    RtFn::CsvDump,
    RtFn::CsvErrorShow,
    RtFn::WireRoundtrip,
    RtFn::Alloc,
    RtFn::StrFree,
    RtFn::ListFree,
    RtFn::EnvFree,
    RtFn::ChanNew,
    RtFn::ChanSend,
    RtFn::ChanRecv,
    RtFn::ChanDrop,
    RtFn::ParBegin,
    RtFn::ParNext,
    RtFn::ParTaskEnd,
    RtFn::ParExitKind,
    RtFn::ParCancel,
    RtFn::ParEnd,
    RtFn::CostBegin,
    RtFn::CostEnd,
    RtFn::CostCount,
    RtFn::ActorSpawn,
    RtFn::ActorTake,
    RtFn::ActorPut,
    RtFn::ActorLend,
    RtFn::ActorRetake,
    RtFn::ActorGive,
    RtFn::ActorSend,
    RtFn::ActorArg,
    RtFn::ActorRan,
    RtFn::ActorIdle,
    RtFn::ActorCrash,
    RtFn::ActorReason,
    RtFn::ActorReasonText,
    RtFn::ActorRestartNext,
    RtFn::ActorRestarted,
    RtFn::SupBegin,
    RtFn::SupEnd,
    RtFn::SimNew,
    RtFn::SimBegin,
    RtFn::ClusterNodes,
    RtFn::ClusterSelfNode,
    RtFn::ClusterPlace,
    RtFn::ClusterLeave,
    RtFn::ClusterReach,
    RtFn::ClusterFaultSend,
    RtFn::ClusterSendMismatch,
    RtFn::ClusterAdmit,
    RtFn::ClusterArrive,
    RtFn::SupEndTo,
];

/// Symbol, address, parameter types and result type.
fn rt_fn(
    f: RtFn,
) -> (
    &'static str,
    *const u8,
    &'static [types::Type],
    Option<types::Type>,
) {
    use types::{F64, I8, I64};
    match f {
        RtFn::Print => (
            "yan_rt_print_str",
            abi::yan_rt_print_str as *const u8,
            &[I64],
            None,
        ),
        RtFn::FailMain => (
            "yan_rt_fail_main",
            abi::yan_rt_fail_main as *const u8,
            &[I64],
            None,
        ),
        RtFn::ReadLine => (
            "yan_rt_read_line",
            abi::yan_rt_read_line as *const u8,
            &[],
            Some(I64),
        ),
        RtFn::ReadAll => (
            "yan_rt_read_all",
            abi::yan_rt_read_all as *const u8,
            &[],
            Some(I64),
        ),
        RtFn::FsRead => (
            "yan_rt_fs_read",
            abi::yan_rt_fs_read as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::FsReason => (
            "yan_rt_fs_reason",
            abi::yan_rt_fs_reason as *const u8,
            &[],
            Some(I64),
        ),
        RtFn::Concat => (
            "yan_rt_concat",
            abi::yan_rt_concat as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::ShowI64 => (
            "yan_rt_show_i64",
            abi::yan_rt_show_i64 as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ShowU64 => (
            "yan_rt_show_u64",
            abi::yan_rt_show_u64 as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ShowF64 => (
            "yan_rt_show_f64",
            abi::yan_rt_show_f64 as *const u8,
            &[F64],
            Some(I64),
        ),
        RtFn::DivI64 => (
            "yan_rt_div_i64",
            abi::yan_rt_div_i64 as *const u8,
            &[I64, I64],
            Some(F64),
        ),
        RtFn::DivU64 => (
            "yan_rt_div_u64",
            abi::yan_rt_div_u64 as *const u8,
            &[I64, I64],
            Some(F64),
        ),
        RtFn::ShowBool => (
            "yan_rt_show_bool",
            abi::yan_rt_show_bool as *const u8,
            &[I8],
            Some(I64),
        ),
        RtFn::Repr => (
            "yan_rt_repr",
            abi::yan_rt_repr as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::StrRepeat => (
            "yan_rt_str_repeat",
            abi::yan_rt_str_repeat as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::StrEq => (
            "yan_rt_str_eq",
            abi::yan_rt_str_eq as *const u8,
            &[I64, I64],
            Some(I8),
        ),
        RtFn::CharCount => (
            "yan_rt_char_count",
            abi::yan_rt_char_count as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ByteLen => (
            "yan_rt_byte_len",
            abi::yan_rt_byte_len as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::IntOf => (
            "yan_rt_int_of",
            abi::yan_rt_int_of as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::TruncF64 => (
            "yan_rt_trunc_f64",
            abi::yan_rt_trunc_f64 as *const u8,
            &[F64],
            Some(I64),
        ),
        RtFn::RoundF64 => (
            "yan_rt_round_f64",
            abi::yan_rt_round_f64 as *const u8,
            &[F64],
            Some(I64),
        ),
        RtFn::IntToF64 => (
            "yan_rt_int_to_f64",
            abi::yan_rt_int_to_f64 as *const u8,
            &[I64],
            Some(F64),
        ),
        RtFn::AbsI64 => (
            "yan_rt_abs_i64",
            abi::yan_rt_abs_i64 as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::AbsF64 => (
            "yan_rt_abs_f64",
            abi::yan_rt_abs_f64 as *const u8,
            &[F64],
            Some(F64),
        ),
        RtFn::Floor => (
            "yan_rt_floor",
            abi::yan_rt_floor as *const u8,
            &[F64],
            Some(F64),
        ),
        RtFn::Ceil => (
            "yan_rt_ceil",
            abi::yan_rt_ceil as *const u8,
            &[F64],
            Some(F64),
        ),
        RtFn::TruncF => (
            "yan_rt_trunc",
            abi::yan_rt_trunc as *const u8,
            &[F64],
            Some(F64),
        ),
        RtFn::NextUp => (
            "yan_rt_next_up",
            abi::yan_rt_next_up as *const u8,
            &[F64],
            Some(F64),
        ),
        RtFn::NextDown => (
            "yan_rt_next_down",
            abi::yan_rt_next_down as *const u8,
            &[F64],
            Some(F64),
        ),
        RtFn::Fmod => (
            "yan_rt_fmod",
            abi::yan_rt_fmod as *const u8,
            &[F64, F64],
            Some(F64),
        ),
        RtFn::Pow => (
            "yan_rt_pow",
            abi::yan_rt_pow as *const u8,
            &[F64, F64],
            Some(F64),
        ),
        RtFn::Ldexp => (
            "yan_rt_ldexp",
            abi::yan_rt_ldexp as *const u8,
            &[F64, I64],
            Some(F64),
        ),
        RtFn::FrexpM => (
            "yan_rt_frexp_m",
            abi::yan_rt_frexp_m as *const u8,
            &[F64],
            Some(F64),
        ),
        RtFn::FrexpE => (
            "yan_rt_frexp_e",
            abi::yan_rt_frexp_e as *const u8,
            &[F64],
            Some(I64),
        ),
        RtFn::StrSplit => (
            "yan_rt_str_split",
            abi::yan_rt_str_split as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::StrSplitLines => (
            "yan_rt_str_splitlines",
            abi::yan_rt_str_splitlines as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::Math1 => (
            "yan_rt_math1",
            abi::yan_rt_math1 as *const u8,
            &[I64, F64],
            Some(F64),
        ),
        RtFn::Math2 => (
            "yan_rt_math2",
            abi::yan_rt_math2 as *const u8,
            &[I64, F64, F64],
            Some(F64),
        ),
        RtFn::Pown => (
            "yan_rt_pown",
            abi::yan_rt_pown as *const u8,
            &[F64, I64],
            Some(F64),
        ),
        RtFn::Copysign => (
            "yan_rt_copysign",
            abi::yan_rt_copysign as *const u8,
            &[F64, F64],
            Some(F64),
        ),
        RtFn::IsNan => (
            "yan_rt_is_nan",
            abi::yan_rt_is_nan as *const u8,
            &[F64],
            Some(I8),
        ),
        RtFn::IsInf => (
            "yan_rt_is_inf",
            abi::yan_rt_is_inf as *const u8,
            &[F64],
            Some(I8),
        ),
        RtFn::IsFinite => (
            "yan_rt_is_finite",
            abi::yan_rt_is_finite as *const u8,
            &[F64],
            Some(I8),
        ),
        RtFn::ListNew => (
            "yan_rt_list_new",
            abi::yan_rt_list_new as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListPush => (
            "yan_rt_list_push",
            abi::yan_rt_list_push as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListLen => (
            "yan_rt_list_len",
            abi::yan_rt_list_len as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListAt => (
            "yan_rt_list_at",
            abi::yan_rt_list_at as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::HeapUp => (
            "yan_rt_heap_up",
            pq::yan_rt_heap_up as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::KeyEq => (
            "yan_rt_key_eq",
            map::yan_rt_key_eq as *const u8,
            &[I64, I64, I64],
            Some(I8),
        ),
        RtFn::HeapFix => (
            "yan_rt_heap_fix",
            pq::yan_rt_heap_fix as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::HeapPop => (
            "yan_rt_heap_pop",
            pq::yan_rt_heap_pop as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::HeapPushPop => (
            "yan_rt_heap_pushpop",
            pq::yan_rt_heap_pushpop as *const u8,
            &[I64, I64, I64, I64],
            None,
        ),
        RtFn::HeapSort => (
            "yan_rt_heap_sort",
            pq::yan_rt_heap_sort as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::StrSlice => (
            "yan_rt_str_slice",
            slice::yan_rt_str_slice as *const u8,
            &[I64, I64, I64, I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::ListSlice => (
            "yan_rt_list_slice",
            slice::yan_rt_list_slice as *const u8,
            &[I64, I64, I64, I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::Select => (
            "yan_rt_select",
            select::yan_rt_select as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::DqLimit => (
            "yan_rt_dq_limit",
            dq::yan_rt_dq_limit as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::DqMaxlen => (
            "yan_rt_dq_maxlen",
            dq::yan_rt_dq_maxlen as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::DqPush => (
            "yan_rt_dq_push",
            dq::yan_rt_dq_push as *const u8,
            &[I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::DqPop => (
            "yan_rt_dq_pop",
            dq::yan_rt_dq_pop as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::DqExtend => (
            "yan_rt_dq_extend",
            dq::yan_rt_dq_extend as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::DqRotate => (
            "yan_rt_dq_rotate",
            dq::yan_rt_dq_rotate as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::DqTake => (
            "yan_rt_dq_take",
            dq::yan_rt_dq_take as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::DqContains => (
            "yan_rt_dq_contains",
            dq::yan_rt_dq_contains as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::DqSuffix => (
            "yan_rt_dq_suffix",
            dq::yan_rt_dq_suffix as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::MapNew => (
            "yan_rt_map_new",
            map::yan_rt_map_new as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::MapFind => (
            "yan_rt_map_find",
            map::yan_rt_map_find as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::MapIndex => (
            "yan_rt_map_index",
            map::yan_rt_map_index as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::MapPush => (
            "yan_rt_map_push",
            map::yan_rt_map_push as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::MapRemove => (
            "yan_rt_map_remove",
            map::yan_rt_map_remove as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::MapLen => (
            "yan_rt_map_len",
            map::yan_rt_map_len as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::MapAt => (
            "yan_rt_map_at",
            map::yan_rt_map_at as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::MapCopy => (
            "yan_rt_map_copy",
            map::yan_rt_map_copy as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::MapFree => (
            "yan_rt_map_free",
            map::yan_rt_map_free as *const u8,
            &[I64],
            None,
        ),
        RtFn::JsonParse => (
            "yan_rt_json_parse",
            abi::yan_rt_json_parse as *const u8,
            &[I64, I64, I64],
            None,
        ),
        RtFn::JsonDump => (
            "yan_rt_json_dump",
            abi::yan_rt_json_dump as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::CsvParse => (
            "yan_rt_csv_parse",
            abi::yan_rt_csv_parse as *const u8,
            &[I64, I64, I64, I64],
            None,
        ),
        RtFn::CsvDump => (
            "yan_rt_csv_dump",
            abi::yan_rt_csv_dump as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::WireRoundtrip => (
            "yan_rt_wire_roundtrip",
            abi::yan_rt_wire_roundtrip as *const u8,
            &[I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::CsvErrorShow => (
            "yan_rt_csv_error_show",
            abi::yan_rt_csv_error_show as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::JsonErrorShow => (
            "yan_rt_json_error_show",
            abi::yan_rt_json_error_show as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::Alloc => (
            "yan_rt_alloc",
            abi::yan_rt_alloc as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::StrFree => (
            "yan_rt_str_free",
            abi::yan_rt_str_free as *const u8,
            &[I64],
            None,
        ),
        RtFn::ListFree => (
            "yan_rt_list_free",
            abi::yan_rt_list_free as *const u8,
            &[I64],
            None,
        ),
        RtFn::EnvFree => (
            "yan_rt_free_env",
            abi::yan_rt_free_env as *const u8,
            &[I64],
            None,
        ),
        RtFn::ListReverse => (
            "yan_rt_list_reverse",
            abi::yan_rt_list_reverse as *const u8,
            &[I64],
            None,
        ),
        RtFn::ListCopy => (
            "yan_rt_list_copy",
            abi::yan_rt_list_copy as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ChanNew => (
            "yan_rt_chan_new",
            chan::yan_rt_chan_new as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::ChanSend => (
            "yan_rt_chan_send",
            chan::yan_rt_chan_send as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::ChanRecv => (
            "yan_rt_chan_recv",
            chan::yan_rt_chan_recv as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::ChanDrop => (
            "yan_rt_chan_drop",
            chan::yan_rt_chan_drop as *const u8,
            &[I64],
            None,
        ),
        RtFn::ParBegin => (
            "yan_rt_par_begin",
            sched::yan_rt_par_begin as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::ParNext => (
            "yan_rt_par_next",
            sched::yan_rt_par_next as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ParTaskEnd => (
            "yan_rt_par_task_end",
            sched::yan_rt_par_task_end as *const u8,
            &[I64],
            None,
        ),
        RtFn::ParExitKind => (
            "yan_rt_par_exit_kind",
            sched::yan_rt_par_exit_kind as *const u8,
            &[I64],
            None,
        ),
        RtFn::ParCancel => (
            "yan_rt_par_cancel",
            sched::yan_rt_par_cancel as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ParEnd => (
            "yan_rt_par_end",
            sched::yan_rt_par_end as *const u8,
            &[I64],
            None,
        ),
        RtFn::ActorSpawn => (
            "yan_rt_actor_spawn",
            actor::yan_rt_actor_spawn as *const u8,
            &[I64, I64, I64, I64, I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::ActorTake => (
            "yan_rt_actor_take",
            actor::yan_rt_actor_take as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::ActorPut => (
            "yan_rt_actor_put",
            actor::yan_rt_actor_put as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::ActorLend => (
            "yan_rt_actor_lend",
            actor::yan_rt_actor_lend as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::ActorRetake => (
            "yan_rt_actor_retake",
            actor::yan_rt_actor_retake as *const u8,
            &[I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::ActorGive => (
            "yan_rt_actor_give",
            actor::yan_rt_actor_give as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::ActorSend => (
            "yan_rt_actor_send",
            actor::yan_rt_actor_send as *const u8,
            &[I64, I64, I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::ActorArg => (
            "yan_rt_actor_arg",
            actor::yan_rt_actor_arg as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ActorRan => (
            "yan_rt_actor_ran",
            actor::yan_rt_actor_ran as *const u8,
            &[],
            None,
        ),
        RtFn::ActorIdle => (
            "yan_rt_actor_idle",
            actor::yan_rt_actor_idle as *const u8,
            &[],
            Some(I64),
        ),
        RtFn::ActorCrash => (
            "yan_rt_actor_crash",
            actor::yan_rt_actor_crash as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::ActorReason => (
            "yan_rt_actor_reason",
            actor::yan_rt_actor_reason as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ActorReasonText => (
            "yan_rt_actor_reason_text",
            actor::yan_rt_actor_reason_text as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ActorRestartNext => (
            "yan_rt_actor_restart_next",
            actor::yan_rt_actor_restart_next as *const u8,
            &[],
            Some(I64),
        ),
        RtFn::ActorRestarted => (
            "yan_rt_actor_restarted",
            actor::yan_rt_actor_restarted as *const u8,
            &[I64, I64],
            None,
        ),
        RtFn::SupBegin => (
            "yan_rt_sup_begin",
            actor::yan_rt_sup_begin as *const u8,
            &[I64, I64, I64, I64],
            Some(I64),
        ),
        RtFn::SupEnd => (
            "yan_rt_sup_end",
            actor::yan_rt_sup_end as *const u8,
            &[],
            None,
        ),
        RtFn::SimNew => (
            "yan_rt_sim_new",
            cluster::yan_rt_sim_new as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::SimBegin => (
            "yan_rt_sim_begin",
            cluster::yan_rt_sim_begin as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::ClusterNodes => (
            "yan_rt_cluster_nodes",
            cluster::yan_rt_cluster_nodes as *const u8,
            &[],
            Some(I64),
        ),
        RtFn::ClusterSelfNode => (
            "yan_rt_cluster_self_node",
            cluster::yan_rt_cluster_self_node as *const u8,
            &[],
            Some(I64),
        ),
        RtFn::ClusterPlace => (
            "yan_rt_cluster_place",
            cluster::yan_rt_cluster_place as *const u8,
            &[I64],
            None,
        ),
        RtFn::ClusterLeave => (
            "yan_rt_cluster_leave",
            cluster::yan_rt_cluster_leave as *const u8,
            &[],
            None,
        ),
        RtFn::ClusterReach => (
            "yan_rt_cluster_reach",
            cluster::yan_rt_cluster_reach as *const u8,
            &[I64],
            None,
        ),
        RtFn::ClusterFaultSend => (
            "yan_rt_cluster_fault_send",
            cluster::yan_rt_cluster_fault_send as *const u8,
            &[I64],
            None,
        ),
        RtFn::ClusterSendMismatch => (
            "yan_rt_cluster_send_mismatch",
            cluster::yan_rt_cluster_send_mismatch as *const u8,
            &[I64],
            None,
        ),
        RtFn::ClusterAdmit => (
            "yan_rt_cluster_admit",
            cluster::yan_rt_cluster_admit as *const u8,
            &[I64],
            None,
        ),
        RtFn::ClusterArrive => (
            "yan_rt_cluster_arrive",
            cluster::yan_rt_cluster_arrive as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::SupEndTo => (
            "yan_rt_sup_end_to",
            actor::yan_rt_sup_end_to as *const u8,
            &[I64],
            None,
        ),
        RtFn::CostBegin => (
            "yan_rt_cost_begin",
            abi::yan_rt_cost_begin as *const u8,
            &[I64, I64, I64],
            Some(I64),
        ),
        RtFn::CostEnd => (
            "yan_rt_cost_end",
            abi::yan_rt_cost_end as *const u8,
            &[I64],
            None,
        ),
        RtFn::CostCount => (
            "yan_rt_cost_count",
            abi::yan_rt_cost_count as *const u8,
            &[I64, I64],
            None,
        ),
    }
}

struct Rt {
    /// In the order of [`RT`].
    fns: Vec<FuncRef>,
    panic: [FuncRef; 3],
    ptr: types::Type,
}

struct Gen<'a, 'b> {
    b: &'a mut FunctionBuilder<'b>,
    f: &'a Func,
    program: &'a Program,
    /// The return area pointer, when this function's results do not fit in registers.
    ret_area: Option<Value>,
    vars: Vec<Option<Variable>>,
    blocks: Vec<CBlock>,
    calls: Calls,
    rt: Rt,
    strings: Strs<'a>,
    /// One panic block per kind.
    kinds: HashMap<&'static str, CBlock>,
}

impl Gen<'_, '_> {
    fn func(mut self) {
        self.blocks = self
            .f
            .blocks
            .iter()
            .map(|_| self.b.create_block())
            .collect();
        let entry = self.blocks[0];
        self.b.append_block_params_for_function_params(entry);
        self.vars = self
            .f
            .locals
            .iter()
            .map(|t| clif(*t).map(|t| self.b.declare_var(t)))
            .collect();
        self.b.switch_to_block(entry);
        let params = self.b.block_params(entry).to_vec();
        let mut it = params.into_iter();
        if returns_by_area(&self.f.ret) {
            self.ret_area = it.next();
        }
        for l in 0..self.f.params as usize {
            if let Some(v) = self.vars[l] {
                let p = it.next().expect("one block param per non-Unit parameter");
                self.b.def_var(v, p);
            }
        }
        for (i, blk) in self.f.blocks.iter().enumerate() {
            if i > 0 {
                self.b.switch_to_block(self.blocks[i]);
            }
            for (inst, line) in blk.insts.iter().zip(&blk.lines) {
                self.b.set_srcloc(SourceLoc::new(*line));
                self.inst(inst);
            }
            self.term(&blk.term);
            self.b.set_srcloc(SourceLoc::default());
        }
        let kinds: Vec<(&'static str, CBlock)> = self.kinds.drain().collect();
        for (kind, blk) in kinds {
            self.b.switch_to_block(blk);
            self.call_panic(kind);
        }
        self.b.seal_all_blocks();
    }

    fn call_panic(&mut self, kind: &'static str) {
        let (ptr, len) = self.kind_str(kind);
        self.b.ins().call(self.rt.panic[0], &[ptr, len]);
        self.b.ins().trap(TrapCode::unwrap_user(1));
    }

    fn call_panic_fields(&mut self, kind: &'static str, fields: &[(Operand, Operand)]) {
        let (ptr, len) = self.kind_str(kind);
        let n = self.b.ins().iconst(types::I64, fields.len() as i64);
        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (fields.len() * 24).max(8) as u32,
            3,
        ));
        for (i, (name, val)) in fields.iter().enumerate() {
            let tag = match val {
                Operand::Local(l) => match self.f.locals[*l as usize] {
                    Ty::Int(t) => i64::from(!t.signed),
                    Ty::F64 => 2,
                    _ => 3,
                },
                Operand::F64(_) => 2,
                Operand::Bool(_) => 3,
                Operand::Str(_) => 4,
                _ => 0,
            };
            let name = self.val(name).expect("a name is a Str");
            let v = self.val(val).expect("a scalar field");
            let v = match self.b.func.dfg.value_type(v) {
                types::F64 => self.b.ins().bitcast(types::I64, MemFlagsData::trusted(), v),
                types::I64 => v,
                _ => self.b.ins().uextend(types::I64, v),
            };
            let tag = self.b.ins().iconst(types::I64, tag);
            for (j, w) in [name, tag, v].into_iter().enumerate() {
                let at = self
                    .b
                    .ins()
                    .stack_addr(self.rt.ptr, slot, (i * 24 + j * 8) as i32);
                self.b.ins().store(MemFlagsData::trusted(), w, at, 0);
            }
        }
        let rec = self.b.ins().stack_addr(self.rt.ptr, slot, 0);
        self.b.ins().call(self.rt.panic[2], &[ptr, len, n, rec]);
        self.b.ins().trap(TrapCode::unwrap_user(1));
    }

    /// The kind's bytes as pointer and length.
    fn kind_str(&mut self, kind: &'static str) -> (Value, Value) {
        let ptr = match &self.strings {
            Strs::Addr(_) => self.b.ins().iconst(self.rt.ptr, kind.as_ptr() as i64),
            Strs::Slots { kinds, .. } => {
                let gv = kinds[kind];
                self.b.ins().symbol_value(self.rt.ptr, gv)
            }
        };
        let len = self.b.ins().iconst(self.rt.ptr, kind.len() as i64);
        (ptr, len)
    }

    /// Like [`Self::guard`], but the panic carries the integer type and operands (R796).
    fn guard_ops(&mut self, ok: Value, kind: &'static str, t: IntTy, x: Value, y: Value) {
        let bad = self.b.create_block();
        let next = self.b.create_block();
        self.b.ins().brif(ok, next, &[], bad, &[]);
        self.b.switch_to_block(bad);
        let (ptr, len) = self.kind_str(kind);
        let bits = self.b.ins().iconst(types::I64, i64::from(t.bits));
        let signed = self.b.ins().iconst(types::I64, i64::from(t.signed));
        self.b
            .ins()
            .call(self.rt.panic[1], &[ptr, len, bits, signed, x, y]);
        self.b.ins().trap(TrapCode::unwrap_user(1));
        self.b.switch_to_block(next);
    }

    fn panic_block(&mut self, kind: &'static str) -> CBlock {
        let b = &mut *self.b;
        *self.kinds.entry(kind).or_insert_with(|| b.create_block())
    }

    /// Continue only when `ok` is true, else panic with `kind`.
    fn guard(&mut self, ok: Value, kind: &'static str) {
        let bad = self.panic_block(kind);
        let next = self.b.create_block();
        self.b.ins().brif(ok, next, &[], bad, &[]);
        self.b.switch_to_block(next);
    }

    fn val(&mut self, o: &Operand) -> Option<Value> {
        Some(match o {
            Operand::Local(l) => {
                let v = self.vars[*l as usize]?;
                self.b.use_var(v)
            }
            Operand::Int(i) => self.b.ins().iconst(types::I64, *i as i64),
            Operand::F64(x) => self.b.ins().f64const(*x),
            Operand::Bool(x) => self.b.ins().iconst(types::I8, i64::from(*x)),
            Operand::Unit => return None,
            Operand::Str(s) => match &self.strings {
                Strs::Addr(a) => {
                    let addr = a[*s as usize];
                    self.b.ins().iconst(types::I64, addr)
                }
                Strs::Slots { lits, .. } => {
                    let gv = lits[*s as usize];
                    let slot = self.b.ins().symbol_value(types::I64, gv);
                    self.b
                        .ins()
                        .load(types::I64, MemFlagsData::trusted(), slot, 0)
                }
            },
            Operand::Func(f) => match &self.calls {
                Calls::Direct(fs) => self.b.ins().func_addr(types::I64, fs[*f as usize]),
                Calls::Table { table, .. } => self.slot(*table, *f),
            },
        })
    }

    /// Function `f`'s current code address in the jump table at `table`.
    fn slot(&mut self, table: i64, f: u32) -> Value {
        let base = self.b.ins().iconst(types::I64, table);
        self.b
            .ins()
            .load(types::I64, MemFlagsData::trusted(), base, f as i32 * 8)
    }

    fn vals(&mut self, os: &[Operand]) -> Vec<Value> {
        os.iter().filter_map(|o| self.val(o)).collect()
    }

    fn set(&mut self, dst: Option<u32>, v: Option<Value>) {
        if let (Some(d), Some(v)) = (dst, v)
            && let Some(var) = self.vars[d as usize]
        {
            self.b.def_var(var, v);
        }
    }

    fn inst(&mut self, i: &Inst) {
        match i {
            Inst::Copy { dst, src } => {
                let v = self.val(src);
                self.set(Some(*dst), v);
            }
            Inst::Call { dst, func, args } => {
                let mut args = self.vals(args);
                let ret = &self.program.funcs[*func as usize].ret;
                let area = self.alloc_area(ret);
                args.splice(0..0, area.map(|(a, _)| a));
                let c = match &self.calls {
                    Calls::Direct(fs) => self.b.ins().call(fs[*func as usize], &args),
                    Calls::Table { table, sigs } => {
                        let sig = sigs[*func as usize];
                        let code = self.slot(*table, *func);
                        self.b.ins().call_indirect(sig, code, &args)
                    }
                };
                let rs = self.results(c, area, ret);
                // Unit slots have neither a variable nor a result.
                let ds = dst.iter().filter(|d| self.vars[**d as usize].is_some());
                for (d, r) in ds.copied().collect::<Vec<_>>().into_iter().zip(rs) {
                    self.set(Some(d), Some(r));
                }
            }
            Inst::CallIndirect {
                dst,
                code,
                args,
                params,
                ret,
            } => {
                let mut sig = Signature::new(CallConv::Tail);
                let area = self.alloc_area(ret);
                if area.is_some() {
                    sig.params.push(AbiParam::new(self.rt.ptr));
                } else {
                    sig.returns
                        .extend(ret.iter().filter_map(|t| clif(*t)).map(AbiParam::new));
                }
                sig.params
                    .extend(params.iter().filter_map(|t| clif(*t)).map(AbiParam::new));
                let sig = self.b.import_signature(sig);
                let code = self.val(code).expect("a code address");
                let mut args = self.vals(args);
                args.splice(0..0, area.map(|(a, _)| a));
                let c = self.b.ins().call_indirect(sig, code, &args);
                let rs = self.results(c, area, ret);
                let ds = dst.iter().filter(|d| self.vars[**d as usize].is_some());
                for (d, r) in ds.copied().collect::<Vec<_>>().into_iter().zip(rs) {
                    self.set(Some(d), Some(r));
                }
            }
            Inst::Rt { dst, f, args } => {
                let args = self.vals(args);
                let at = RT.iter().position(|x| x == f).expect("every RtFn is in RT");
                let c = self.b.ins().call(self.rt.fns[at], &args);
                let r = self.b.inst_results(c).first().copied();
                self.set(*dst, r);
            }
            Inst::Load { dst, ptr, word } => {
                if let Some(t) = clif(self.f.locals[*dst as usize]) {
                    let p = self.val(ptr).expect("a pointer");
                    let v = self
                        .b
                        .ins()
                        .load(t, MemFlagsData::trusted(), p, *word as i32 * 8);
                    self.set(Some(*dst), Some(v));
                }
            }
            Inst::Store { ptr, word, src } => {
                if let Some(v) = self.val(src) {
                    let p = self.val(ptr).expect("a pointer");
                    self.b
                        .ins()
                        .store(MemFlagsData::trusted(), v, p, *word as i32 * 8);
                }
            }
            Inst::Scalar {
                dst, op, ty, args, ..
            } => {
                let args = self.vals(args);
                let v = self.scalar(*op, *ty, &args);
                self.set(*dst, v);
            }
        }
    }

    fn term(&mut self, t: &Term) {
        match t {
            Term::Jump(b) => {
                self.b.ins().jump(self.blocks[*b as usize], &[]);
            }
            Term::Branch { cond, then, els } => {
                let c = self.val(cond).expect("a bool");
                let (t, e) = (self.blocks[*then as usize], self.blocks[*els as usize]);
                self.b.ins().brif(c, t, &[], e, &[]);
            }
            Term::Switch { on, targets } => {
                let v = self.val(&Operand::Local(*on)).expect("an int");
                // A jump table costs about as much as four compares, so a switch with at most
                // that many cases besides its most common target branches instead.
                let common = *targets
                    .iter()
                    .max_by_key(|t| targets.iter().filter(|u| u == t).count())
                    .expect("a switch has targets");
                if targets.iter().filter(|t| **t != common).count() <= 4 {
                    for (i, t) in targets.iter().enumerate() {
                        if *t == common {
                            continue;
                        }
                        let eq = self.b.ins().icmp_imm_s(IntCC::Equal, v, i as i64);
                        let next = self.b.create_block();
                        self.b
                            .ins()
                            .brif(eq, self.blocks[*t as usize], &[], next, &[]);
                        self.b.switch_to_block(next);
                    }
                    self.b.ins().jump(self.blocks[common as usize], &[]);
                    return;
                }
                let mut sw = Switch::new();
                for (i, t) in targets.iter().enumerate() {
                    sw.set_entry(i as u128, self.blocks[*t as usize]);
                }
                let otherwise = self.panic_block("unreachable");
                sw.emit(self.b, v, otherwise);
            }
            Term::Return(os) => {
                let v = self.vals(os);
                if let Some(area) = self.ret_area {
                    for (i, v) in v.into_iter().enumerate() {
                        self.b
                            .ins()
                            .store(MemFlagsData::new(), v, area, 8 * i as i32);
                    }
                    self.b.ins().return_(&[]);
                } else {
                    self.b.ins().return_(&v);
                }
            }
            Term::TailCall { func, args } => {
                let mut args = self.vals(args);
                // The callee returns what this function returns, into the same area.
                args.splice(0..0, self.ret_area);
                match &self.calls {
                    Calls::Direct(fs) => {
                        self.b.ins().return_call(fs[*func as usize], &args);
                    }
                    Calls::Table { table, sigs } => {
                        let sig = sigs[*func as usize];
                        let code = self.slot(*table, *func);
                        self.b.ins().return_call_indirect(sig, code, &args);
                    }
                }
            }
            Term::Panic(kind) => self.call_panic(kind),
            Term::Par { .. } | Term::Suspend { .. } | Term::TaskEnd | Term::Unwind { .. } => {
                unreachable!("removed by the task transform")
            }
            Term::PanicFields { kind, fields } => self.call_panic_fields(kind, fields),
        }
    }

    /// A stack return area for a call whose results do not fit in registers.
    fn alloc_area(&mut self, ret: &[Ty]) -> Option<(Value, usize)> {
        if !returns_by_area(ret) {
            return None;
        }
        let n = ret.iter().filter(|t| clif(**t).is_some()).count();
        let slot = self.b.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            8 * n as u32,
            3,
        ));
        Some((self.b.ins().stack_addr(self.rt.ptr, slot, 0), n))
    }

    /// The call's results, read back from its return area when it has one.
    fn results(&mut self, c: CInst, area: Option<(Value, usize)>, ret: &[Ty]) -> Vec<Value> {
        let Some((area, _)) = area else {
            return self.b.inst_results(c).to_vec();
        };
        ret.iter()
            .filter_map(|t| clif(*t))
            .enumerate()
            .map(|(i, t)| {
                self.b
                    .ins()
                    .load(t, MemFlagsData::new(), area, 8 * i as i32)
            })
            .collect()
    }

    fn scalar(&mut self, op: ScalarOp, ty: Ty, a: &[Value]) -> Option<Value> {
        use ScalarOp::*;
        Some(match (op, ty) {
            (Exclusive, _) => {
                let ne = if ty == Ty::F64 {
                    self.b.ins().fcmp(FloatCC::NotEqual, a[0], a[1])
                } else {
                    self.b.ins().icmp(IntCC::NotEqual, a[0], a[1])
                };
                self.guard(ne, "exclusivity");
                return None;
            }
            // Mask arithmetic only: no branch, select or compare on the operands (R611).
            (CtSelect, _) => {
                let wide = self.b.ins().uextend(types::I64, a[0]);
                let mask = self.b.ins().ineg(wide);
                let diff = self.b.ins().bxor(a[1], a[2]);
                let pick = self.b.ins().band(diff, mask);
                self.b.ins().bxor(a[2], pick)
            }
            (CtEq, _) => {
                let x = self.b.ins().bxor(a[0], a[1]);
                let neg = self.b.ins().ineg(x);
                let nonzero = self.b.ins().bor(x, neg);
                let bit = self.b.ins().ushr_imm_u(nonzero, 63);
                let eq = self.b.ins().bxor_imm_u(bit, 1);
                self.b.ins().ireduce(types::I8, eq)
            }
            (CtLt, _) => {
                // sign of a - b, corrected for overflow
                let d = self.b.ins().isub(a[0], a[1]);
                let x = self.b.ins().bxor(a[0], a[1]);
                let y = self.b.ins().bxor(d, a[0]);
                let o = self.b.ins().band(x, y);
                let m = self.b.ins().bxor(d, o);
                let bit = self.b.ins().ushr_imm_u(m, 63);
                self.b.ins().ireduce(types::I8, bit)
            }
            (Not, Ty::Bool) => self.b.ins().bxor_imm_s(a[0], 1),
            (And, _) => self.b.ins().band(a[0], a[1]),
            (Or, _) => self.b.ins().bor(a[0], a[1]),
            (Xor, _) => self.b.ins().bxor(a[0], a[1]),
            (Add, Ty::F64) => self.b.ins().fadd(a[0], a[1]),
            (Sub, Ty::F64) => self.b.ins().fsub(a[0], a[1]),
            (Mul, Ty::F64) => self.b.ins().fmul(a[0], a[1]),
            (Div, Ty::F64) => self.b.ins().fdiv(a[0], a[1]),
            (Eq | Ne | Lt | Le | Gt | Ge, Ty::F64) => {
                let cc = match op {
                    Eq => FloatCC::Equal,
                    Ne => FloatCC::NotEqual,
                    Lt => FloatCC::LessThan,
                    Le => FloatCC::LessThanOrEqual,
                    Gt => FloatCC::GreaterThan,
                    _ => FloatCC::GreaterThanOrEqual,
                };
                self.b.ins().fcmp(cc, a[0], a[1])
            }
            (Eq | Ne | Lt | Le | Gt | Ge, Ty::Int(t)) => {
                let cc = match (op, t.signed) {
                    (Eq, _) => IntCC::Equal,
                    (Ne, _) => IntCC::NotEqual,
                    (Lt, true) => IntCC::SignedLessThan,
                    (Le, true) => IntCC::SignedLessThanOrEqual,
                    (Gt, true) => IntCC::SignedGreaterThan,
                    (Ge, true) => IntCC::SignedGreaterThanOrEqual,
                    (Lt, false) => IntCC::UnsignedLessThan,
                    (Le, false) => IntCC::UnsignedLessThanOrEqual,
                    (Gt, false) => IntCC::UnsignedGreaterThan,
                    _ => IntCC::UnsignedGreaterThanOrEqual,
                };
                self.b.ins().icmp(cc, a[0], a[1])
            }
            (Eq | Ne, Ty::Bool) => {
                let cc = if op == Eq {
                    IntCC::Equal
                } else {
                    IntCC::NotEqual
                };
                self.b.ins().icmp(cc, a[0], a[1])
            }
            (Div, Ty::Int(t)) if t.bits == 64 => {
                let f = if t.signed { RtFn::DivI64 } else { RtFn::DivU64 };
                let at = RT
                    .iter()
                    .position(|x| *x == f)
                    .expect("every RtFn is in RT");
                let c = self.b.ins().call(self.rt.fns[at], &[a[0], a[1]]);
                self.b.inst_results(c)[0]
            }
            // narrower operands convert exactly, so the division rounds once
            (Div, Ty::Int(t)) => {
                let (x, y) = if t.signed {
                    (
                        self.b.ins().fcvt_from_sint(types::F64, a[0]),
                        self.b.ins().fcvt_from_sint(types::F64, a[1]),
                    )
                } else {
                    (
                        self.b.ins().fcvt_from_uint(types::F64, a[0]),
                        self.b.ins().fcvt_from_uint(types::F64, a[1]),
                    )
                };
                self.b.ins().fdiv(x, y)
            }
            (Shl | Shr | WrappingShl | RotateLeft | RotateRight, Ty::Int(t)) => {
                self.shift(op, t, a[0], a[1])
            }
            (
                CountOnes | CountZeros | LeadingZeros | TrailingZeros | BitWidth | SwapBytes
                | ReverseBits | IsPowerOfTwo,
                Ty::Int(t),
            ) => self.bits(op, t, a[0]),
            (AddChecked | SubChecked | MulChecked, Ty::Int(t)) => self.checked(op, t, a[0], a[1]),
            (FloorDiv | Mod, Ty::Int(t)) => self.floor(op, t, a[0], a[1]),
            _ => unreachable!("yan-mir decoded {op:?} on {ty:?}"),
        })
    }

    /// A value held as its sign- or zero-extended 64-bit form: wrap `v` to `t`.
    /// `c - v`.
    fn rsub(&mut self, v: Value, c: i64) -> Value {
        let c = self.b.ins().iconst(types::I64, c);
        self.b.ins().isub(c, v)
    }

    fn wrap(&mut self, t: IntTy, v: Value) -> Value {
        if t.bits == 64 {
            return v;
        }
        let narrow = match t.bits {
            8 => types::I8,
            16 => types::I16,
            _ => types::I32,
        };
        let r = self.b.ins().ireduce(narrow, v);
        if t.signed {
            self.b.ins().sextend(types::I64, r)
        } else {
            self.b.ins().uextend(types::I64, r)
        }
    }

    /// The two's-complement pattern of `x` in the low `t.bits` bits.
    fn pattern(&mut self, t: IntTy, x: Value) -> Value {
        if t.bits == 64 || !t.signed {
            return x;
        }
        self.b.ins().band_imm_s(x, (1i64 << t.bits) - 1)
    }

    fn shift(&mut self, op: ScalarOp, t: IntTy, x: Value, n: Value) -> Value {
        use ScalarOp::*;
        let w = i64::from(t.bits);
        if matches!(op, RotateLeft | RotateRight) {
            let k = self.b.ins().band_imm_s(n, w - 1);
            let k = if op == RotateRight {
                let back = self.rsub(k, w);
                self.b.ins().band_imm_s(back, w - 1)
            } else {
                k
            };
            let p = self.pattern(t, x);
            if t.bits == 64 {
                return self.b.ins().rotl(p, k);
            }
            let up = self.b.ins().ishl(p, k);
            let down_by = self.rsub(k, w);
            let down = self.b.ins().ushr(p, down_by);
            let both = self.b.ins().bor(up, down);
            return self.wrap(t, both);
        }
        let ok = self.b.ins().icmp_imm_s(IntCC::UnsignedLessThan, n, w);
        self.guard(ok, "overflow");
        let r = match op {
            Shr if t.signed => return self.b.ins().sshr(x, n),
            Shr => return self.b.ins().ushr(x, n),
            _ => self.b.ins().ishl(x, n),
        };
        if op == WrappingShl {
            return self.wrap(t, r);
        }
        // The shifted value must fit `t` and lose no bits.
        if t.bits == 64 {
            let back = if t.signed {
                self.b.ins().sshr(r, n)
            } else {
                self.b.ins().ushr(r, n)
            };
            let same = self.b.ins().icmp(IntCC::Equal, back, x);
            self.guard(same, "overflow");
        } else {
            let (lo, hi) = t.range();
            let ge = self
                .b
                .ins()
                .icmp_imm_s(IntCC::SignedGreaterThanOrEqual, r, lo as i64);
            let le = self
                .b
                .ins()
                .icmp_imm_s(IntCC::SignedLessThanOrEqual, r, hi as i64);
            let ok = self.b.ins().band(ge, le);
            self.guard(ok, "overflow");
        }
        r
    }

    fn bits(&mut self, op: ScalarOp, t: IntTy, x: Value) -> Value {
        use ScalarOp::*;
        let w = i64::from(t.bits);
        let p = self.pattern(t, x);
        match op {
            CountOnes => self.b.ins().popcnt(p),
            CountZeros => {
                let ones = self.b.ins().popcnt(p);
                self.rsub(ones, w)
            }
            LeadingZeros => {
                let z = self.b.ins().clz(p);
                self.b.ins().iadd_imm_s(z, w - 64)
            }
            BitWidth => {
                let z = self.b.ins().clz(p);
                self.rsub(z, 64)
            }
            TrailingZeros => {
                let z = self.b.ins().ctz(p);
                let cap = self.b.ins().iconst(types::I64, w);
                self.b.ins().umin(z, cap)
            }
            SwapBytes | ReverseBits => {
                let r = if op == SwapBytes {
                    self.b.ins().bswap(p)
                } else {
                    self.b.ins().bitrev(p)
                };
                let r = self.b.ins().ushr_imm_s(r, 64 - w);
                self.wrap(t, r)
            }
            _ => {
                let m = self.b.ins().iadd_imm_s(x, -1);
                let both = self.b.ins().band(x, m);
                let single = self.b.ins().icmp_imm_s(IntCC::Equal, both, 0);
                let cc = if t.signed {
                    IntCC::SignedGreaterThan
                } else {
                    IntCC::NotEqual
                };
                let pos = self.b.ins().icmp_imm_s(cc, x, 0);
                self.b.ins().band(single, pos)
            }
        }
    }

    fn checked(&mut self, op: ScalarOp, t: IntTy, x: Value, y: Value) -> Value {
        use ScalarOp::*;
        if t.bits == 64 {
            let (r, of) = match (op, t.signed) {
                (AddChecked, true) => self.b.ins().sadd_overflow(x, y),
                (SubChecked, true) => self.b.ins().ssub_overflow(x, y),
                (MulChecked, true) => self.b.ins().smul_overflow(x, y),
                (AddChecked, false) => self.b.ins().uadd_overflow(x, y),
                (SubChecked, false) => self.b.ins().usub_overflow(x, y),
                _ => self.b.ins().umul_overflow(x, y),
            };
            let ok = self.b.ins().icmp_imm_s(IntCC::Equal, of, 0);
            self.guard_ops(ok, "overflow", t, x, y);
            return r;
        }
        // Narrower operands are exact in i64, or wrap to a negative value that is out of range.
        let r = match op {
            AddChecked => self.b.ins().iadd(x, y),
            SubChecked => self.b.ins().isub(x, y),
            _ => self.b.ins().imul(x, y),
        };
        self.in_range(r, t, x, y);
        r
    }

    fn in_range(&mut self, r: Value, t: IntTy, x: Value, y: Value) {
        let (lo, hi) = t.range();
        let ge = self
            .b
            .ins()
            .icmp_imm_s(IntCC::SignedGreaterThanOrEqual, r, lo as i64);
        let le = self
            .b
            .ins()
            .icmp_imm_s(IntCC::SignedLessThanOrEqual, r, hi as i64);
        let ok = self.b.ins().band(ge, le);
        self.guard_ops(ok, "overflow", t, x, y);
    }

    /// Division and remainder rounding toward negative infinity.
    fn floor(&mut self, op: ScalarOp, t: IntTy, x: Value, y: Value) -> Value {
        let nonzero = self.b.ins().icmp_imm_s(IntCC::NotEqual, y, 0);
        self.guard_ops(nonzero, "div_zero", t, x, y);
        if !t.signed {
            return if op == ScalarOp::FloorDiv {
                self.b.ins().udiv(x, y)
            } else {
                self.b.ins().urem(x, y)
            };
        }
        // `x / -1` is `-x`; dividing by -1 directly traps at i64::MIN.
        let minus_one = self.b.ins().icmp_imm_s(IntCC::Equal, y, -1);
        let one = self.b.ins().iconst(types::I64, 1);
        let d = self.b.ins().select(minus_one, one, y);
        let q = self.b.ins().sdiv(x, d);
        let r = self.b.ins().srem(x, d);
        let rnz = self.b.ins().icmp_imm_s(IntCC::NotEqual, r, 0);
        let sx = self.b.ins().bxor(r, y);
        let neg = self.b.ins().icmp_imm_s(IntCC::SignedLessThan, sx, 0);
        let adjust = self.b.ins().band(rnz, neg);
        if op == ScalarOp::Mod {
            let r2 = self.b.ins().iadd(r, y);
            return self.b.ins().select(adjust, r2, r);
        }
        let q1 = self.b.ins().iadd_imm_s(q, -1);
        let q = self.b.ins().select(adjust, q1, q);
        let zero = self.b.ins().iconst(types::I64, 0);
        let (negx, of) = self.b.ins().ssub_overflow(zero, x);
        let bad = self.b.ins().band(minus_one, of);
        let ok = self.b.ins().icmp_imm_s(IntCC::Equal, bad, 0);
        self.guard_ops(ok, "overflow", t, x, y);
        let q = self.b.ins().select(minus_one, negx, q);
        if t.bits < 64 {
            self.in_range(q, t, x, y);
        }
        q
    }
}

#[cfg(test)]
mod selftest {
    use super::{Jit, compile_with};

    fn run_both(p: &yan_mir::Program) {
        for jit in [Jit::Eager, Jit::Lazy] {
            compile_with(p, jit).expect("compiles").run();
        }
    }

    /// Runs in process: a failed `assert` in the program exits the test binary with 101.
    #[test]
    fn runs_loops_join_points_and_tail_calls() {
        let src = "module t\n\
            fn count(let n: i64, let acc: i64) -> i64 {\n let z: bool = eq_i64(let n, let 0)\n if z { ret acc }\n\
            let m: i64 = sub_checked_i64(let n, let 1)\n let a: i64 = add_checked_i64(let acc, let 1)\n become count(let m, let a)\n}\n\
            fn main() -> Unit {\n var n: i64 = 0\n jp k(let m: i64) { n = add_checked_i64(let n, let m) }\n\
            loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n if big { jump k(10) }\n }\n\
            assert eq_i64(let n, let 13)\n let r: i64 = count(let 1000000, let 0)\n assert eq_i64(let r, let 1000000)\n\
            let q: i64 = floordiv_i64(let -7, let 2)\n assert eq_i64(let q, let -4)\n\
            let w: u8 = mod_u8(let 7, let 3)\n assert eq_u8(let w, let 1)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yan_mir::lower(&m.unwrap()).expect("lowers");
        run_both(&p);
    }

    /// `std.ct` helpers agree with their definitions at the signed extremes (R611).
    #[test]
    fn runs_constant_time_helpers() {
        let src = "module t\n\
            fn main() -> Unit {\n\
            let lo: i64 = -9223372036854775807\n let hi: i64 = 9223372036854775807\n\
            let a: bool = std.ct.ct_lt(let lo, let 0)\n assert a\n\
            let b: bool = std.ct.ct_lt(let hi, let lo)\n let nb: bool = not_bool(let b)\n assert nb\n\
            let c: bool = std.ct.ct_lt(let 3, let 3)\n let nc: bool = not_bool(let c)\n assert nc\n\
            let d: bool = std.ct.ct_eq(let hi, let hi)\n assert d\n\
            let e: bool = std.ct.ct_eq(let lo, let hi)\n let ne: bool = not_bool(let e)\n assert ne\n\
            let f: i64 = std.ct.ct_select(let true, let 7, let 9)\n assert eq_i64(let f, let 7)\n\
            let g: i64 = std.ct.ct_select(let false, let 7, let 9)\n assert eq_i64(let g, let 9)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yan_mir::lower(&m.unwrap()).expect("lowers");
        run_both(&p);
    }

    /// Bounds that hold exactly: 50 loop back edges, and a new list plus 50 appends (R925).
    #[test]
    fn runs_cost_counters_that_hold() {
        let src = "module t\n\
            fn spin(let n: i64) -> i64\n cost steps <= n, alloc <= add_checked_i64(let n, let 1)\n{\n\
            var ys: List[i64] = std.list.new[i64]()\n var i: i64 = 0\n\
            loop 'l {\n let d: bool = ge_i64(let i, let n)\n if d { break 'l }\n\
            i = add_checked_i64(let i, let 1)\n std.list.append[i64](inout ys, sink i)\n }\n\
            let k: i64 = std.list.len[i64](let ys)\n ret k\n}\n\
            fn main() -> Unit {\n let r: i64 = spin(let 50)\n assert eq_i64(let r, let 50)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yan_mir::lower(&m.unwrap()).expect("lowers");
        run_both(&p);
    }

    /// `twice` propagates `check`'s error with `?`; `main` catches it with `try`.
    #[test]
    fn runs_fail_try_and_propagate() {
        let src = "module t\nrecord Bad { pub code: i64 }\n\
            fn check(let x: i64) -> i64 fail Bad {\n let neg: bool = lt_i64(let x, let 0)\n\
            if neg {\n let e: Bad = Bad { code: x }\n fail e\n }\n ret x\n}\n\
            fn twice(let x: i64) -> i64 fail Bad {\n let y: i64 = check(let x)?\n\
            let z: i64 = add_checked_i64(let y, let y)\n ret z\n}\n\
            fn main() -> Unit {\n let r: Result[i64, Bad] = try twice(let -3)\n\
            match r {\n Err { error: e } => {\n let c: i64 = e.code\n assert eq_i64(let c, let -3)\n }\n _ => { unreachable }\n }\n\
            let o: Result[i64, Bad] = try twice(let 4)\n\
            match o {\n Ok { value: 8 } => {}\n _ => { unreachable }\n }\n\
            let s: Option[i64] = Some { value: 2 }\n let t: Str = std.fmt.show[Option[i64]](let s)\n\
            match t {\n \"2\" => {}\n _ => { unreachable }\n }\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yan_mir::lower(&m.unwrap()).expect("lowers");
        run_both(&p);
    }

    /// Nine result slots exceed what aarch64's tail convention returns in registers: they
    /// come back through a return area, directly, indirectly and through `become`.
    #[test]
    fn returns_wide_records_through_an_area() {
        let src = "module t\nrecord W { pub a: i64, pub b: i64, pub c: i64, pub d: i64, pub e: i64, pub f: i64, pub g: i64, pub h: i64, pub i: i64 }\n\
            fn mk(let x: i64) -> W {\n let w: W = W { a: x, b: 2, c: 3, d: 4, e: 5, f: 6, g: 7, h: 8, i: 9 }\n ret w\n}\n\
            fn fwd(let x: i64) -> W {\n become mk(let x)\n}\n\
            fn main() -> Unit {\n let w: W = mk(let 1)\n let a: i64 = w.a\n assert eq_i64(let a, let 1)\n\
            let i: i64 = w.i\n assert eq_i64(let i, let 9)\n\
            let v: W = fwd(let 7)\n let b: i64 = v.a\n assert eq_i64(let b, let 7)\n let h: i64 = v.h\n assert eq_i64(let h, let 8)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yan_mir::lower(&m.unwrap()).expect("lowers");
        run_both(&p);
    }

    #[test]
    fn runs_generic_functions_and_closures() {
        let src = "module t\n\
            fn apply[T: Type, U: Type](let f: Fn(let T) -> U, let x: T) -> U {\n\
            let y: U = f(let x)\n ret y\n}\n\
            fn main() -> Unit {\n let k: i64 = 10\n\
            let add: Fn(let i64) -> i64 = fn [let k](let x: i64) -> i64 {\n\
            let s: i64 = add_checked_i64(let x, let k)\n ret s\n }\n\
            let r: i64 = apply[i64, i64](let add, let 5)\n assert eq_i64(let r, let 15)\n\
            let neg: Fn(let bool) -> bool = fn [](let b: bool) -> bool {\n let n: bool = not_bool(let b)\n ret n\n }\n\
            let q: bool = apply[bool, bool](let neg, let true)\n assert eq_bool(let q, let false)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let m = yan_opt::optimize("t.yir", &m.unwrap()).expect("specializes");
        let p = yan_mir::lower(&m).expect("lowers");
        assert_eq!(
            p.funcs.len(),
            5,
            "main, two instances of apply, two closures"
        );
        run_both(&p);
    }

    #[test]
    fn runs_sums_lists_inout_and_show() {
        let src = "module t\n\
            sum S { A { pub x: i64 }, B { pub w: i64, pub h: bool }, C }\n\
            record P { pub s: S, pub t: Tuple(f64, Str) }\n\
            fn bump(inout n: i64, let by: i64) -> Unit {\n n = add_checked_i64(let n, let by)\n ret ()\n}\n\
            fn main() -> Unit {\n var xs: List[S] = std.list.new[S]()\n let a: S = A { x: 7 }\n\
            std.list.append[S](inout xs, sink a)\n std.list.append[S](inout xs, sink C)\n\
            let b: S = B { w: 2, h: true }\n std.list.append[S](inout xs, sink b)\n\
            var n: i64 = 0\n let e: S = xs[2]\n\
            match e {\n B { w: w, h: true } => { bump(inout n, let w) }\n _ => { unreachable }\n }\n\
            bump(inout n, let 1)\n assert eq_i64(let n, let 3)\n\
            let t: Tuple(f64, Str) = (0.5, \"q\")\n let p: P = P { s: C, t: t }\n\
            let shown: Str = std.fmt.show[List[S]](let xs)\n\
            match shown {\n \"[A(x=7), C(), B(w=2, h=True)]\" => {}\n _ => { unreachable }\n }\n\
            let q: Str = std.fmt.show[P](let p)\n\
            match q {\n \"P(s=C(), t=(0.5, 'q'))\" => {}\n _ => { unreachable }\n }\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yan_mir::lower(&m.unwrap()).expect("lowers");
        run_both(&p);
    }

    /// Nested variants, or-patterns that bind, literals, ranges, tuples and `Str` through the
    /// decision-tree lowering of `match`.
    #[test]
    fn runs_decision_tree_matches() {
        let src = r#"module t1

sum Op { Add, Sub, Neg { k: i64 }, Lit { v: i64 } }
sum Pair2 { P { a: Op, b: Op }, Q }

fn f(let o: Op) -> i64 {
    jp k(let a: i64) { ret a }
    match o {
        Add => { jump k(1) }
        Neg { k: 0 } => { jump k(2) }
        Neg { k: n } | Lit { v: n } => { jump k(n) }
        _ => { jump k(7) }
    }
}

fn g(let p: Pair2) -> i64 {
    jp k(let a: i64) { ret a }
    match p {
        P { a: Add, b: Add } => { jump k(10) }
        P { a: _, b: Sub } => { jump k(20) }
        P { a: Lit { v: 1..=5 }, b: x } => { let r: i64 = f(let x)  jump k(r) }
        P { a: Lit { v: 6 }, b: _ } => { jump k(30) }
        P { .. } => { jump k(40) }
        Q => { jump k(50) }
    }
}

fn h(let t: Tuple(i64, bool)) -> i64 {
    jp k(let a: i64) { ret a }
    match t {
        (0, true) => { jump k(1) }
        (0, false) => { jump k(2) }
        (1, _) | (2, _) => { jump k(3) }
        (n, true) => { jump k(n) }
        (_, false) => { jump k(9) }
    }
}

fn s(let x: Str) -> i64 {
    jp k(let a: i64) { ret a }
    match x {
        "a" => { jump k(1) }
        "b" | "c" => { jump k(2) }
        _ => { jump k(3) }
    }
}

fn main() -> Unit {
    let a: i64 = f(let Add)
    assert eq_i64(let a, let 1)
    let n0: Op = Neg { k: 0 }
    let b: i64 = f(let n0)
    assert eq_i64(let b, let 2)
    let n5: Op = Neg { k: 5 }
    let c: i64 = f(let n5)
    assert eq_i64(let c, let 5)
    let l8: Op = Lit { v: 8 }
    let d: i64 = f(let l8)
    assert eq_i64(let d, let 8)
    let e: i64 = f(let Sub)
    assert eq_i64(let e, let 7)
    let p1: Pair2 = P { a: Add, b: Add }
    let r1: i64 = g(let p1)
    assert eq_i64(let r1, let 10)
    let p2: Pair2 = P { a: l8, b: Sub }
    let r2: i64 = g(let p2)
    assert eq_i64(let r2, let 20)
    let l3: Op = Lit { v: 3 }
    let p3: Pair2 = P { a: l3, b: n5 }
    let r3: i64 = g(let p3)
    assert eq_i64(let r3, let 5)
    let l6: Op = Lit { v: 6 }
    let p4: Pair2 = P { a: l6, b: Add }
    let r4: i64 = g(let p4)
    assert eq_i64(let r4, let 30)
    let p5: Pair2 = P { a: Add, b: n5 }
    let r5: i64 = g(let p5)
    assert eq_i64(let r5, let 40)
    let r6: i64 = g(let Q)
    assert eq_i64(let r6, let 50)
    let at1: Tuple(i64, bool) = (0, true)
    let t1: i64 = h(let at1)
    assert eq_i64(let t1, let 1)
    let at2: Tuple(i64, bool) = (0, false)
    let t2: i64 = h(let at2)
    assert eq_i64(let t2, let 2)
    let at3: Tuple(i64, bool) = (2, false)
    let t3: i64 = h(let at3)
    assert eq_i64(let t3, let 3)
    let at4: Tuple(i64, bool) = (8, true)
    let t4: i64 = h(let at4)
    assert eq_i64(let t4, let 8)
    let at5: Tuple(i64, bool) = (8, false)
    let t5: i64 = h(let at5)
    assert eq_i64(let t5, let 9)
    let s1: i64 = s(let "c")
    assert eq_i64(let s1, let 2)
    let s2: i64 = s(let "z")
    assert eq_i64(let s2, let 3)
    ret ()
}
"#;
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yan_mir::lower(&m.unwrap()).expect("lowers");
        super::compile(&p).expect("compiles").run();
    }
}
