//! The reader of the canonical MIR text (RFC 0097 D1, D2, D3): the inverse of `text.rs`, a
//! hand-written recursive descent over lines. It checks the grammar and that ids are dense and in
//! order; whether the program is well-formed (ranges, types, paths) is the validator's job.

use std::collections::HashSet;
use std::fmt::{self, Display, Formatter};
use std::sync::{Mutex, OnceLock};

use crate::{
    ArmEvent, AtomicOp, Block, Func, Inst, IntTy, Loc, MemOrder, Operand, Program, RetagKind, RtFn,
    ScalarOp, Term, Ty, text::VERSION,
};

/// A panic kind as `&'static str`, which is what `Term::Panic` holds. Kinds are an open set
/// (some are sentences), so each distinct one is leaked once and reused.
fn intern(kind: &str) -> &'static str {
    static KINDS: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let mut kinds = KINDS
        .get_or_init(Default::default)
        .lock()
        .expect("not poisoned");
    if let Some(k) = kinds.get(kind) {
        return k;
    }
    let k: &'static str = Box::leak(kind.into());
    kinds.insert(k);
    k
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line of the text.
    pub line: usize,
    pub message: String,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "mir text line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

type Res<T> = Result<T, String>;

/// One line being read.
struct Cur<'a> {
    s: &'a str,
}

impl<'a> Cur<'a> {
    fn ws(&mut self) {
        self.s = self.s.trim_start_matches([' ', '\t']);
    }

    fn eat(&mut self, lit: &str) -> bool {
        match self.s.strip_prefix(lit) {
            Some(rest) => {
                self.s = rest;
                true
            }
            None => false,
        }
    }

    fn expect(&mut self, lit: &str) -> Res<()> {
        if self.eat(lit) {
            Ok(())
        } else {
            Err(format!("expected `{lit}`, found `{}`", self.s))
        }
    }

    fn end(&mut self) -> Res<()> {
        self.ws();
        if self.s.is_empty() {
            Ok(())
        } else {
            Err(format!("unexpected `{}`", self.s))
        }
    }

