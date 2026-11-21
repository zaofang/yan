//! The MIR validator (RFC 0097 D7). It checks the invariants a code generator relies on and
//! reports each violation with an I-code naming the rule, over parsed or in-memory MIR.

use std::fmt::{self, Display, Formatter};

use crate::{BlockId, Func, Inst, Loc, Local, Operand, Program, Term, Ty};

/// The form a program is in: backends read [`Form::Backend`], which is after the task transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// Between lowering and the task transform: `Par`, `Suspend`, `TaskEnd` and `Unwind` allowed.
    Lowered,
    /// What backends read: none of the four.
    Backend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// A local, function, string or pinned-group member does not exist; `main` is not a
    /// `unit`-returning function without parameters; `lines` and `insts` differ in length.
    Id,
    /// A function has no entry block, or a terminator names a block that does not exist.
    Target,
    /// A local is read on a path where nothing wrote it.
    UseBeforeDef,
    /// A call or tail call passes the wrong number of arguments or binds the wrong number of
    /// results.
    Arity,
    /// An argument, result or returned value has a type the callee or the function does not.
    Type,
    /// `Par`, `Suspend`, `TaskEnd` or `Unwind` in the backend form.
    PreTransform,
    /// `AddrOf` of a local that no pinned group holds, or a local in two groups.
    Pinned,
}

impl Rule {
    pub fn code(self) -> &'static str {
        match self {
            Rule::Id => "I101",
            Rule::Target => "I102",
            Rule::UseBeforeDef => "I103",
            Rule::Arity => "I104",
            Rule::Type => "I105",
            Rule::PreTransform => "I106",
            Rule::Pinned => "I107",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub rule: Rule,
    pub func: String,
    pub block: Option<BlockId>,
    /// Index of the instruction; `None` for the terminator and for function-level facts.
    pub inst: Option<usize>,
    pub message: String,
}

impl Display for Violation {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{}: fn \"{}\"", self.rule.code(), self.func)?;
        if let Some(b) = self.block {
            write!(f, " bb{b}")?;
            if let Some(i) = self.inst {
                write!(f, " inst {i}")?;
            }
        }
        write!(f, ": {}", self.message)
    }
}

impl Program {
    /// Every violation of the validator's rules, in function, block, instruction order.
    pub fn validate(&self, form: Form) -> Vec<Violation> {
        let mut out = Vec::new();
        self.check_main(&mut out);
        for f in &self.funcs {
            Checker {
                p: self,
                f,
                form,
                out: &mut out,
            }
            .run();
        }
        out
    }

    fn check_main(&self, out: &mut Vec<Violation>) {
        let bad = |message: String| Violation {
            rule: Rule::Id,
            func: "main".into(),
            block: None,
            inst: None,
            message,
        };
        match self.funcs.get(self.main as usize) {
            None => out.push(bad(format!(
                "main names fn{}, which does not exist",
                self.main
            ))),
            Some(m) if m.params != 0 || m.ret != [Ty::Unit] => out.push(bad(format!(
                "main fn{} must take no parameters and return unit",
                self.main
            ))),
            Some(_) => {}
        }
    }
}

struct Checker<'a> {
    p: &'a Program,
    f: &'a Func,
    form: Form,
    out: &'a mut Vec<Violation>,
}

/// Whether an operand of this type can stand where a slot of type `want` is read. Integer
/// literals carry no width, so they fit any integer slot, and a pointer slot as a dead filler.
fn fits(got: Option<Ty>, op: &Operand, want: Ty) -> bool {
    match op {
        Operand::Int(_) => matches!(want, Ty::Int(_) | Ty::Ptr),
        _ => got.is_none_or(|g| g == want),
    }
}

