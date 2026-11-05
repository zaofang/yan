//! Yan IR as written in `.yir` text (spec 3.5).
//!
//! Built-in type constructors (`List`, `Map`, `Array`, scalars, ...) are ordinary named types
//! here; the validator gives them meaning.

use yan_diag::Span;

pub type Name = String;
pub type Path = Vec<Name>;

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub header: Vec<String>,
    pub path: Path,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub attrs: Vec<Attr>,
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    pub name: Name,
    pub args: Option<Vec<AttrArg>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AttrArg {
    pub name: Option<Name>,
    pub value: ConstVal,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Fn(FnDef),
    Gen(FnDef),
    /// Optimized Yan Core only (R772).
    Inst(Inst),
    Record(Record),
    Sum(Sum),
    Newtype(Newtype),
    Trait(Trait),
    Impl(Impl),
    Const(Const),
    Test(Test),
    Extern(Extern),
}

/// A function, generator or trait signature. Signatures have a one-name path and may lack a body.
#[derive(Clone, Debug, PartialEq)]
pub struct FnDef {
    pub is_pub: bool,
    pub is_unsafe: bool,
    pub path: Path,
    /// The first token of `path`.
    pub name_span: Span,
    pub tparams: Vec<TParam>,
    pub params: Vec<Param>,
    pub ret: Type,
    pub from: Vec<Name>,
    pub fail: Option<Type>,
    pub contracts: Vec<Contract>,
    pub body: Option<Block>,
}