    /// A mnemonic: letters, digits, `_` and `.`.
    fn word(&mut self) -> &'a str {
        let n = self
            .s
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(self.s.len());
        let (w, rest) = self.s.split_at(n);
        self.s = rest;
        w
    }

    fn num<T: std::str::FromStr>(&mut self) -> Res<T> {
        let n = self
            .s
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(self.s.len());
        let (d, rest) = self.s.split_at(n);
        let v = d
            .parse()
            .map_err(|_| format!("expected a number, found `{}`", self.s))?;
        self.s = rest;
        Ok(v)
    }

    fn prefixed(&mut self, prefix: &str) -> Res<u32> {
        self.expect(prefix)?;
        self.num()
    }

    fn local(&mut self) -> Res<u32> {
        self.prefixed("%")
    }

    fn block(&mut self) -> Res<u32> {
        self.prefixed("bb")
    }

    fn func(&mut self) -> Res<u32> {
        self.prefixed("fn")
    }

    /// `item, item, ...` up to (not including) `close`, which may be empty.
    fn seq<T>(&mut self, close: char, mut item: impl FnMut(&mut Self) -> Res<T>) -> Res<Vec<T>> {
        let mut out = Vec::new();
        self.ws();
        if self.s.is_empty() || self.s.starts_with([close, '@']) {
            return Ok(out);
        }
        loop {
            out.push(item(self)?);
            self.ws();
            if !self.eat(",") {
                return Ok(out);
            }
            self.ws();
        }
    }

    fn quoted(&mut self) -> Res<String> {
        self.expect("\"")?;
        let mut out = String::new();
        let mut chars = self.s.char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => {
                    self.s = &self.s[i + 1..];
                    return Ok(out);
                }
                '\\' => match chars.next().map(|(_, c)| c) {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('u') => {
                        if chars.next().map(|(_, c)| c) != Some('{') {
                            return Err("expected `{` after `\\u`".into());
                        }
                        let mut hex = String::new();
                        loop {
                            match chars.next().map(|(_, c)| c) {
                                Some('}') => break,
                                Some(c) if c.is_ascii_hexdigit() => hex.push(c),
                                _ => return Err("bad `\\u{..}` escape".into()),
                            }
                        }
                        let c = u32::from_str_radix(&hex, 16)
                            .ok()
                            .and_then(char::from_u32)
                            .ok_or("bad `\\u{..}` escape")?;
                        out.push(c);
                    }
                    _ => return Err("unknown escape".into()),
                },
                c => out.push(c),
            }
        }
        Err("unterminated string".into())
    }

    fn int_ty(&mut self, word: &str) -> Res<IntTy> {
        let signed = match word.as_bytes().first() {
            Some(b'i') => true,
            Some(b'u') => false,
            _ => return Err(format!("expected a type, found `{word}`")),
        };
        match word[1..].parse::<u8>() {
            Ok(bits) if (1..=64).contains(&bits) => Ok(IntTy { bits, signed }),
            _ => Err(format!("expected a type, found `{word}`")),
        }
    }

    fn ty_of(&mut self, word: &str) -> Res<Ty> {
        Ok(match word {
            "f64" => Ty::F64,
            "bool" => Ty::Bool,
            "unit" => Ty::Unit,
            "ptr" => Ty::Ptr,
            w => Ty::Int(self.int_ty(w)?),
        })
    }

    fn ty(&mut self) -> Res<Ty> {
        let w = self.word();
        self.ty_of(w)
    }

    /// `(ty, ty, ...)`
    fn tys(&mut self) -> Res<Vec<Ty>> {
        self.expect("(")?;
        let v = self.seq(')', |c| c.ty())?;
        self.expect(")")?;
        Ok(v)
    }

    fn operand(&mut self) -> Res<Operand> {
        if self.eat("()") {
            return Ok(Operand::Unit);
        }
        let mut n = 0;
        for (i, c) in self.s.char_indices() {
            let stop = matches!(c, ',' | ')' | ']' | '(' | '[' | ' ' | '\t')
                || (c == ':' && !self.s.starts_with("nan"));
            if stop {
                break;
            }
            n = i + c.len_utf8();
        }
        let (tok, rest) = self.s.split_at(n);
        self.s = rest;
        operand_of(tok).ok_or_else(|| format!("expected an operand, found `{tok}`"))
    }

    /// `(operand, ...)`
    fn args(&mut self) -> Res<Vec<Operand>> {
        self.expect("(")?;
        let v = self.seq(')', |c| c.operand())?;
        self.expect(")")?;
        Ok(v)
    }

    fn loc(&mut self) -> Res<Loc> {
        let o = self.operand()?;
        if self.eat("[") {
            let word = self.num()?;
            self.expect("]")?;
            return Ok(Loc::Word { ptr: o, word });
        }
        match o {
            Operand::Local(l) => Ok(Loc::Local(l)),
            o => Err(format!("expected a place, found `{o}`")),
        }
    }

    /// `operand[word]`
    fn word_ref(&mut self) -> Res<(Operand, u32)> {
        let ptr = self.operand()?;
        self.expect("[")?;
        let word = self.num()?;
        self.expect("]")?;
        Ok((ptr, word))
    }

    fn blocks(&mut self) -> Res<Vec<u32>> {
        self.expect("[")?;
        let v = self.seq(']', |c| c.block())?;
        self.expect("]")?;
        Ok(v)
    }

    fn order(&mut self) -> Res<MemOrder> {
        self.ws();
        let w = self.word();
        MemOrder::from_mnemonic(w).ok_or_else(|| format!("unknown memory order `{w}`"))
    }
}

fn operand_of(tok: &str) -> Option<Operand> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let id = |rest: &str| digits(rest).then(|| rest.parse().ok()).flatten();
    match tok {
        "true" => return Some(Operand::Bool(true)),
        "false" => return Some(Operand::Bool(false)),
        "inf" => return Some(Operand::F64(f64::INFINITY)),
        "-inf" => return Some(Operand::F64(f64::NEG_INFINITY)),
        _ => {}
    }
    if let Some(bits) = tok.strip_prefix("nan:0x") {
        let bits = u64::from_str_radix(bits, 16).ok()?;
        let x = f64::from_bits(bits);
        return x.is_nan().then_some(Operand::F64(x));
    }
    if let Some(r) = tok.strip_prefix('%') {
        return id(r).map(Operand::Local);
    }
    if let Some(r) = tok.strip_prefix("fn") {
        return id(r).map(Operand::Func);
    }
    if let Some(r) = tok.strip_prefix('s') {
        return id(r).map(Operand::Str);
    }
    let body = tok.strip_prefix('-').unwrap_or(tok);
    if digits(body) {
        return tok.parse().ok().map(Operand::Int);
    }
    // The printer writes every float with a `.` or an exponent, so these never read as integers.
    let float = body.starts_with(|c: char| c.is_ascii_digit()) && body.contains(['.', 'e']);
    float.then(|| tok.parse().ok().map(Operand::F64)).flatten()
}

