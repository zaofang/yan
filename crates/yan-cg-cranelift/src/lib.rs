//! Debug code generation (M1b): yan-mir to machine code with Cranelift, run in process. Std
//! calls go to yan-rt's C-ABI symbols (R561); scalar intrinsics are lowered inline. Every
//! function uses the tail calling convention so `become` is a `return_call` (3.5).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block as CBlock, FuncRef, InstBuilder, MemFlagsData, Signature, TrapCode, Value,
    types,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Switch, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{FuncId as CFuncId, Linkage, Module, default_libcall_names};
use yan_mir::{Func, Inst, IntTy, Operand, Program, RtFn, ScalarOp, Term, Ty};
use yan_rt::abi;

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

/// A compiled program, ready to run.
pub struct Compiled {
    _lazy: Box<Lazy>,
    entry: extern "C" fn(),
}

impl Compiled {
    /// Run `main`. A panic exits the process with status 101 (8.3).
    pub fn run(&self) {
        (self.entry)()
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

/// The module and what it takes to define more functions in it.
struct State {
    module: JITModule,
    program: Program,
    sigs: Vec<Signature>,
    ids: Vec<CFuncId>,
    rt_ids: Vec<CFuncId>,
    panic: CFuncId,
    /// Owned by the program: the JIT code refers to them by address.
    strings: Vec<i64>,
    fctx: FunctionBuilderContext,
    ctx: cranelift_codegen::Context,
}

/// Owns the module; its address is baked into the stubs, so it is boxed.
struct Lazy {
    state: RefCell<State>,
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

impl State {
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
            panic: module.declare_func_in_func(*panic, b.func),
            ptr,
        };
        Gen {
            b: &mut b,
            f: &program.funcs[i],
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

/// Compile `p` for the host.
pub fn compile_with(p: &Program, jit: Jit) -> R<Compiled> {
    let mut flags = settings::builder();
    flags.set("opt_level", "none").map_err(err)?;
    // Cranelift's tail calls need frame pointers.
    flags.set("preserve_frame_pointers", "true").map_err(err)?;
    let isa = cranelift_native::builder()
        .map_err(err)?
        .finish(settings::Flags::new(flags))
        .map_err(err)?;
    let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
    jb.symbol("yan_rt_panic", abi::yan_rt_panic as *const u8);
    jb.symbol("yan_rt_exit", abi::yan_rt_exit as *const u8);
    jb.symbol("yan_jit_resolve", yan_jit_resolve as *const u8);
    for f in RT {
        let (name, addr, ..) = rt_fn(*f);
        jb.symbol(name, addr);
    }
    let mut module = JITModule::new(jb);
    let ptr = module.target_config().pointer_type();

    let mut panic_sig = module.make_signature();
    panic_sig
        .params
        .extend([AbiParam::new(ptr), AbiParam::new(ptr)]);
    let panic = module
        .declare_function("yan_rt_panic", Linkage::Import, &panic_sig)
        .map_err(err)?;
    let mut exit_sig = module.make_signature();
    exit_sig.params.push(AbiParam::new(types::I64));
    let exit = module
        .declare_function("yan_rt_exit", Linkage::Import, &exit_sig)
        .map_err(err)?;
    let mut resolve_sig = module.make_signature();
    resolve_sig
        .params
        .extend([AbiParam::new(ptr), AbiParam::new(types::I64)]);
    resolve_sig.returns.push(AbiParam::new(ptr));
    let resolve = module
        .declare_function("yan_jit_resolve", Linkage::Import, &resolve_sig)
        .map_err(err)?;
    let rt_ids = RT
        .iter()
        .map(|f| {
            let (name, _, params, ret) = rt_fn(*f);
            let mut sig = module.make_signature();
            sig.params.extend(params.iter().map(|t| AbiParam::new(*t)));
            sig.returns.extend(ret.map(AbiParam::new));
            module
                .declare_function(name, Linkage::Import, &sig)
                .map_err(err)
        })
        .collect::<R<Vec<_>>>()?;

    let sigs: Vec<Signature> = p.funcs.iter().map(|f| signature(f, &module)).collect();
    let ids: Vec<CFuncId> = p
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
            strings: p.strings.iter().map(|s| abi::str_block(s) as i64).collect(),
            fctx: FunctionBuilderContext::new(),
            ctx,
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
        entry_point(&mut st, main, p.main_fails, exit)?
    };
    Ok(Compiled { _lazy: lazy, entry })
}

/// One stub per function, with its signature: resolve the function, then tail-call it with
/// the same arguments.
fn stubs(st: &mut State, lazy: &Lazy, resolve: CFuncId) -> R<Vec<CFuncId>> {
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
fn entry_point(st: &mut State, main: CFuncId, fails: bool, exit: CFuncId) -> R<extern "C" fn()> {
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
        let c = b.ins().call(main, &[]);
        if fails {
            let tag = b.inst_results(c)[0];
            let (failed, done) = (b.create_block(), b.create_block());
            b.ins().brif(tag, failed, &[], done, &[]);
            b.switch_to_block(failed);
            let exit = module.declare_func_in_func(exit, b.func);
            let one = b.ins().iconst(types::I64, 1);
            b.ins().call(exit, &[one]);
            b.ins().trap(TrapCode::unwrap_user(1));
            b.switch_to_block(done);
            b.seal_all_blocks();
        }
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

fn signature(f: &Func, m: &JITModule) -> Signature {
    let mut s = m.make_signature();
    s.call_conv = CallConv::Tail;
    for t in &f.locals[..f.params as usize] {
        if let Some(t) = clif(*t) {
            s.params.push(AbiParam::new(t));
        }
    }
    s.returns
        .extend(f.ret.iter().filter_map(|t| clif(*t)).map(AbiParam::new));
    s
}

const RT: &[RtFn] = &[
    RtFn::Print,
    RtFn::Concat,
    RtFn::ShowI64,
    RtFn::ShowU64,
    RtFn::ShowF64,
    RtFn::DivI64,
    RtFn::DivU64,
    RtFn::ShowBool,
    RtFn::Repr,
    RtFn::StrEq,
    RtFn::ListNew,
    RtFn::ListPush,
    RtFn::ListLen,
    RtFn::ListAt,
    RtFn::ListCopy,
    RtFn::Alloc,
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
        RtFn::StrEq => (
            "yan_rt_str_eq",
            abi::yan_rt_str_eq as *const u8,
            &[I64, I64],
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
        RtFn::Alloc => (
            "yan_rt_alloc",
            abi::yan_rt_alloc as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListCopy => (
            "yan_rt_list_copy",
            abi::yan_rt_list_copy as *const u8,
            &[I64],
            Some(I64),
        ),
    }
}

struct Rt {
    /// In the order of [`RT`].
    fns: Vec<FuncRef>,
    panic: FuncRef,
    ptr: types::Type,
}

struct Gen<'a, 'b> {
    b: &'a mut FunctionBuilder<'b>,
    f: &'a Func,
    vars: Vec<Option<Variable>>,
    blocks: Vec<CBlock>,
    calls: Calls,
    rt: Rt,
    strings: &'a [i64],
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
            for inst in &blk.insts {
                self.inst(inst);
            }
            self.term(&blk.term);
        }
        let kinds: Vec<(&'static str, CBlock)> = self.kinds.drain().collect();
        for (kind, blk) in kinds {
            self.b.switch_to_block(blk);
            self.call_panic(kind);
        }
        self.b.seal_all_blocks();
    }

    fn call_panic(&mut self, kind: &'static str) {
        let ptr = self.b.ins().iconst(self.rt.ptr, kind.as_ptr() as i64);
        let len = self.b.ins().iconst(self.rt.ptr, kind.len() as i64);
        self.b.ins().call(self.rt.panic, &[ptr, len]);
        self.b.ins().trap(TrapCode::unwrap_user(1));
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
            Operand::Str(s) => self.b.ins().iconst(types::I64, self.strings[*s as usize]),
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
                let args = self.vals(args);
                let c = match &self.calls {
                    Calls::Direct(fs) => self.b.ins().call(fs[*func as usize], &args),
                    Calls::Table { table, sigs } => {
                        let sig = sigs[*func as usize];
                        let code = self.slot(*table, *func);
                        self.b.ins().call_indirect(sig, code, &args)
                    }
                };
                let rs = self.b.inst_results(c).to_vec();
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
                sig.params
                    .extend(params.iter().filter_map(|t| clif(*t)).map(AbiParam::new));
                sig.returns
                    .extend(ret.iter().filter_map(|t| clif(*t)).map(AbiParam::new));
                let sig = self.b.import_signature(sig);
                let code = self.val(code).expect("a code address");
                let args = self.vals(args);
                let c = self.b.ins().call_indirect(sig, code, &args);
                let rs = self.b.inst_results(c).to_vec();
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
                self.b.ins().return_(&v);
            }
            Term::TailCall { func, args } => {
                let args = self.vals(args);
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
        }
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
            (AddChecked | SubChecked | MulChecked, Ty::Int(t)) => self.checked(op, t, a[0], a[1]),
            (FloorDiv | Mod, Ty::Int(t)) => self.floor(op, t, a[0], a[1]),
            _ => unreachable!("yan-mir decoded {op:?} on {ty:?}"),
        })
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
            self.guard(ok, "overflow");
            return r;
        }
        // Narrower operands are exact in i64, or wrap to a negative value that is out of range.
        let r = match op {
            AddChecked => self.b.ins().iadd(x, y),
            SubChecked => self.b.ins().isub(x, y),
            _ => self.b.ins().imul(x, y),
        };
        self.in_range(r, t);
        r
    }

    fn in_range(&mut self, r: Value, t: IntTy) {
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

    /// Division and remainder rounding toward negative infinity.
    fn floor(&mut self, op: ScalarOp, t: IntTy, x: Value, y: Value) -> Value {
        let nonzero = self.b.ins().icmp_imm_s(IntCC::NotEqual, y, 0);
        self.guard(nonzero, "div_zero");
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
        self.guard(ok, "overflow");
        let q = self.b.ins().select(minus_one, negx, q);
        if t.bits < 64 {
            self.in_range(q, t);
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