fn successors(t: &Term) -> Vec<BlockId> {
    match t {
        Term::Jump(b) => vec![*b],
        Term::Branch { then, els, .. } => vec![*then, *els],
        Term::Switch { targets, .. } => targets.clone(),
        Term::Par { tasks, join } => tasks.iter().copied().chain([*join]).collect(),
        Term::Suspend { resume } => vec![*resume],
        Term::Unwind { drops } => vec![*drops],
        Term::Return(_)
        | Term::TailCall { .. }
        | Term::Panic(_)
        | Term::PanicFields { .. }
        | Term::TaskEnd => {
            vec![]
        }
    }
}

impl Checker<'_> {
    fn report(&mut self, rule: Rule, block: Option<BlockId>, inst: Option<usize>, message: String) {
        self.out.push(Violation {
            rule,
            func: self.f.name.clone(),
            block,
            inst,
            message,
        });
    }

    fn run(&mut self) {
        let f = self.f;
        if f.params as usize > f.locals.len() {
            self.report(
                Rule::Id,
                None,
                None,
                format!("{} parameters but {} locals", f.params, f.locals.len()),
            );
        }
        self.pinned();
        if f.blocks.is_empty() {
            self.report(Rule::Target, None, None, "no entry block".into());
            return;
        }
        for (b, blk) in f.blocks.iter().enumerate() {
            let b = b as BlockId;
            if blk.lines.len() != blk.insts.len() {
                self.report(
                    Rule::Id,
                    Some(b),
                    None,
                    "`lines` and `insts` differ in length".into(),
                );
            }
            for (i, inst) in blk.insts.iter().enumerate() {
                self.inst(b, i, inst);
            }
            self.term(b, &blk.term);
        }
        self.defs();
    }

    fn pinned(&mut self) {
        let mut seen = vec![false; self.f.locals.len()];
        for l in self.f.pinned.iter().flatten() {
            match seen.get_mut(*l as usize) {
                None => self.report(Rule::Id, None, None, format!("pinned %{l} is not a local")),
                Some(s) if *s => self.report(
                    Rule::Pinned,
                    None,
                    None,
                    format!("%{l} is in two pinned groups"),
                ),
                Some(s) => *s = true,
            }
        }
    }

    fn is_pinned(&self, l: Local) -> bool {
        self.f.pinned.iter().flatten().any(|p| *p == l)
    }

    fn local_ty(&self, l: Local) -> Option<Ty> {
        self.f.locals.get(l as usize).copied()
    }

    fn operand_ty(&self, o: &Operand) -> Option<Ty> {
        match o {
            Operand::Local(l) => self.local_ty(*l),
            Operand::Int(_) => None,
            Operand::F64(_) => Some(Ty::F64),
            Operand::Bool(_) => Some(Ty::Bool),
            Operand::Unit => Some(Ty::Unit),
            Operand::Str(_) | Operand::Func(_) => Some(Ty::Ptr),
        }
    }

    fn operand(&mut self, b: BlockId, i: Option<usize>, o: &Operand) {
        match o {
            Operand::Local(l) if self.local_ty(*l).is_none() => {
                self.report(Rule::Id, Some(b), i, format!("%{l} is not a local"));
            }
            Operand::Str(s) if *s as usize >= self.p.strings.len() => {
                self.report(Rule::Id, Some(b), i, format!("string {s} does not exist"));
            }
            Operand::Func(g) if *g as usize >= self.p.funcs.len() => {
                self.report(Rule::Id, Some(b), i, format!("fn{g} does not exist"));
            }
            _ => {}
        }
    }

    fn dst(&mut self, b: BlockId, i: Option<usize>, l: Local) {
        if self.local_ty(l).is_none() {
            self.report(Rule::Id, Some(b), i, format!("%{l} is not a local"));
        }
    }

    fn inst(&mut self, b: BlockId, i: usize, inst: &Inst) {
        let at = Some(i);
        for o in operands(inst) {
            self.operand(b, at, o);
        }
        for l in dsts(inst) {
            self.dst(b, at, l);
        }
        match inst {
            Inst::Call { dst, func, args } => match self.p.funcs.get(*func as usize) {
                None => self.report(Rule::Id, Some(b), at, format!("fn{func} does not exist")),
                Some(g) => {
                    let params: Vec<Ty> =
                        g.locals.iter().take(g.params as usize).copied().collect();
                    self.signature(b, at, &format!("fn{func}"), dst, args, &params, &g.ret);
                }
            },
            Inst::CallIndirect {
                dst,
                args,
                params,
                ret,
                ..
            } => {
                self.signature(b, at, "indirect callee", dst, args, params, ret);
            }
            Inst::AddrOf { local, .. } => {
                if self.local_ty(*local).is_none() {
                    self.report(Rule::Id, Some(b), at, format!("%{local} is not a local"));
                } else if !self.is_pinned(*local) {
                    self.report(
                        Rule::Pinned,
                        Some(b),
                        at,
                        format!("address of %{local}, which is not pinned"),
                    );
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn signature(
        &mut self,
        b: BlockId,
        i: Option<usize>,
        callee: &str,
        dst: &[Local],
        args: &[Operand],
        params: &[Ty],
        ret: &[Ty],
    ) {
        if args.len() != params.len() {
            self.report(
                Rule::Arity,
                Some(b),
                i,
                format!(
                    "{callee} takes {} arguments, got {}",
                    params.len(),
                    args.len()
                ),
            );
        } else {
            for (n, (a, want)) in args.iter().zip(params).enumerate() {
                if !fits(self.operand_ty(a), a, *want) {
                    self.report(
                        Rule::Type,
                        Some(b),
                        i,
                        format!("argument {n} of {callee} is not {want}"),
                    );
                }
            }
        }
        if dst.len() != ret.len() {
            self.report(
                Rule::Arity,
                Some(b),
                i,
                format!("{callee} returns {} values, {} bound", ret.len(), dst.len()),
            );
        } else {
            for (n, (d, want)) in dst.iter().zip(ret).enumerate() {
                if self.local_ty(*d).is_some_and(|t| t != *want) {
                    self.report(
                        Rule::Type,
                        Some(b),
                        i,
                        format!("result {n} of {callee} is {want}, %{d} is not"),
                    );
                }
            }
        }
    }

    fn term(&mut self, b: BlockId, t: &Term) {
        for o in term_operands(t) {
            self.operand(b, None, o);
        }
        for s in successors(t) {
            if s as usize >= self.f.blocks.len() {
                self.report(
                    Rule::Target,
                    Some(b),
                    None,
                    format!("target bb{s} does not exist"),
                );
            }
        }
        if let Term::Switch { on, .. } = t
            && self.local_ty(*on).is_none()
        {
            self.report(Rule::Id, Some(b), None, format!("%{on} is not a local"));
        }
        if self.form == Form::Backend
            && matches!(
                t,
                Term::Par { .. } | Term::Suspend { .. } | Term::TaskEnd | Term::Unwind { .. }
            )
        {
            self.report(
                Rule::PreTransform,
                Some(b),
                None,
                "terminator exists only before the task transform".into(),
            );
        }
        match t {
            Term::Return(vals) => {
                let ret = &self.f.ret;
                if vals.len() != ret.len() {
                    self.report(
                        Rule::Arity,
                        Some(b),
                        None,
                        format!(
                            "returns {} values, the function has {}",
                            vals.len(),
                            ret.len()
                        ),
                    );
                } else {
                    for (n, (v, want)) in vals.iter().zip(ret.clone()).enumerate() {
                        if !fits(self.operand_ty(v), v, want) {
                            self.report(
                                Rule::Type,
                                Some(b),
                                None,
                                format!("returned value {n} is not {want}"),
                            );
                        }
                    }
                }
            }
            Term::TailCall { func, args } => {
                match self.p.funcs.get(*func as usize) {
                    None => {
                        self.report(Rule::Id, Some(b), None, format!("fn{func} does not exist"))
                    }
                    Some(g) => {
                        let params: Vec<Ty> =
                            g.locals.iter().take(g.params as usize).copied().collect();
                        if g.ret != self.f.ret {
                            self.report(Rule::Type, Some(b), None, format!("tail call of fn{func}, whose results differ from this function's"));
                        }
                        self.signature(b, None, &format!("fn{func}"), &[], args, &params, &[]);
                    }
                }
            }
            _ => {}
        }
    }

    /// Forward must-defined analysis: a local read must be written on every path from the entry.
    /// Pinned locals are written through their address, and `unit` locals carry no value, so both
    /// count as defined throughout.
    fn defs(&mut self) {
        let f = self.f;
        let n = f.locals.len();
        let nb = f.blocks.len();
        let mut entry: Vec<Option<Vec<bool>>> = vec![None; nb];
        let mut start = vec![false; n];
        for (l, d) in start.iter_mut().enumerate() {
            *d = l < f.params as usize || f.locals[l] == Ty::Unit || self.is_pinned(l as Local);
        }
        entry[0] = Some(start);
        let mut work = vec![0usize];
        while let Some(b) = work.pop() {
            let mut cur = entry[b].clone().expect("queued blocks have a state");
            for inst in &f.blocks[b].insts {
                for l in dsts(inst) {
                    if let Some(d) = cur.get_mut(l as usize) {
                        *d = true;
                    }
                }
            }
            for s in successors(&f.blocks[b].term) {
                let s = s as usize;
                if s >= nb {
                    continue;
                }
                let next = match &entry[s] {
                    None => cur.clone(),
                    Some(old) => old.iter().zip(&cur).map(|(a, b)| *a && *b).collect(),
                };
                if entry[s].as_ref() != Some(&next) {
                    entry[s] = Some(next);
                    work.push(s);
                }
            }
        }
        for (b, state) in entry.into_iter().enumerate() {
            let Some(mut cur) = state else { continue };
            for (i, inst) in f.blocks[b].insts.iter().enumerate() {
                for l in uses(inst) {
                    if cur.get(l as usize) == Some(&false) {
                        self.report(
                            Rule::UseBeforeDef,
                            Some(b as BlockId),
                            Some(i),
                            format!("%{l} read before it is written on some path"),
                        );
                    }
                }
                for l in dsts(inst) {
                    if let Some(d) = cur.get_mut(l as usize) {
                        *d = true;
                    }
                }
            }
            for l in term_operands(&f.blocks[b].term)
                .filter_map(local_of)
                .chain(switch_on(&f.blocks[b].term))
            {
                if cur.get(l as usize) == Some(&false) {
                    self.report(
                        Rule::UseBeforeDef,
                        Some(b as BlockId),
                        None,
                        format!("%{l} read before it is written on some path"),
                    );
                }
            }
        }
    }
}

fn local_of(o: &Operand) -> Option<Local> {
    match o {
        Operand::Local(l) => Some(*l),
        _ => None,
    }
}

fn switch_on(t: &Term) -> Option<Local> {
    match t {
        Term::Switch { on, .. } => Some(*on),
        _ => None,
    }
}

fn term_operands(t: &Term) -> Box<dyn Iterator<Item = &Operand> + '_> {
    match t {
        Term::Branch { cond, .. } => Box::new(std::iter::once(cond)),
        Term::Return(v) | Term::TailCall { args: v, .. } => Box::new(v.iter()),
        Term::PanicFields { fields, .. } => Box::new(fields.iter().flat_map(|(a, b)| [a, b])),
        _ => Box::new(std::iter::empty()),
    }
}

fn operands(i: &Inst) -> Vec<&Operand> {
    match i {
        Inst::Copy { src, .. } => vec![src],
        Inst::Scalar { args, .. } | Inst::Call { args, .. } | Inst::Rt { args, .. } => {
            args.iter().collect()
        }
        Inst::CallIndirect { code, args, .. } => std::iter::once(code).chain(args).collect(),
        Inst::Load { ptr, .. } => vec![ptr],
        Inst::Store { ptr, src, .. } => vec![ptr, src],
        Inst::Atomic { ptr, args, .. } => std::iter::once(ptr).chain(args).collect(),
        Inst::Move { places } => places
            .iter()
            .filter_map(|p| match p {
                Loc::Word { ptr, .. } => Some(ptr),
                Loc::Local(_) => None,
            })
            .collect(),
        Inst::Retag { ptr, .. } => vec![ptr],
        Inst::Access { ptr, prov, .. } => vec![ptr, prov],
        Inst::AddrOf { .. } | Inst::Fence { .. } | Inst::Arm(_) => vec![],
    }
}

fn dsts(i: &Inst) -> Vec<Local> {
    match i {
        Inst::Copy { dst, .. } | Inst::Load { dst, .. } | Inst::AddrOf { dst, .. } => vec![*dst],
        Inst::Scalar { dst, .. } | Inst::Rt { dst, .. } | Inst::Atomic { dst, .. } => {
            dst.iter().copied().collect()
        }
        Inst::Call { dst, .. } | Inst::CallIndirect { dst, .. } => dst.clone(),
        Inst::Retag { prov, .. } => vec![*prov],
        Inst::Store { .. }
        | Inst::Fence { .. }
        | Inst::Arm(_)
        | Inst::Move { .. }
        | Inst::Access { .. } => vec![],
    }
}

/// The locals an instruction reads: its local operands, and a moved local.
fn uses(i: &Inst) -> Vec<Local> {
    let mut v: Vec<Local> = operands(i).into_iter().filter_map(local_of).collect();
    if let Inst::Move { places } = i {
        v.extend(places.iter().filter_map(|p| match p {
            Loc::Local(l) => Some(*l),
            Loc::Word { .. } => None,
        }));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(text: &str, form: Form) -> Vec<Rule> {
        let p = Program::parse(text).expect("parses");
        p.validate(form).into_iter().map(|v| v.rule).collect()
    }

    fn body(funcs: &str) -> String {
        format!("mir 1\nmain fn0\nstr 0 \"s\"\n\n{funcs}")
    }

    const MAIN: &str = "fn 0 \"main\"() -> (unit) {\nbb0:\n  ret ()\n}\n";

    #[test]
    fn minimal_program_is_valid() {
        assert!(rules(&body(MAIN), Form::Backend).is_empty());
    }

    #[test]
    fn missing_main_signature_is_an_id_error() {
        let t = body("fn 0 \"main\"(i64) -> (unit) {\nbb0:\n  ret ()\n}\n");
        assert_eq!(rules(&t, Form::Backend), [Rule::Id]);
    }

    #[test]
    fn local_out_of_range_is_an_id_error() {
        let t = body("fn 0 \"main\"() -> (unit) {\nbb0:\n  %9 = copy 1\n  ret ()\n}\n");
        assert!(rules(&t, Form::Backend).contains(&Rule::Id));
    }

    #[test]
    fn string_out_of_range_is_an_id_error() {
        let t = body("fn 0 \"main\"() -> (unit) {\nbb0:\n  rt.print(s7)\n  ret ()\n}\n");
        assert!(rules(&t, Form::Backend).contains(&Rule::Id));
    }

    #[test]
    fn branch_to_a_missing_block_is_a_target_error() {
        let t = body("fn 0 \"main\"() -> (unit) {\nbb0:\n  jump bb4\n}\n");
        assert_eq!(rules(&t, Form::Backend), [Rule::Target]);
    }

    #[test]
    fn switch_target_out_of_range_is_a_target_error() {
        let t = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\nbb0:\n  %0 = copy 0\n  switch %0 [bb0, bb3]\n}\n",
        );
        assert_eq!(rules(&t, Form::Backend), [Rule::Target]);
    }

    #[test]
    fn read_before_write_is_reported_and_a_write_on_every_path_is_not() {
        let bad = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\n  local %1: i64\nbb0:\n  %1 = copy %0\n  ret ()\n}\n",
        );
        assert_eq!(rules(&bad, Form::Backend), [Rule::UseBeforeDef]);
        let one_arm = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\n  local %1: i64\nbb0:\n  br true, bb1, bb2\nbb1:\n  %0 = copy 1\n  jump bb2\nbb2:\n  %1 = copy %0\n  ret ()\n}\n",
        );
        assert_eq!(rules(&one_arm, Form::Backend), [Rule::UseBeforeDef]);
        let both = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\n  local %1: i64\nbb0:\n  br true, bb1, bb2\nbb1:\n  %0 = copy 1\n  jump bb3\nbb2:\n  %0 = copy 2\n  jump bb3\nbb3:\n  %1 = copy %0\n  ret ()\n}\n",
        );
        assert!(rules(&both, Form::Backend).is_empty());
    }

    #[test]
    fn parameters_pinned_and_unit_locals_count_as_written() {
        let t = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\n  local %1: unit\n  pin %0\nbb0:\n  %0 = copy 1\n  jump bb1\nbb1:\n  ret %1\n}\n\nfn 1 \"f\"(i64) -> (i64) {\nbb0:\n  ret %0\n}\n",
        );
        assert!(rules(&t, Form::Backend).is_empty());
    }

    #[test]
    fn wrong_argument_count_is_an_arity_error() {
        let t = body(
            "fn 0 \"main\"() -> (unit) {\nbb0:\n  call fn1(1, 2)\n  ret ()\n}\n\nfn 1 \"f\"(i64) -> () {\nbb0:\n  ret\n}\n",
        );
        assert_eq!(rules(&t, Form::Backend), [Rule::Arity]);
    }

    #[test]
    fn wrong_argument_or_result_type_is_a_type_error() {
        let t = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: bool\n  local %1: i64\nbb0:\n  %0 = copy true\n  %1 = call fn1(%0)\n  ret ()\n}\n\nfn 1 \"f\"(i64) -> (bool) {\nbb0:\n  ret true\n}\n",
        );
        assert_eq!(rules(&t, Form::Backend), [Rule::Type, Rule::Type]);
    }

    #[test]
    fn return_must_match_the_function_results() {
        let t = body("fn 0 \"main\"() -> (unit) {\nbb0:\n  ret 1\n}\n");
        assert_eq!(rules(&t, Form::Backend), [Rule::Type]);
        let t = body("fn 0 \"main\"() -> (unit) {\nbb0:\n  ret (), ()\n}\n");
        assert_eq!(rules(&t, Form::Backend), [Rule::Arity]);
    }

    #[test]
    fn pre_transform_terminators_only_pass_in_the_lowered_form() {
        let t = body("fn 0 \"main\"() -> (unit) {\nbb0:\n  task_end\n}\n");
        assert!(rules(&t, Form::Lowered).is_empty());
        assert_eq!(rules(&t, Form::Backend), [Rule::PreTransform]);
    }

    #[test]
    fn addr_of_needs_a_pinned_local_and_groups_are_disjoint() {
        let t = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\n  local %1: ptr\nbb0:\n  %0 = copy 1\n  %1 = addr %0\n  ret ()\n}\n",
        );
        assert_eq!(rules(&t, Form::Backend), [Rule::Pinned]);
        let t = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\n  pin %0\n  pin %0\nbb0:\n  ret ()\n}\n",
        );
        assert_eq!(rules(&t, Form::Backend), [Rule::Pinned]);
    }

    #[test]
    fn violations_name_the_code_and_place() {
        let t = body(
            "fn 0 \"main\"() -> (unit) {\n  local %0: i64\nbb0:\n  %0 = copy %0\n  ret ()\n}\n",
        );
        let v = Program::parse(&t).expect("parses").validate(Form::Backend);
        assert_eq!(v.len(), 1);
        assert_eq!(
            v[0].to_string(),
            "I103: fn \"main\" bb0 inst 0: %0 read before it is written on some path"
        );
    }
}