fn scalar_op(c: &mut Cur, word: &str) -> Res<(Ty, ScalarOp)> {
    let (ty, op) = word
        .split_once('.')
        .ok_or_else(|| format!("unknown instruction `{word}`"))?;
    let ty = c.ty_of(ty)?;
    let (name, conv) = match op.split_once('.') {
        Some((n, t)) => (n, Some(t)),
        None => (op, None),
    };
    let op = match (name, conv) {
        ("wrap_as", Some(t)) => ScalarOp::WrapAs(c.int_ty(t)?),
        ("sat_as", Some(t)) => ScalarOp::SatAs(c.int_ty(t)?),
        ("checked_as", Some(t)) => ScalarOp::CheckedAs(c.int_ty(t)?),
        (n, None) => {
            ScalarOp::from_mnemonic(n).ok_or_else(|| format!("unknown operation `{n}`"))?
        }
        _ => return Err(format!("unknown instruction `{word}`")),
    };
    Ok((ty, op))
}

/// The instruction on a line, without its trailing `@line`.
fn inst(c: &mut Cur) -> Res<Inst> {
    let dsts = if c.s.starts_with('%') {
        let d = c.seq('=', |c| c.local())?;
        c.ws();
        c.expect("=")?;
        c.ws();
        d
    } else {
        Vec::new()
    };
    let word = c.word();
    let at_most_one = |d: &[u32]| match d {
        [] => Ok(None),
        [d] => Ok(Some(*d)),
        _ => Err(format!("`{word}` has one result")),
    };
    let exactly_one = |d: &[u32]| match d {
        [d] => Ok(*d),
        _ => Err(format!("`{word}` has one result")),
    };
    let none = |d: &[u32]| {
        if d.is_empty() {
            Ok(())
        } else {
            Err(format!("`{word}` has no result"))
        }
    };
    let i = match word {
        "copy" => {
            c.ws();
            Inst::Copy {
                dst: exactly_one(&dsts)?,
                src: c.operand()?,
            }
        }
        "call" => {
            c.ws();
            let func = c.func()?;
            Inst::Call {
                dst: dsts,
                func,
                args: c.args()?,
            }
        }
        "call_indirect" => {
            c.ws();
            let code = c.operand()?;
            let args = c.args()?;
            c.ws();
            c.expect(":")?;
            c.ws();
            let params = c.tys()?;
            c.ws();
            c.expect("->")?;
            c.ws();
            Inst::CallIndirect {
                dst: dsts,
                code,
                args,
                params,
                ret: c.tys()?,
            }
        }
        "load" => {
            c.ws();
            let (ptr, word) = c.word_ref()?;
            Inst::Load {
                dst: exactly_one(&dsts)?,
                ptr,
                word,
            }
        }
        "store" => {
            none(&dsts)?;
            c.ws();
            let (ptr, word) = c.word_ref()?;
            c.ws();
            c.expect(",")?;
            c.ws();
            Inst::Store {
                ptr,
                word,
                src: c.operand()?,
            }
        }
        "addr" => {
            c.ws();
            Inst::AddrOf {
                dst: exactly_one(&dsts)?,
                local: c.local()?,
            }
        }
        "fence" => {
            none(&dsts)?;
            Inst::Fence { order: c.order()? }
        }
        "arm.fork" | "arm.exit" | "arm.join" | "arm.leave" | "arm.enter" => {
            none(&dsts)?;
            Inst::Arm(match word {
                "arm.fork" => ArmEvent::Fork,
                "arm.exit" => ArmEvent::Exit,
                "arm.join" => ArmEvent::Join,
                "arm.leave" => ArmEvent::Leave,
                _ => {
                    c.ws();
                    ArmEvent::Enter(c.num()?)
                }
            })
        }
        w if w.starts_with("retag.") => {
            none(&dsts)?;
            let mut parts = w["retag.".len()..].split('.');
            let kind = match parts.next() {
                Some("unique") => RetagKind::Unique,
                Some("frozen") => RetagKind::Frozen,
                Some("cell") => RetagKind::Cell,
                k => return Err(format!("unknown retag kind `{}`", k.unwrap_or(""))),
            };
            let protected = match parts.next() {
                None => false,
                Some("protected") => true,
                Some(m) => return Err(format!("unknown retag modifier `{m}`")),
            };
            c.ws();
            let ptr = c.operand()?;
            c.ws();
            c.expect(",")?;
            c.ws();
            let prov = c.local()?;
            c.ws();
            c.expect(",")?;
            c.ws();
            Inst::Retag {
                ptr,
                prov,
                bytes: c.num()?,
                kind,
                protected,
            }
        }
        w if w.starts_with("access.") => {
            none(&dsts)?;
            let write = match &w["access.".len()..] {
                "r" => false,
                "w" => true,
                m => return Err(format!("unknown access mode `{m}`")),
            };
            c.ws();
            let ptr = c.operand()?;
            c.ws();
            c.expect(",")?;
            c.ws();
            let prov = c.operand()?;
            c.ws();
            c.expect(",")?;
            c.ws();
            Inst::Access {
                ptr,
                prov,
                bytes: c.num()?,
                write,
            }
        }
        "move" => {
            none(&dsts)?;
            c.ws();
            Inst::Move {
                places: c.seq('\0', |c| c.loc())?,
            }
        }
        w if w.starts_with("rt.") => {
            let name = &w[3..];
            let f = RtFn::from_mnemonic(name)
                .ok_or_else(|| format!("unknown runtime function `{name}`"))?;
            Inst::Rt {
                dst: at_most_one(&dsts)?,
                f,
                args: c.args()?,
            }
        }
        w if w.starts_with("atomic.") => {
            let mut parts = w["atomic.".len()..].splitn(2, '.');
            let (op, ty) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let op = AtomicOp::from_mnemonic(op)
                .ok_or_else(|| format!("unknown atomic operation `{op}`"))?;
            let ty = c.ty_of(ty)?;
            let order = c.order()?;
            let failure = c.order()?;
            c.ws();
            let ptr = c.operand()?;
            let mut args = Vec::new();
            loop {
                c.ws();
                if !c.eat(",") {
                    break;
                }
                c.ws();
                args.push(c.operand()?);
            }
            Inst::Atomic {
                dst: at_most_one(&dsts)?,
                op,
                ty,
                order,
                failure,
                ptr,
                args,
            }
        }
        w => {
            let (ty, op) = scalar_op(c, w)?;
            c.ws();
            let args = c.seq('\0', |c| c.operand())?;
            Inst::Scalar {
                dst: at_most_one(&dsts)?,
                op,
                ty,
                args,
            }
        }
    };
    Ok(i)
}