/// `inst fn id[i64](...)`: a generic `fn` specialized to concrete arguments. `def` has no type
/// parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Inst {
    pub args: Vec<TArg>,
    pub def: FnDef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conv {
    Let,
    Inout,
    Sink,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Value {
        comptime: bool,
        conv: Conv,
        name: Name,
        ty: Type,
    },
    Cap {
        name: Name,
        ty: Named,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Contract {
    Requires(Expr),
    Ensures(Expr),
    Decreases(Vec<Expr>),
    Example(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub is_pub: bool,
    pub path: Path,
    pub tparams: Vec<TParam>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub is_pub: bool,
    pub name: Name,
    pub ty: Type,
    pub default: Option<ConstVal>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sum {
    pub is_pub: bool,
    pub path: Path,
    pub tparams: Vec<TParam>,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub name: Name,
    pub fields: Option<Vec<Field>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Newtype {
    pub is_pub: bool,
    pub path: Path,
    pub tparams: Vec<TParam>,
    pub ty: Type,
    pub pred: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trait {
    pub is_pub: bool,
    pub path: Path,
    pub tparams: Vec<TParam>,
    pub assoc: Vec<Name>,
    pub sigs: Vec<FnDef>,
    pub derived: Option<Block>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Impl {
    pub is_unsafe: bool,
    pub trait_: Named,
    pub for_ty: Type,
    pub assoc: Vec<(Name, Type)>,
    pub fns: Vec<FnDef>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Const {
    pub is_pub: bool,
    pub path: Path,
    pub ty: Type,
    pub value: ConstVal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Test {
    pub path: Path,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Extern {
    pub abi: String,
    pub lib: Name,
    pub link: Option<String>,
    pub items: Vec<XItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum XItem {
    Fn {
        safe: bool,
        name: Name,
        params: Vec<XParam>,
        variadic: bool,
        ret: Type,
        from: Vec<Name>,
        uses_none: bool,
        nonblocking: bool,
    },
    Type {
        name: Name,
        drop: Path,
    },
    Static {
        safe: bool,
        name: Name,
        ty: Type,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum XParam {
    Value {
        name: Name,
        ty: Type,
    },
    Callback {
        name: Name,
        escaping: bool,
        ty: FnType,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TParam {
    pub comptime: bool,
    pub name: Name,
    pub kind: Kind,
    pub bounds: Vec<Path>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Type,
    Nat,
    Dim,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TArg {
    Type(Type),
    Nat(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    pub path: Path,
    pub args: Vec<TArg>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    Named(Named),
    Tuple(Vec<Type>),
    Dyn(Named),
    Fn(FnType),
    Refined {
        name: Name,
        ty: Box<Type>,
        pred: Expr,
    },
    /// Anonymous error union; only after `fail`.
    Union(Vec<Type>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct FnType {
    pub params: Vec<(Conv, Type)>,
    pub ret: Box<Type>,
    pub uses: Option<CapSet>,
    pub fail: Option<Box<Type>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CapSet {
    None,
    Names(Vec<Name>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Let {
        mutable: bool,
        pat: Pat,
        pat_span: Span,
        ty: Type,
        rhs: Rhs,
        rhs_span: Span,
    },
    Assign {
        place: Place,
        rhs: Rhs,
        rhs_span: Span,
    },
    If {
        cond: Atom,
        then: Block,
        els: Option<Block>,
    },
    Match {
        scrutinee: Atom,
        arms: Vec<(Pat, Block)>,
    },
    Loop {
        label: Name,
        invariants: Vec<Expr>,
        decreases: Vec<Expr>,
        body: Block,
    },
    Break {
        label: Name,
        value: Option<Atom>,
    },
    Continue {
        label: Name,
    },
    Jp {
        name: Name,
        params: Vec<Param>,
        body: Block,
    },
    Jump {
        name: Name,
        args: Vec<Atom>,
    },
    Ret(Atom),
    Fail(Atom),
    Yield(Atom),
    Become(Call),
    With {
        name: Name,
        dyn_: bool,
        handler: Atom,
        body: Block,
    },
    /// A cancel scope: `at` is a `MonoInstant`; blocking operations in `body` fail with `Timeout`
    /// once it passes, and the earliest enclosing deadline wins (9.4).
    Deadline {
        at: Atom,
        body: Block,
    },
    Par {
        concurrent: bool,
        blocks: Vec<Block>,
    },
    ComptimeFor {
        name: Name,
        ty: Type,
        iter: Atom,
        body: Block,
    },
    ComptimeIf {
        cond: Atom,
        then: Block,
        els: Option<Block>,
    },
    Assert(Expr),
    Unreachable,
    Unsafe(Block),
    Asm {
        template: String,
        operands: Vec<AsmOperand>,
    },
    Drop(Name),
    Reuse {
        name: Name,
        ctor: Ctor,
    },
    Rhs(Rhs),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rhs {
    Atom(Atom),
    Call(Call),
    /// `Call?`, optionally `via` a `From` conversion. `drops` are dropped on the failing edge
    /// before the function returns; only optimized Yan Core has any.
    Propagate {
        call: Call,
        via: Option<Path>,
        drops: Vec<Name>,
    },
    Try(Call),
    Ctor(Ctor),
    Proj(Place),
    Copy(Place),
    Move(Place),
    Closure(Box<Closure>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub path: Path,
    pub span: Span,
    pub targs: Vec<TArg>,
    pub args: Vec<Arg>,
}

#[derive(Clone, Debug, PartialEq)]
/// The span is the argument's first token.
pub enum Arg {
    Value(Conv, Atom, Span),
    Cap(Name, Span),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ctor {
    Named {
        path: Path,
        targs: Vec<TArg>,
        fields: Vec<(Name, Atom)>,
    },
    Tuple(Vec<Atom>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub root: Name,
    pub root_span: Span,
    pub projs: Vec<Proj>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Proj {
    Field(Name, Span),
    Index(Atom),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Closure {
    pub captures: Vec<(Conv, Name, Span)>,
    pub params: Vec<Param>,
    pub ret: Type,
    pub uses: Option<CapSet>,
    pub fail: Option<Type>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pat {
    Wild,
    Lit(Literal),
    Bind(Name),
    /// `rest` is the trailing `..` of a record pattern.
    Ctor {
        path: Path,
        fields: Option<(Vec<(Name, Pat)>, bool)>,
    },
    Tuple(Vec<Pat>),
    Or(Vec<Pat>),
    Range(Literal, Literal),
    /// `rest` is `Some(binding)` when the pattern ends in `..` or `.. name`.
    Slice {
        elems: Vec<Pat>,
        rest: Option<Option<Name>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Atom {
    Name(Name, Span),
    Lit(Literal, Span),
}

impl Atom {
    pub fn span(&self) -> Span {
        match self {
            Atom::Name(_, s) | Atom::Lit(_, s) => *s,
        }
    }
}

/// Pure expressions, used only in contracts and predicates.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Atom(Atom),
    Call(Call),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Int(String),
    Float(String),
    Str(String),
    Char(char),
    Bool(bool),
    Unit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstVal {
    Lit(Literal),
    Path {
        path: Path,
        fields: Option<Vec<(Name, ConstVal)>>,
    },
    List(Vec<ConstVal>),
    Tuple(Vec<ConstVal>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsmDir {
    In,
    Out,
    Inout,
    Lateout,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AsmReg {
    Class(Name),
    Explicit(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AsmOperand {
    Reg {
        dir: AsmDir,
        reg: AsmReg,
        value: Atom,
    },
    ClobberAbi(String),
    Options(Vec<Name>),
}
