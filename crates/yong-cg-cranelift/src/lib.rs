//! Debug code generation (M1b): yong-mir to machine code with Cranelift, run in process. Std
//! calls go to yong-rt's C-ABI symbols (R561); scalar intrinsics are lowered inline. Every
//! function uses the tail calling convention so `become` is a `return_call` (3.5).

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
use yong_mir::{Func, Inst, IntTy, Operand, Program, RtFn, ScalarOp, Term, Ty};
use yong_rt::abi;

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
    _module: JITModule,
    entry: extern "C" fn(),
}

impl Compiled {
    /// Run `main`. A panic exits the process with status 101 (8.3).
    pub fn run(&self) {
        (self.entry)()
    }
}

/// Compile `p` for the host.
pub fn compile(p: &Program) -> R<Compiled> {
    let mut flags = settings::builder();
    flags.set("opt_level", "none").map_err(err)?;
    // Cranelift's tail calls need frame pointers.
    flags.set("preserve_frame_pointers", "true").map_err(err)?;
    let isa = cranelift_native::builder()
        .map_err(err)?
        .finish(settings::Flags::new(flags))
        .map_err(err)?;
    let mut jb = JITBuilder::with_isa(isa, default_libcall_names());
    jb.symbol("yong_rt_panic", abi::yong_rt_panic as *const u8);
    jb.symbol("yong_rt_exit", abi::yong_rt_exit as *const u8);
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
        .declare_function("yong_rt_panic", Linkage::Import, &panic_sig)
        .map_err(err)?;
    let mut exit_sig = module.make_signature();
    exit_sig.params.push(AbiParam::new(types::I64));
    let exit = module
        .declare_function("yong_rt_exit", Linkage::Import, &exit_sig)
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
                .declare_function(&format!("yong.{i}.{}", f.name), Linkage::Local, s)
                .map_err(err)
        })
        .collect::<R<_>>()?;
    // Owned by the program: the JIT code refers to them by address.
    let strings: Vec<i64> = p.strings.iter().map(|s| abi::str_block(s) as i64).collect();

    let mut fctx = FunctionBuilderContext::new();
    let mut ctx = module.make_context();
    for (i, f) in p.funcs.iter().enumerate() {
        ctx.func.signature = sigs[i].clone();
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let funcs: Vec<FuncRef> = ids
            .iter()
            .map(|id| module.declare_func_in_func(*id, b.func))
            .collect();
        let rt = Rt {
            fns: rt_ids
                .iter()
                .map(|id| module.declare_func_in_func(*id, b.func))
                .collect(),
            panic: module.declare_func_in_func(panic, b.func),
            ptr,
        };
        Gen {
            b: &mut b,
            f,
            vars: Vec::new(),
            blocks: Vec::new(),
            funcs,
            rt,
            strings: &strings,
            kinds: HashMap::new(),
        }
        .func();
        b.finalize(module.target_config());
        module.define_function(ids[i], &mut ctx).map_err(err)?;
        module.clear_context(&mut ctx);
    }

    // The host calls this C-ABI trampoline, which calls the tail-convention `main`.
    let entry_sig = module.make_signature();
    let entry = module
        .declare_function("yong.entry", Linkage::Local, &entry_sig)
        .map_err(err)?;
    ctx.func.signature = entry_sig;
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let blk = b.create_block();
        b.switch_to_block(blk);
        b.seal_block(blk);
        let main = module.declare_func_in_func(ids[p.main as usize], b.func);
        let c = b.ins().call(main, &[]);
        if p.main_fails {
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
    module.define_function(entry, &mut ctx).map_err(err)?;
    module.clear_context(&mut ctx);
    module.finalize_definitions().map_err(err)?;
    let code = module.get_finalized_function(entry);
    #[allow(unsafe_code)]
    // SAFETY: `yong.entry` was defined above with the host's default C signature, no arguments
    // and no result.
    let entry = unsafe { std::mem::transmute::<*const u8, extern "C" fn()>(code) };
    Ok(Compiled {
        _module: module,
        entry,
    })
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
            "yong_rt_print_str",
            abi::yong_rt_print_str as *const u8,
            &[I64],
            None,
        ),
        RtFn::Concat => (
            "yong_rt_concat",
            abi::yong_rt_concat as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::ShowI64 => (
            "yong_rt_show_i64",
            abi::yong_rt_show_i64 as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ShowU64 => (
            "yong_rt_show_u64",
            abi::yong_rt_show_u64 as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ShowF64 => (
            "yong_rt_show_f64",
            abi::yong_rt_show_f64 as *const u8,
            &[F64],
            Some(I64),
        ),
        RtFn::DivI64 => (
            "yong_rt_div_i64",
            abi::yong_rt_div_i64 as *const u8,
            &[I64, I64],
            Some(F64),
        ),
        RtFn::DivU64 => (
            "yong_rt_div_u64",
            abi::yong_rt_div_u64 as *const u8,
            &[I64, I64],
            Some(F64),
        ),
        RtFn::ShowBool => (
            "yong_rt_show_bool",
            abi::yong_rt_show_bool as *const u8,
            &[I8],
            Some(I64),
        ),
        RtFn::Repr => (
            "yong_rt_repr",
            abi::yong_rt_repr as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::StrEq => (
            "yong_rt_str_eq",
            abi::yong_rt_str_eq as *const u8,
            &[I64, I64],
            Some(I8),
        ),
        RtFn::ListNew => (
            "yong_rt_list_new",
            abi::yong_rt_list_new as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListPush => (
            "yong_rt_list_push",
            abi::yong_rt_list_push as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListLen => (
            "yong_rt_list_len",
            abi::yong_rt_list_len as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListAt => (
            "yong_rt_list_at",
            abi::yong_rt_list_at as *const u8,
            &[I64, I64],
            Some(I64),
        ),
        RtFn::Alloc => (
            "yong_rt_alloc",
            abi::yong_rt_alloc as *const u8,
            &[I64],
            Some(I64),
        ),
        RtFn::ListCopy => (
            "yong_rt_list_copy",
            abi::yong_rt_list_copy as *const u8,
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
    funcs: Vec<FuncRef>,
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
            Operand::Func(f) => self.b.ins().func_addr(types::I64, self.funcs[*f as usize]),
        })
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
                let c = self.b.ins().call(self.funcs[*func as usize], &args);
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
                self.b.ins().return_call(self.funcs[*func as usize], &args);
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
            _ => unreachable!("yong-mir decoded {op:?} on {ty:?}"),
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
        let (m, d) = yong_core::parse("t.yong", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yong_mir::lower(&m.unwrap()).expect("lowers");
        super::compile(&p).expect("compiles").run();
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
        let (m, d) = yong_core::parse("t.yong", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yong_mir::lower(&m.unwrap()).expect("lowers");
        super::compile(&p).expect("compiles").run();
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
        let (m, d) = yong_core::parse("t.yong", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yong_mir::lower(&m.unwrap()).expect("lowers");
        assert_eq!(
            p.funcs.len(),
            5,
            "main, two instances of apply, two closures"
        );
        super::compile(&p).expect("compiles").run();
    }

    #[test]
    fn runs_sums_lists_inout_and_show() {
        let src = "module t\n\
            sum S { A { pub x: i64 }, B { pub w: i64, pub h: bool }, C }\n\
            record P { pub s: S, pub t: Tuple(f64, Str) }\n\
            fn bump(inout n: i64, let by: i64) -> Unit {\n n = add_checked_i64(let n, let by)\n ret ()\n}\n\
            fn main() -> Unit {\n var xs: List[S] = std.list.new[S]()\n let a: S = A { x: 7 }\n\
            std.list.push[S](inout xs, sink a)\n std.list.push[S](inout xs, sink C)\n\
            let b: S = B { w: 2, h: true }\n std.list.push[S](inout xs, sink b)\n\
            var n: i64 = 0\n let e: S = xs[2]\n\
            match e {\n B { w: w, h: true } => { bump(inout n, let w) }\n _ => { unreachable }\n }\n\
            bump(inout n, let 1)\n assert eq_i64(let n, let 3)\n\
            let t: Tuple(f64, Str) = (0.5, \"q\")\n let p: P = P { s: C, t: t }\n\
            let shown: Str = std.fmt.show[List[S]](let xs)\n\
            match shown {\n \"[A(x=7), C(), B(w=2, h=True)]\" => {}\n _ => { unreachable }\n }\n\
            let q: Str = std.fmt.show[P](let p)\n\
            match q {\n \"P(s=C(), t=(0.5, 'q'))\" => {}\n _ => { unreachable }\n }\n ret ()\n}\n";
        let (m, d) = yong_core::parse("t.yong", src);
        assert!(d.is_empty(), "{d:?}");
        let p = yong_mir::lower(&m.unwrap()).expect("lowers");
        super::compile(&p).expect("compiles").run();
    }
}