const TERMS: &[&str] = &[
    "jump", "br", "switch", "ret", "tailcall", "panic", "par", "suspend", "task_end", "unwind",
];

fn term(c: &mut Cur) -> Res<Term> {
    let word = c.word();
    c.ws();
    let t = match word {
        "jump" => Term::Jump(c.block()?),
        "br" => {
            let cond = c.operand()?;
            c.ws();
            c.expect(",")?;
            c.ws();
            let then = c.block()?;
            c.ws();
            c.expect(",")?;
            c.ws();
            Term::Branch {
                cond,
                then,
                els: c.block()?,
            }
        }
        "switch" => {
            let on = c.local()?;
            c.ws();
            Term::Switch {
                on,
                targets: c.blocks()?,
            }
        }
        "ret" => Term::Return(c.seq('\0', |c| c.operand())?),
        "tailcall" => {
            let func = c.func()?;
            Term::TailCall {
                func,
                args: c.args()?,
            }
        }
        "panic" => {
            let kind = c.quoted()?;
            let kind = intern(&kind);
            c.ws();
            if c.eat("(") {
                let fields = c.seq(')', |c| {
                    let name = c.operand()?;
                    c.expect(":")?;
                    c.ws();
                    Ok((name, c.operand()?))
                })?;
                c.expect(")")?;
                Term::PanicFields { kind, fields }
            } else {
                Term::Panic(kind)
            }
        }
        "par" => {
            let tasks = c.blocks()?;
            c.ws();
            c.expect("join")?;
            c.ws();
            Term::Par {
                tasks,
                join: c.block()?,
            }
        }
        "suspend" => Term::Suspend { resume: c.block()? },
        "task_end" => Term::TaskEnd,
        "unwind" => Term::Unwind { drops: c.block()? },
        w => return Err(format!("unknown terminator `{w}`")),
    };
    Ok(t)
}

