//! High-level IR (22.2): the surface-neutral tree every surface lowers to and yan-elab lowers.
//! Built-in types carry their Yan Core names (16.3); surfaces desugar their own forms before HIR
//! (R558). Spans are the first token of each node.

pub mod raise;

use yan_diag::Span;

pub type Name = String;
pub type Path = Vec<Name>;

#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    /// Leading comment lines, kept by the printer (conformance headers).
    pub header: Vec<String>,
    pub attrs: Vec<Decorator>,
    pub doc: Option<String>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decorator {
    pub path: Path,
    pub args: Option<Vec<Arg>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub decorators: Vec<Decorator>,
    pub is_pub: bool,
    pub kind: ItemKind,
    pub span: Span,
    pub name_span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    Func(FuncDef),
    Lemma(FuncDef),
    /// Source lines raise prints as they are: an erased lemma (R870).
    Verbatim(String),
    Record(Record),
    Type(TypeDef),
    Trait(Trait),
    Impl(Impl),
    Const {
        name: Name,
        ty: Type,
        value: Expr,
    },
    /// Yan's `unsafe extern "C" lib name link "x"` (12.1).
    Extern(Extern),
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
        name_span: Span,
        params: Vec<XParam>,
        variadic: bool,
        ret: Option<(Type, Span)>,
        uses_none: bool,
        nonblocking: bool,
    },
    Static {
        safe: bool,
        name: Name,
        ty: Type,
        ty_span: Span,
    },
    Type {
        name: Name,
        drop: Name,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Callback {
    Scoped,
    Escaping,
}

#[derive(Clone, Debug, PartialEq)]
pub struct XParam {
    pub name: Name,
    pub callback: Option<Callback>,
    pub ty: Type,
    pub ty_span: Span,
}

impl ItemKind {
    pub fn name(&self) -> &str {
        match self {
            ItemKind::Func(f) | ItemKind::Lemma(f) => &f.name,
            ItemKind::Record(r) => &r.name,
            ItemKind::Type(t) => &t.name,
            ItemKind::Trait(t) => &t.name,
            ItemKind::Impl(_) => "impl",
            ItemKind::Verbatim(_) => "lemma",
            ItemKind::Const { name, .. } => name,
            ItemKind::Extern(x) => &x.lib,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FuncDef {
    pub name: Name,
    pub name_span: Span,
    pub tparams: Vec<TParam>,
    pub params: Vec<Param>,
    pub ret: Option<RetType>,
    pub uses: Option<Uses>,
    /// `None` for a trait signature written `: ...`.
    pub body: Option<Body>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetType {
    pub ty: Option<Type>,
    pub fail: Vec<Path>,
    pub from: Vec<Name>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Uses {
    None,
    Caps(Vec<CapRef>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapRef {
    pub path: Path,
    pub alias: Option<Name>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TParam {
    Type { name: Name, bounds: Vec<Bound> },
    Value { name: Name, ty: Type },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Bound {
    Path { path: Path, args: Vec<Type> },
    Nat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conv {
    Var,
    Take,
    Ghost,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Value {
        conv: Option<Conv>,
        name: Name,
        span: Span,
        ty: Type,
        default: Option<Expr>,
    },
    Slash,
    Pack {
        name: Name,
        ty: Type,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    pub doc: Option<String>,
    pub contracts: Vec<Contract>,
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Contract {
    Pre(Expr),
    Post(Expr),
    Decreases(Expr),
    Ex { expr: Expr, with: Vec<(Name, Expr)> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub name: Name,
    pub tparams: Vec<TParam>,
    pub doc: Option<String>,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub decorators: Vec<Decorator>,
    pub name: Name,
    pub ty: Type,
    /// The type annotation as written; the default span when the field has no source.
    pub ty_span: Span,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypeDef {
    pub name: Name,
    pub tparams: Vec<TParam>,
    pub rhs: TypeRhs,
    pub where_: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypeRhs {
    Sum(Vec<Variant>),
    Alias(Vec<Type>),
    Caps(Vec<CapRef>),
    Newtype(Type),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub decorators: Vec<Decorator>,
    pub name: Name,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trait {
    pub name: Name,
    pub tparams: Vec<TParam>,
    pub doc: Option<String>,
    pub assoc: Vec<Name>,
    pub sigs: Vec<(Vec<Decorator>, FuncDef)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Impl {
    pub trait_: Path,
    pub args: Vec<Type>,
    pub for_ty: Type,
    pub assoc: Vec<(Name, Type)>,
    pub fns: Vec<FuncDef>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    /// Built-in and user types alike, built-ins by their Yan Core names: `List[i64]` is `List` with
    /// one argument, `Option[T]` is an optional.
    Named {
        path: Path,
        args: Vec<TArg>,
    },
    Callable {
        par: bool,
        params: Vec<Type>,
        ret: Box<Type>,
        fail: Vec<Path>,
        uses: Vec<Name>,
    },
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TArg {
    Type(Type),
    Int(String),
    /// An error union in `Result[T, A | B]`, or a dimension in `Quantity[T, m / s]`.
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Expr(Expr),
    /// `targets` is one expression or a tuple of them.
    Assign {
        target: Expr,
        ty: Option<Type>,
        value: Expr,
    },
    AugAssign {
        target: Expr,
        op: &'static str,
        value: Expr,
    },
    Ghost {
        name: Name,
        ty: Option<Type>,
        value: Expr,
    },
    /// Yan's `let` and `var` (15.3): declared mutability instead of R421's inference.
    Bind {
        mutable: bool,
        target: Name,
        target_span: Span,
        ty: Option<Type>,
        value: Expr,
    },
    Return(Option<Expr>),
    Become(Expr),
    Fail(Expr),
    Break,
    Continue,
    BreakTo(Name),
    ContinueTo(Name),
    Pass,
    Assert(Expr),
    Unreachable,
    Reveal(Path),
    /// Yan's `unsafe` block (12).
    Unsafe(Vec<Stmt>),
    /// Yan's `#if` (10.4): only the taken branch is checked.
    StageIf {
        cond: Expr,
        then: Vec<Stmt>,
        els: Option<Vec<Stmt>>,
    },
    /// Yan's `#for`: unrolled over a stage-0 sequence.
    StageFor {
        target: Name,
        iter: Expr,
        body: Vec<Stmt>,
    },
    /// `asm(template, operands...)` inside `unsafe` (12.9).
    Asm {
        template: String,
        args: Vec<AsmArg>,
    },
    If {
        arms: Vec<(Expr, Block)>,
        els: Option<Block>,
    },
    While {
        label: Option<Name>,
        cond: Expr,
        contracts: Vec<LoopContract>,
        body: Vec<Stmt>,
    },
    For {
        label: Option<Name>,
        target: Vec<Name>,
        iter: Expr,
        contracts: Vec<LoopContract>,
        body: Vec<Stmt>,
    },
    Match {
        scrutinee: Expr,
        cases: Vec<Case>,
    },
    With {
        kind: WithKind,
        body: Block,
    },
    Par {
        concurrent: bool,
        limit: Option<Expr>,
        body: ParBody,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum AsmReg {
    Class(Name),
    /// An explicit register, with the span of its string literal.
    Explicit(String, Span),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AsmArg {
    Operand {
        dir: Name,
        reg: AsmReg,
        value: Expr,
    },
    Typed {
        dir: Name,
        reg: AsmReg,
        name: Name,
        ty: Type,
    },
    ClobberAbi(String),
    Option(Name),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LoopContract {
    Invariant(Expr),
    Decreases(Expr),
}

/// A block written on its header line (`if c: return x`) or indented below it.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub inline: bool,
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub pat: Pat,
    /// The pattern's first token.
    pub span: Span,
    pub guard: Option<Expr>,
    pub body: Block,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WithKind {
    Bind(Name, Expr),
    Deadline(Expr),
    Timeout(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParBody {
    Block(Vec<Stmt>),
    For {
        target: Vec<Name>,
        iter: Expr,
        contracts: Vec<LoopContract>,
        body: Vec<Stmt>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Pat {
    Wild,
    Lit(Expr),
    /// A name, a qualified constant, or a class pattern when `args` is present.
    Path {
        path: Path,
        args: Option<Vec<(Option<Name>, Pat)>>,
    },
    Tuple(Vec<Pat>),
    As(Box<Pat>, Name),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// Written inside parentheses; the printer recomputes parentheses, E101 reads this.
    pub paren: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Int(String),
    Float(String),
    Str { prefix: String, value: String },
    Char(char),
    Bool(bool),
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FPiece {
    Lit(String),
    Expr(Expr, Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompKind {
    List,
    Set,
    Dict,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Name(Name),
    Lit(Lit),
    FStr {
        template: bool,
        pieces: Vec<FPiece>,
    },
    /// `...`, an unfilled hole.
    Hole,
    Wild,
    Tuple(Vec<Expr>),
    List(Vec<Expr>),
    Set(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Comp {
        kind: CompKind,
        par: Option<bool>,
        elem: Box<Expr>,
        value: Option<Box<Expr>>,
        target: Vec<Name>,
        iter: Box<Expr>,
        cond: Option<Box<Expr>>,
    },
    /// The span is the attribute name's.
    Attr(Box<Expr>, Name, Span),
    Call(Box<Expr>, Vec<Arg>),
    Index(Box<Expr>, Box<Expr>),
    Slice(Box<Expr>, [Option<Box<Expr>>; 3], bool),
    Try(Box<Expr>),
    Unary(&'static str, Box<Expr>),
    Not(Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Compare(&'static str, Box<Expr>, Box<Expr>),
    Cond {
        then: Box<Expr>,
        cond: Box<Expr>,
        els: Box<Expr>,
    },
    Fallback(Box<Expr>, Box<Fallback>),
    Lambda(Vec<Name>, Box<Expr>),
    /// Yan's `try e`: the failure as a `Result` value.
    Reify(Box<Expr>),
    /// `e as T`.
    Cast(Box<Expr>, Type),
    /// Yan's `#e`: evaluated at stage 0 (10.4).
    Stage(Box<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fallback {
    Value(Expr),
    Bind(Name, Expr),
    Return(Option<Expr>),
    Fail(Expr),
    Break,
    Continue,
    /// A block that ends by exiting, with the failure's name when it binds one.
    Block(Option<Name>, Vec<Stmt>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub name: Option<Name>,
    pub kind: ArgKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ArgKind {
    Expr(Expr),
    /// `&place`, an inout argument.
    Ref(Expr),
    Become(Expr),
}