struct Lines<'a> {
    lines: Vec<&'a str>,
    at: usize,
}

impl<'a> Lines<'a> {
    fn peek(&self) -> Option<&'a str> {
        self.lines.get(self.at).copied()
    }

    /// The 1-based number of the line `peek` returns.
    fn line(&self) -> usize {
        self.at + 1
    }

    fn next(&mut self) -> Option<&'a str> {
        let l = self.peek();
        self.at += 1;
        l
    }

    fn skip_blank(&mut self) {
        while self.peek().is_some_and(|l| l.trim().is_empty()) {
            self.at += 1;
        }
    }
}

struct Reader<'a> {
    lines: Lines<'a>,
}

impl<'a> Reader<'a> {
    /// Parse the current line with `f`, which must consume all of it; errors carry its number.
    fn on_line<T>(&mut self, f: impl FnOnce(&mut Cur) -> Res<T>) -> Result<T, ParseError> {
        let line = self.lines.line();
        let text = self.lines.next().ok_or(ParseError {
            line,
            message: "unexpected end of text".into(),
        })?;
        let mut c = Cur { s: text };
        f(&mut c)
            .and_then(|v| c.end().map(|()| v))
            .map_err(|message| ParseError { line, message })
    }

    fn program(&mut self) -> Result<Program, ParseError> {
        let version = self.on_line(|c| {
            c.expect("mir ")?;
            c.num::<u32>()
        })?;
        if version != VERSION {
            return Err(ParseError {
                line: 1,
                message: format!("unsupported mir version {version}, this reader reads {VERSION}"),
            });
        }
        let main = self.on_line(|c| {
            c.expect("main ")?;
            c.func()
        })?;
        let mut strings = Vec::new();
        while self.lines.peek().is_some_and(|l| l.starts_with("str ")) {
            let i = strings.len();
            let s = self.on_line(|c| {
                c.expect("str ")?;
                if c.num::<usize>()? != i {
                    return Err(format!("string ids must be dense: expected {i}"));
                }
                c.expect(" ")?;
                c.quoted()
            })?;
            strings.push(s);
        }
        let mut funcs = Vec::new();
        loop {
            self.lines.skip_blank();
            if self.lines.peek().is_none() {
                break;
            }
            funcs.push(self.func(funcs.len())?);
        }
        Ok(Program {
            funcs,
            main,
            strings,
        })
    }

    fn func(&mut self, id: usize) -> Result<Func, ParseError> {
        let (unsafe_fn, name, param_tys, ret) = self.on_line(|c| {
            let unsafe_fn = c.eat("unsafe ");
            c.expect("fn ")?;
            if c.num::<usize>()? != id {
                return Err(format!("function ids must be dense: expected {id}"));
            }
            c.expect(" ")?;
            let name = c.quoted()?;
            let params = c.tys()?;
            c.expect(" -> ")?;
            let ret = c.tys()?;
            c.expect(" {")?;
            Ok((unsafe_fn, name, params, ret))
        })?;
        let params = param_tys.len() as u32;
        let mut locals = param_tys;
        let mut pinned = Vec::new();
        while let Some(l) = self.lines.peek() {
            if l.starts_with("  local ") {
                let n = locals.len();
                let t = self.on_line(|c| {
                    c.expect("  local ")?;
                    if c.local()? as usize != n {
                        return Err(format!("local ids must be dense: expected %{n}"));
                    }
                    c.expect(": ")?;
                    c.ty()
                })?;
                locals.push(t);
            } else if l.starts_with("  pin ") {
                pinned.push(self.on_line(|c| {
                    c.expect("  pin ")?;
                    c.seq('\0', |c| c.local())
                })?);
            } else {
                break;
            }
        }
        let mut blocks = Vec::new();
        while self.lines.peek().is_some_and(|l| l.starts_with("bb")) {
            blocks.push(self.block(blocks.len())?);
        }
        self.on_line(|c| c.expect("}"))?;
        Ok(Func {
            name,
            unsafe_fn,
            params,
            ret,
            locals,
            pinned,
            blocks,
        })
    }

    fn block(&mut self, id: usize) -> Result<Block, ParseError> {
        self.on_line(|c| {
            if c.block()? as usize != id {
                return Err(format!("block ids must be dense: expected bb{id}"));
            }
            c.expect(":")
        })?;
        let (mut insts, mut lines) = (Vec::new(), Vec::new());
        loop {
            let line = self.lines.line();
            let Some(text) = self.lines.peek() else {
                return Err(ParseError {
                    line,
                    message: format!("bb{id} has no terminator"),
                });
            };
            let body = text.trim_start();
            let head = body.split([' ', '(']).next().unwrap_or("");
            if !text.starts_with("  ") || body.is_empty() {
                return Err(ParseError {
                    line,
                    message: format!("bb{id} has no terminator"),
                });
            }
            if TERMS.contains(&head) {
                let term = self.on_line(|c| {
                    c.ws();
                    term(c)
                })?;
                return Ok(Block { insts, lines, term });
            }
            let (i, l) = self.on_line(|c| {
                c.ws();
                let i = inst(c)?;
                c.ws();
                let l = if c.eat("@") { c.num()? } else { 0 };
                Ok((i, l))
            })?;
            insts.push(i);
            lines.push(l);
        }
    }
}

impl Program {
    /// Read the canonical text of a program (RFC 0097). A header of another version is refused.
    pub fn parse(text: &str) -> Result<Program, ParseError> {
        let mut r = Reader {
            lines: Lines {
                lines: text.lines().collect(),
                at: 0,
            },
        };
        r.program()
    }
}

impl std::str::FromStr for Program {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Program, ParseError> {
        Program::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(text: &str) -> ParseError {
        Program::parse(text).expect_err("rejected")
    }

    #[test]
    fn golden_round_trips() {
        let text = crate::text::tests::GOLDEN;
        let p = Program::parse(text).expect("parses");
        assert_eq!(p, crate::text::tests::sample());
        assert_eq!(p.to_string(), text);
    }

    #[test]
    fn other_version_is_refused() {
        let e = err("mir 2\nmain fn0\n");
        assert_eq!(e.line, 1);
        assert!(e.message.contains("version 2"));
    }

    #[test]
    fn ids_must_be_dense() {
        let head = "mir 1\nmain fn0\n";
        assert!(
            err(&format!("{head}\nfn 1 \"a\"() -> () {{\n}}\n"))
                .message
                .contains("dense")
        );
        let f = "\nfn 0 \"a\"() -> () {\n  local %1: i64\n";
        assert!(
            err(&format!("{head}{f}bb0:\n  ret\n}}\n"))
                .message
                .contains("%0")
        );
        let f = "\nfn 0 \"a\"() -> () {\nbb1:\n  ret\n}\n";
        assert!(err(&format!("{head}{f}")).message.contains("bb0"));
        assert!(
            err("mir 1\nmain fn0\nstr 1 \"x\"\n")
                .message
                .contains("dense")
        );
    }

    #[test]
    fn a_block_needs_a_terminator() {
        let head = "mir 1\nmain fn0\n\nfn 0 \"a\"() -> () {\nbb0:\n";
        assert!(err(&format!("{head}}}\n")).message.contains("terminator"));
        assert!(
            err(&format!("{head}  jump bb0\n  ret\n}}\n"))
                .message
                .contains("`}`")
        );
    }

    #[test]
    fn rejects_unknown_names_and_bad_operands() {
        let wrap = |body: &str| {
            format!(
                "mir 1\nmain fn0\n\nfn 0 \"a\"() -> () {{\n  local %0: i64\nbb0:\n  {body}\n  ret\n}}\n"
            )
        };
        assert!(err(&wrap("rt.nope()")).message.contains("nope"));
        assert!(err(&wrap("%0 = i64.nope 1")).message.contains("nope"));
        assert!(err(&wrap("%0 = i7x.add 1")).message.contains("i7x"));
        assert!(err(&wrap("%0 = copy 1.2.3")).message.contains("1.2.3"));
        assert!(err(&wrap("%0 = copy nan")).message.contains("nan"));
        assert!(err(&wrap("%0, %1 = copy 1")).message.contains("one result"));
        assert!(err(&wrap("copy 1")).message.contains("one result"));
        assert!(err(&wrap("%0 = copy 1 junk")).message.contains("junk"));
    }

    #[test]
    fn float_and_integer_forms_stay_apart() {
        for (text, want) in [
            ("1", Operand::Int(1)),
            ("-7", Operand::Int(-7)),
            ("1.0", Operand::F64(1.0)),
            ("1e300", Operand::F64(1e300)),
            ("1.5e-7", Operand::F64(1.5e-7)),
            ("inf", Operand::F64(f64::INFINITY)),
            ("true", Operand::Bool(true)),
            ("s3", Operand::Str(3)),
            ("fn2", Operand::Func(2)),
            ("%9", Operand::Local(9)),
        ] {
            assert_eq!(operand_of(text), Some(want), "{text}");
        }
        let Some(Operand::F64(z)) = operand_of("-0.0") else {
            panic!("float")
        };
        assert!(z == 0.0 && z.is_sign_negative());
        let Some(Operand::F64(n)) = operand_of("nan:0x7ff8000000000001") else {
            panic!("nan")
        };
        assert_eq!(n.to_bits(), 0x7ff8000000000001);
    }

    #[test]
    fn task_terminators_and_text_escapes() {
        for t in [
            "par [bb1, bb2] join bb3",
            "suspend bb4",
            "task_end",
            "unwind bb5",
            "switch %2 [bb1, bb3]",
            "tailcall fn2()",
            "tailcall fn2(())",
            "ret",
            "ret (), 1.5",
            "panic \"value\" (s0: %1, s1: nan:0x7ff8000000000000)",
        ] {
            let mut c = Cur { s: t };
            let parsed = term(&mut c).expect(t);
            assert_eq!(parsed.to_string(), t);
        }
        let mut c = Cur {
            s: r#""a\"b\\c\n\u{1}é" rest"#,
        };
        assert_eq!(c.quoted().unwrap(), "a\"b\\c\n\u{1}é");
    }

    #[test]
    fn lowered_programs_reach_a_fixed_point() {
        let src = "module t\nfn main() -> Unit {\n var n: i64 = 0\n\
                   loop 'l {\n n = add_checked_i64(let n, let 1)\n let big: bool = gt_i64(let n, let 2)\n\
                   if big { break 'l }\n }\n assert eq_i64(let n, let 3)\n ret ()\n}\n";
        let (m, d) = yan_core::parse("t.yir", src);
        assert!(d.is_empty());
        let p = crate::lower(&m.unwrap()).expect("lowers");
        let text = p.to_string();
        let back = Program::parse(&text).expect("parses");
        assert_eq!(back, p);
        assert_eq!(back.to_string(), text);
    }

    /// Every Yan Core conformance program that lowers prints, parses and prints identically.
    #[test]
    fn conformance_corpus_reaches_a_fixed_point() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/yir");
        let mut lowered = 0;
        for dir in ["run", "pass", "opt"] {
            let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                if path.extension().is_none_or(|x| x != "yir") {
                    continue;
                }
                let src = std::fs::read_to_string(&path).expect("readable");
                let (Some(m), d) = yan_core::parse(&path.display().to_string(), &src) else {
                    continue;
                };
                if !d.is_empty() {
                    continue;
                }
                let Ok(p) = crate::lower(&m) else { continue };
                let text = p.to_string();
                let back =
                    Program::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert_eq!(back, p, "{}", path.display());
                assert_eq!(back.to_string(), text, "{}", path.display());
                lowered += 1;
            }
        }
        assert!(lowered > 10, "only {lowered} programs lowered");
    }

    /// The printed output of every `view` conformance case is its own fixed point.
    #[test]
    fn view_cases_reach_a_fixed_point() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance/view");
        let mut seen = 0;
        for e in std::fs::read_dir(root).expect("view cases").flatten() {
            let path = e.path().join("out.mir");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let p = Program::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(p.to_string(), text, "{}", path.display());
            seen += 1;
        }
        assert!(seen >= 2, "only {seen} view cases found");
    }
}
